//! `GitHubAnalyzerPass` — converts GitHub issue/PR observation artifacts
//! (RFC 0020) into KIR. Produces:
//! - `KirObject(kind=Custom("Issue"|"PullRequest"))` per item
//! - `KirRelationship(kind=References)` from a PR to each file it changed
//! - `KirRelationship(kind=References)` from an item to another item its
//!   body closes (GitHub's own documented auto-close keywords)
//! - `KirRelationship(kind=References)` from an item to another item its
//!   body merely *mentions* by number, with no closing keyword (RFC 0062) —
//!   confirmed live against real `plausible/analytics` PR bodies to be the
//!   dominant real-world shape, not the keyword-qualified one: PR #3834's
//!   real body is literally `"Migration for #3828"`, no closing keyword
//!   anywhere. Without this edge, the closes-keyword-only scan misses most
//!   real cross-references in this actual repo. Distinguished from the
//!   closing edge only by its evidence text (`"mentions #N"` vs.
//!   `"...closing #N"`), not a separate `RelationshipKind` — same weight,
//!   weaker claim.
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
    /// RFC 0079 project qualification — `build.rs`'s central choke point stamps this onto every
    /// connector's artifact `data` in a multi-`[observe]`-paths workspace (absent entirely for the
    /// common single-project case). Was never read here: `file_kir_id(path)` below hashed the bare
    /// path with no project context, so a `References` edge landed on a `KirId` that no longer
    /// matched `build.rs`'s own project-qualified `File`-object id the moment a workspace had more
    /// than one `[observe]` path — silently wrong, not just untested.
    #[serde(default)]
    project: Option<String>,
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

/// Finds every `#<number>` occurrence anywhere in `body`, deduplicated, in
/// first-occurrence order — regardless of whether a closing keyword precedes
/// it (RFC 0062). Real PR bodies overwhelmingly use bare references
/// (`"Migration for #3828"`, `"see also #6514"`) rather than GitHub's
/// documented auto-close vocabulary; this catches those too. Plain scanning,
/// not a parser — same "misses non-standard phrasing, catches the common
/// case" tradeoff `find_closed_issue_numbers` already documents. A known,
/// accepted limitation: `#3828a1` matches as `3828` (digit-prefix scan, no
/// word-boundary check after the digits) — not fixed here, a real GFM
/// reference parser is out of scope for the one concrete gap this closes.
fn find_bare_issue_numbers(body: &str) -> Vec<u64> {
    let mut out = Vec::new();
    for (idx, ch) in body.char_indices() {
        if ch != '#' {
            continue;
        }
        let rest = &body[idx + 1..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<u64>()
            && !out.contains(&n)
        {
            out.push(n);
        }
    }
    out
}

/// Finds every `github.com/<owner>/<repo>/(pull|issues)/<number>` URL occurrence in `body`,
/// scoped to the same `owner`/`repo` this pass is already processing — a cross-repo URL would need
/// a different KIR item namespace entirely (`item_kir_id` is already scoped to one owner/repo per
/// pass), so it's deliberately not matched here. Real example (RFC 0062, `plausible/analytics`):
/// PR #6597's body is literally `"Extracted from
/// https://github.com/plausible/analytics/pull/6591"` — a full URL, no bare `#N` anywhere,
/// invisible to `find_bare_issue_numbers`. Case-insensitive (real URLs are consistently lowercase,
/// but the whole scan is deliberately robust to it anyway).
fn find_full_url_issue_numbers(body: &str, owner: &str, repo: &str) -> Vec<u64> {
    let lower = body.to_lowercase();
    let prefixes = [
        format!(
            "github.com/{}/{}/pull/",
            owner.to_lowercase(),
            repo.to_lowercase()
        ),
        format!(
            "github.com/{}/{}/issues/",
            owner.to_lowercase(),
            repo.to_lowercase()
        ),
    ];
    let mut out = Vec::new();
    for prefix in &prefixes {
        let mut start = 0;
        while let Some(pos) = lower[start..].find(prefix.as_str()) {
            let abs = start + pos;
            start = abs + prefix.len();
            let digits: String = lower[start..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
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
                let qualified_path =
                    ekos_common::project::project_qualify(path, data.project.as_deref());
                let mut rel = KirRelationship::deterministic(
                    RelationshipKind::References,
                    from_id,
                    file_kir_id(&qualified_path),
                    "",
                );
                rel.evidence.push(ev_id);
                graph.relationships.push(rel);
            }

            let closed_numbers = find_closed_issue_numbers(&data.body);
            for &closed_number in &closed_numbers {
                let to_id = *item_ids
                    .entry(closed_number)
                    .or_insert_with(|| item_kir_id(&owner, &repo, closed_number));
                let ev = KirEvidence::new(
                    SourceLocation::file(format!("github:{}/{}#{}", owner, repo, data.number)),
                    format!("#{} body references closing #{closed_number}", data.number),
                );
                let ev_id = graph.add_evidence(ev);
                let mut rel = KirRelationship::deterministic(
                    RelationshipKind::References,
                    from_id,
                    to_id,
                    "",
                );
                rel.evidence.push(ev_id);
                graph.relationships.push(rel);
            }

            let mut mentioned_numbers = find_bare_issue_numbers(&data.body);
            for n in find_full_url_issue_numbers(&data.body, &owner, &repo) {
                if !mentioned_numbers.contains(&n) {
                    mentioned_numbers.push(n);
                }
            }
            for mentioned_number in mentioned_numbers {
                if mentioned_number == data.number || closed_numbers.contains(&mentioned_number) {
                    continue;
                }
                let to_id = *item_ids
                    .entry(mentioned_number)
                    .or_insert_with(|| item_kir_id(&owner, &repo, mentioned_number));
                let ev = KirEvidence::new(
                    SourceLocation::file(format!("github:{}/{}#{}", owner, repo, data.number)),
                    format!("#{} body mentions #{mentioned_number}", data.number),
                );
                let ev_id = graph.add_evidence(ev);
                let mut rel = KirRelationship::deterministic(
                    RelationshipKind::References,
                    from_id,
                    to_id,
                    "",
                );
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

    struct SeedItem<'a> {
        owner: &'a str,
        repo: &'a str,
        number: u64,
        title: &'a str,
        body: &'a str,
        is_pr: bool,
        files: &'a [&'a str],
    }

    /// Tuple shape every `run_pass(vec![...])` call site below literally writes out —
    /// factored into a named type rather than touching each of those call sites.
    type SeedTuple<'a> = (&'a str, &'a str, u64, &'a str, &'a str, bool, Vec<&'a str>);

    fn seed_item(ctx: &PassContext, item: SeedItem) -> ArtifactId {
        let SeedItem {
            owner,
            repo,
            number,
            title,
            body,
            is_pr,
            files,
        } = item;
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
            format!("{owner}/{repo}#{number}"),
            data,
        );
        let json = serde_json::to_value(&artifact).unwrap();
        ctx.artifact_store.write(&artifact.id, &json).unwrap();
        artifact.id
    }

    async fn run_pass(items: Vec<SeedTuple<'_>>) -> ekos_kir::KirGraph {
        let (c, _dir) = ctx();
        let mut ids = Vec::new();
        for (owner, repo, number, title, body, is_pr, files) in items {
            ids.push(seed_item(
                &c,
                SeedItem {
                    owner,
                    repo,
                    number,
                    title,
                    body,
                    is_pr,
                    files: &files,
                },
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

    /// Real bug, found live: `file_kir_id(path)` hashed the *bare* path with no project
    /// qualification, even though `build.rs`'s central choke point stamps a `"project"` field
    /// onto every connector's artifact `data` in a multi-`[observe]`-paths workspace — so a
    /// `References` edge silently pointed at a `KirId` that no longer matched `build.rs`'s own
    /// project-qualified `File`-object id the moment a workspace had more than one `[observe]`
    /// path. This constructs the artifact directly (bypassing the shared `seed_item` test helper,
    /// which has no `project` field) to prove the qualified id is used once `data.project` is
    /// present, and matches `build.rs`'s own `id_key = format!("{project_key}:{rel_str}")` scheme
    /// exactly.
    #[tokio::test]
    async fn a_project_qualified_artifact_emits_a_project_qualified_file_reference() {
        let (ctx, _dir) = ctx();
        let data = serde_json::json!({
            "owner": "acme",
            "repo": "widgets",
            "number": 9,
            "title": "Fix crash",
            "body": "no closes here",
            "state": "open",
            "is_pull_request": true,
            "files_changed": ["src/lib.rs"],
            "project": "backend",
        });
        let artifact = ekos_artifact::ObservationArtifact::new("github", "acme/widgets#9", data);
        let json = serde_json::to_value(&artifact).unwrap();
        ctx.artifact_store.write(&artifact.id, &json).unwrap();

        let mut pass = GitHubAnalyzerPass::new("test", vec![artifact.id]);
        let mut c = ctx;
        pass.run(&mut c).await.unwrap();
        let ids: Vec<ArtifactId> = c.artifact_store.list().unwrap();
        let knowledge_json = ids
            .iter()
            .find_map(|id| {
                let j = c.artifact_store.read(id).unwrap()?;
                (j["artifact_type"] == "knowledge").then_some(j)
            })
            .unwrap();
        let graph: ekos_kir::KirGraph =
            serde_json::from_value(knowledge_json["kir"].clone()).unwrap();

        let rel = graph
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::References)
            .unwrap();
        let expected_id = ekos_kir::KirId(uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            b"backend:src/lib.rs",
        ));
        assert_eq!(
            rel.to, expected_id,
            "the References edge must land on build.rs's own project-qualified File id"
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
    async fn unrecognized_phrasing_emits_bare_mention_not_closing_edge() {
        // "addresses #1" has no closing keyword, but the number itself is a
        // real bare reference (RFC 0062) — must produce exactly one edge,
        // evidenced as a mention, not a closing.
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
        assert_eq!(graph.relationships.len(), 1);
        let ev_id = graph.relationships[0].evidence[0];
        let ev = graph.evidence.iter().find(|e| e.id == ev_id).unwrap();
        assert!(ev.fragment.contains("mentions #1"));
        assert!(!ev.fragment.contains("closing"));
    }

    #[tokio::test]
    async fn bare_mention_without_keyword_emits_reference_edge_real_data() {
        // Real bug (RFC 0062): PR #3834's actual body on plausible/analytics
        // is literally "Migration for #3828" — no closing keyword at all.
        // Confirmed live: before this fix, zero edges were produced.
        let graph = run_pass(vec![
            (
                "plausible",
                "analytics",
                3828,
                "Shield: Country Rules",
                "feature request",
                false,
                vec![],
            ),
            (
                "plausible",
                "analytics",
                3834,
                "Migration: add country rules",
                "Migration for #3828",
                true,
                vec!["priv/repo/migrations/20240221122626_shield_country_rules.exs"],
            ),
        ])
        .await;
        let pr_id = item_kir_id("plausible", "analytics", 3834);
        let issue_id = item_kir_id("plausible", "analytics", 3828);
        let rel = graph
            .relationships
            .iter()
            .find(|r| r.from == pr_id && r.to == issue_id)
            .expect("expected a References edge from PR #3834 to issue #3828");
        let ev = graph
            .evidence
            .iter()
            .find(|e| e.id == rel.evidence[0])
            .unwrap();
        assert!(ev.fragment.contains("mentions #3828"));
    }

    #[tokio::test]
    async fn closes_keyword_hit_does_not_also_emit_duplicate_bare_edge() {
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
        let pr_id = item_kir_id("acme", "widgets", 2);
        let issue_id = item_kir_id("acme", "widgets", 1);
        let edges: Vec<_> = graph
            .relationships
            .iter()
            .filter(|r| r.from == pr_id && r.to == issue_id)
            .collect();
        assert_eq!(
            edges.len(),
            1,
            "a closes-keyword match must not also emit a separate bare-mention edge"
        );
    }

    #[tokio::test]
    async fn self_mention_does_not_emit_self_loop_edge() {
        let graph = run_pass(vec![(
            "acme",
            "widgets",
            5,
            "Title",
            "see also #5 for context",
            false,
            vec![],
        )])
        .await;
        assert!(graph.relationships.is_empty());
    }

    #[test]
    fn find_full_url_issue_numbers_matches_pull_and_issues_urls() {
        assert_eq!(
            find_full_url_issue_numbers(
                "Extracted from https://github.com/plausible/analytics/pull/6591",
                "plausible",
                "analytics",
            ),
            vec![6591]
        );
        assert_eq!(
            find_full_url_issue_numbers(
                "see https://github.com/plausible/analytics/issues/42 for context",
                "plausible",
                "analytics",
            ),
            vec![42]
        );
    }

    #[test]
    fn find_full_url_issue_numbers_ignores_a_different_repo() {
        assert!(
            find_full_url_issue_numbers(
                "see https://github.com/other/project/pull/6591",
                "plausible",
                "analytics",
            )
            .is_empty()
        );
    }

    #[tokio::test]
    async fn full_url_mention_without_bare_hash_emits_reference_edge_real_data() {
        // Real body (RFC 0062 follow-up): plausible/analytics PR #6597's actual body is
        // "Extracted from https://github.com/plausible/analytics/pull/6591" — a full URL, no
        // bare `#N` anywhere, previously invisible to `find_bare_issue_numbers`.
        let graph = run_pass(vec![
            (
                "plausible",
                "analytics",
                6591,
                "Original PR",
                "the original change",
                true,
                vec![],
            ),
            (
                "plausible",
                "analytics",
                6597,
                "Follow-up",
                "Extracted from https://github.com/plausible/analytics/pull/6591",
                true,
                vec![],
            ),
        ])
        .await;
        let from_id = item_kir_id("plausible", "analytics", 6597);
        let to_id = item_kir_id("plausible", "analytics", 6591);
        assert!(
            graph
                .relationships
                .iter()
                .any(|r| r.from == from_id && r.to == to_id),
            "expected a References edge from PR #6597 to PR #6591 via the full URL mention"
        );
    }

    #[tokio::test]
    async fn full_url_mention_of_an_already_bare_mentioned_number_is_not_duplicated() {
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
            (
                "acme",
                "widgets",
                2,
                "Fix",
                "see #1 and also https://github.com/acme/widgets/issues/1",
                true,
                vec![],
            ),
        ])
        .await;
        let from_id = item_kir_id("acme", "widgets", 2);
        let to_id = item_kir_id("acme", "widgets", 1);
        let edges: Vec<_> = graph
            .relationships
            .iter()
            .filter(|r| r.from == from_id && r.to == to_id)
            .collect();
        assert_eq!(
            edges.len(),
            1,
            "a bare mention and a full-URL mention of the same number must not double-emit"
        );
    }

    #[tokio::test]
    async fn same_item_across_two_runs_gets_same_object_id() {
        let graph1 = run_pass(vec![("acme", "widgets", 5, "Title", "body", false, vec![])]).await;
        let graph2 = run_pass(vec![("acme", "widgets", 5, "Title", "body", false, vec![])]).await;
        assert_eq!(graph1.objects[0].id, graph2.objects[0].id);
    }
}
