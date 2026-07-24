//! `GitHubAnalyzerPass` — converts GitHub issue/PR observation artifacts
//! (RFC 0020) into KIR. Produces:
//! - `KirObject(kind=Custom("Issue"|"PullRequest"))` per item
//! - `KirRelationship(kind=References)` from a PR to each file it changed
//! - `KirRelationship(kind=References)` from an item to another item its
//!   body closes (GitHub's own documented auto-close keywords)
//!
//! Pure structural mapping — no LLM in the loop, same shape as
//! `CryptoAnalyzerPass`.

use std::collections::HashMap;

use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use serde::Deserialize;
use uuid::Uuid;

/// GitHub's own documented auto-close keywords (case-insensitive), checked
/// as plain substrings followed by `#<number>` — not a GitHub-flavored-
/// markdown parser. Same tradeoff RFC 0019's dependency-pattern matching
/// makes: misses non-standard phrasing, catches the documented common case.
const CLOSE_KEYWORDS: &[&str] = &[
    "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved",
];

/// Cap on the body text stored as the object's searchable `excerpt`
/// property — mirrors RFC 0014's `EXCERPT_MAX_CHARS`.
const BODY_EXCERPT_MAX_CHARS: usize = 600;

#[derive(Debug, Deserialize)]
struct ItemData {
    owner: String,
    repo: String,
    number: u64,
    title: String,
    body: String,
    state: String,
    is_pull_request: bool,
    #[serde(default)]
    files_changed: Vec<String>,
}

/// Deterministic id for an issue/PR object — stable across passes and
/// `ekos recover` runs.
fn item_kir_id(owner: &str, repo: &str, number: u64) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("github:{owner}/{repo}#{number}").as_bytes(),
    ))
}

/// Deterministic id for a file object — matches `build.rs`'s scheme
/// (also reused by RFC 0019's `DependencyAnalyzerPass`) so a `References`
/// edge lands on the same object `ekos_search`/`ekos_impact` resolve, when
/// that file has been observed by `ekos build`.
fn file_kir_id(rel_path: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, rel_path.as_bytes()))
}

/// Finds every `<keyword> #<number>` occurrence in `body` (case-insensitive),
/// deduplicated. Plain scanning, not a parser.
fn find_closed_issue_numbers(body: &str) -> Vec<u64> {
    let lower = body.to_lowercase();
    let mut out = Vec::new();
    for keyword in CLOSE_KEYWORDS {
        let mut start = 0;
        while let Some(pos) = lower[start..].find(keyword) {
            let abs = start + pos;
            start = abs + keyword.len();
            let after = lower[start..].trim_start();
            let Some(rest) = after.strip_prefix('#') else {
                continue;
            };
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<u64>()
                && !out.contains(&n)
            {
                out.push(n);
            }
        }
    }
    out
}

fn body_excerpt(body: &str) -> String {
    body.chars().take(BODY_EXCERPT_MAX_CHARS).collect()
}

pub struct GitHubAnalyzerPass {
    pass_id: String,
    /// GitHub issue/PR ObservationArtifact IDs to process.
    item_artifact_ids: Vec<ArtifactId>,
}

impl GitHubAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, item_artifact_ids: Vec<ArtifactId>) -> Self {
        Self {
            pass_id: format!("github-analyzer:{}", workspace_name.into()),
            item_artifact_ids,
        }
    }
}

#[async_trait]
impl CompilerPass for GitHubAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .item_artifact_ids
            .iter()
            .map(|id| id.to_string())
            .collect();
        ids.sort();
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut graph = KirGraph::new();
        let mut item_ids: HashMap<u64, KirId> = HashMap::new();
        let mut items: Vec<ItemData> = Vec::new();

        for artifact_id in &self.item_artifact_ids {
            let json = match ctx.artifact_store.read(artifact_id) {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "GITHUB001",
                        format!("cannot read artifact {artifact_id}: {e}"),
                    );
                    continue;
                }
            };
            let data: ItemData = match serde_json::from_value(json["data"].clone()) {
                Ok(d) => d,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "GITHUB002",
                        format!("malformed github item payload in {artifact_id}: {e}"),
                    );
                    continue;
                }
            };
            items.push(data);
        }

        if items.is_empty() {
            return Ok(());
        }

        let owner = items[0].owner.clone();
        let repo = items[0].repo.clone();

        // First pass: one object per item.
        for data in &items {
            let id = item_kir_id(&data.owner, &data.repo, data.number);
            item_ids.insert(data.number, id);

            let kind = ObjectKind::Custom(
                (if data.is_pull_request {
                    "PullRequest"
                } else {
                    "Issue"
                })
                .to_string(),
            );
            let mut obj = KirObject::new(
                format!(
                    "{}/{}#{}: {}",
                    data.owner, data.repo, data.number, data.title
                ),
                kind,
            );
            obj.id = id;
            obj.properties
                .insert("number".into(), serde_json::json!(data.number));
            obj.properties
                .insert("state".into(), serde_json::json!(data.state));
            obj.properties.insert(
                "excerpt".into(),
                serde_json::json!(body_excerpt(&data.body)),
            );
            graph.objects.push(obj);
        }

        // Second pass: References edges (file changes, closes-keywords).
        for data in &items {
            let from_id = item_ids[&data.number];

            for path in &data.files_changed {
                let ev = KirEvidence::new(
                    SourceLocation::file(format!("github:{}/{}#{}", owner, repo, data.number)),
                    format!("PR #{} changes {path}", data.number),
                );
                let ev_id = graph.add_evidence(ev);
                let mut rel =
                    KirRelationship::new(RelationshipKind::References, from_id, file_kir_id(path));
                rel.evidence.push(ev_id);
                graph.relationships.push(rel);
            }

            for closed_number in find_closed_issue_numbers(&data.body) {
                let to_id = *item_ids
                    .entry(closed_number)
                    .or_insert_with(|| item_kir_id(&owner, &repo, closed_number));
                let ev = KirEvidence::new(
                    SourceLocation::file(format!("github:{}/{}#{}", owner, repo, data.number)),
                    format!("#{} body references closing #{closed_number}", data.number),
                );
                let ev_id = graph.add_evidence(ev);
                let mut rel = KirRelationship::new(RelationshipKind::References, from_id, to_id);
                rel.evidence.push(ev_id);
                graph.relationships.push(rel);
            }
        }

        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], graph);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            items = knowledge.content.kir.objects.len(),
            edges = knowledge.content.kir.relationships.len(),
            "github-analyzer complete"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_compiler_core::{EkosConfig, pass::PassContext};
    use std::sync::Arc;

    fn ctx() -> (PassContext, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (
            PassContext::new(Arc::new(EkosConfig::default()), dir.path().to_path_buf()),
            dir,
        )
    }

    fn seed_item(
        ctx: &PassContext,
        owner: &str,
        repo: &str,
        number: u64,
        title: &str,
        body: &str,
        is_pr: bool,
        files: &[&str],
    ) -> ArtifactId {
        let data = serde_json::json!({
            "owner": owner,
            "repo": repo,
            "number": number,
            "title": title,
            "body": body,
            "state": "open",
            "is_pull_request": is_pr,
            "files_changed": files,
        });
        let artifact = ekos_artifact::ObservationArtifact::new(
            "github",
            &format!("{owner}/{repo}#{number}"),
            data,
        );
        let json = serde_json::to_value(&artifact).unwrap();
        ctx.artifact_store.write(&artifact.id, &json).unwrap();
        artifact.id
    }

    async fn run_pass(
        items: Vec<(&str, &str, u64, &str, &str, bool, Vec<&str>)>,
    ) -> ekos_kir::KirGraph {
        let (c, _dir) = ctx();
        let mut ids = Vec::new();
        for (owner, repo, number, title, body, is_pr, files) in items {
            ids.push(seed_item(
                &c, owner, repo, number, title, body, is_pr, &files,
            ));
        }
        let mut pass = GitHubAnalyzerPass::new("test", ids);
        let mut c = c;
        pass.run(&mut c).await.unwrap();

        let before: std::collections::HashSet<_> =
            c.artifact_store.list().unwrap().into_iter().collect();
        // The KnowledgeArtifact is the one entry whose payload deserializes
        // as one (item artifacts don't have a `kir` field).
        let knowledge_id = before
            .into_iter()
            .find(|id| {
                let json = c.artifact_store.read(id).unwrap().unwrap();
                json.get("kir").is_some()
            })
            .expect("pass must have written a KnowledgeArtifact");
        let json = c.artifact_store.read(&knowledge_id).unwrap().unwrap();
        let knowledge: ekos_artifact::KnowledgeArtifact = serde_json::from_value(json).unwrap();
        knowledge.content.kir
    }

    #[test]
    fn finds_closes_keyword_case_insensitively() {
        assert_eq!(find_closed_issue_numbers("Fixes #7"), vec![7]);
        assert_eq!(
            find_closed_issue_numbers("this CLOSES #3 for real"),
            vec![3]
        );
        assert_eq!(find_closed_issue_numbers("resolved #10"), vec![10]);
    }

    #[test]
    fn ignores_unrecognized_phrasing() {
        assert!(find_closed_issue_numbers("this addresses #12").is_empty());
        assert!(find_closed_issue_numbers("no reference here").is_empty());
    }

    #[tokio::test]
    async fn pr_files_changed_emit_references_edges() {
        let graph = run_pass(vec![(
            "acme",
            "widgets",
            2,
            "Fix crash",
            "no closes here",
            true,
            vec!["src/lib.rs", "src/main.rs"],
        )])
        .await;
        assert_eq!(graph.objects.len(), 1);
        assert_eq!(graph.relationships.len(), 2);
        assert!(
            graph
                .relationships
                .iter()
                .all(|r| r.kind == RelationshipKind::References)
        );
    }

    #[tokio::test]
    async fn body_closes_keyword_emits_reference_to_issue() {
        let graph = run_pass(vec![
            (
                "acme",
                "widgets",
                1,
                "Bug report",
                "it crashes",
                false,
                vec![],
            ),
            ("acme", "widgets", 2, "Fix crash", "Fixes #1", true, vec![]),
        ])
        .await;
        assert_eq!(graph.objects.len(), 2);
        let pr_id = item_kir_id("acme", "widgets", 2);
        let issue_id = item_kir_id("acme", "widgets", 1);
        assert!(
            graph
                .relationships
                .iter()
                .any(|r| r.from == pr_id && r.to == issue_id)
        );
    }

    #[tokio::test]
    async fn unrecognized_phrasing_emits_no_closing_edge() {
        let graph = run_pass(vec![(
            "acme",
            "widgets",
            2,
            "Fix crash",
            "this addresses #1",
            true,
            vec![],
        )])
        .await;
        assert!(graph.relationships.is_empty());
    }

    #[tokio::test]
    async fn same_item_across_two_runs_gets_same_object_id() {
        let graph1 = run_pass(vec![("acme", "widgets", 5, "Title", "body", false, vec![])]).await;
        let graph2 = run_pass(vec![("acme", "widgets", 5, "Title", "body", false, vec![])]).await;
        assert_eq!(graph1.objects[0].id, graph2.objects[0].id);
    }
}
