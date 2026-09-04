//! `ConfluenceAnalyzerPass` — converts Confluence page observation artifacts
//! (RFC 0022) into KIR. Produces:
//! - `KirObject(kind=Custom("Page"))` per page
//! - `KirRelationship(kind=Contains)` from a parent page to each child page
//! - `KirRelationship(kind=References)` from a page to another page it links
//!   to via Confluence's own `content-title` link markup, when the linked
//!   title resolves within the same batch
//!
//! Pure structural mapping — no LLM in the loop, same shape as
//! `GitHubAnalyzerPass`.

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

/// Cap on the body text stored as the object's searchable `excerpt`
/// property — mirrors RFC 0014's `EXCERPT_MAX_CHARS`.
const BODY_EXCERPT_MAX_CHARS: usize = 600;

/// Confluence's own storage-format page-link tag looks like
/// `<ri:page ri:content-title="Some Page" />` — this is the attribute name
/// scanned for, not a full XML parse.
const LINK_MARKER: &str = "content-title=\"";

#[derive(Debug, Deserialize)]
struct PageData {
    id: String,
    space_key: String,
    title: String,
    body: String,
    #[serde(default)]
    parent_id: Option<String>,
}

/// Deterministic id for a page object — stable across passes and
/// `ekos recover` runs.
fn page_kir_id(space_key: &str, page_id: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("confluence:{space_key}:{page_id}").as_bytes(),
    ))
}

fn body_excerpt(body: &str) -> String {
    body.chars().take(BODY_EXCERPT_MAX_CHARS).collect()
}

/// Finds every `content-title="<title>"` occurrence in `body` (Confluence's
/// own page-link markup), deduplicated. Plain scanning, not an XML parse.
fn find_linked_titles(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    while let Some(pos) = body[start..].find(LINK_MARKER) {
        let abs = start + pos + LINK_MARKER.len();
        let rest = &body[abs..];
        let Some(end) = rest.find('"') else {
            break;
        };
        let title = rest[..end].to_string();
        if !title.is_empty() && !out.contains(&title) {
            out.push(title);
        }
        start = abs + end;
    }
    out
}

pub struct ConfluenceAnalyzerPass {
    pass_id: String,
    /// Confluence page ObservationArtifact IDs to process.
    page_artifact_ids: Vec<ArtifactId>,
}

impl ConfluenceAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, page_artifact_ids: Vec<ArtifactId>) -> Self {
        Self {
            pass_id: format!("confluence-analyzer:{}", workspace_name.into()),
            page_artifact_ids,
        }
    }
}

#[async_trait]
impl CompilerPass for ConfluenceAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .page_artifact_ids
            .iter()
            .map(|id| id.to_string())
            .collect();
        ids.sort();
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut graph = KirGraph::new();
        let mut page_ids: HashMap<String, KirId> = HashMap::new();
        let mut title_to_id: HashMap<String, KirId> = HashMap::new();
        let mut pages: Vec<PageData> = Vec::new();

        for artifact_id in &self.page_artifact_ids {
            let json = match ctx.artifact_store.read(artifact_id) {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "CONFLUENCE001",
                        format!("cannot read artifact {artifact_id}: {e}"),
                    );
                    continue;
                }
            };
            let data: PageData = match serde_json::from_value(json["data"].clone()) {
                Ok(d) => d,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "CONFLUENCE002",
                        format!("malformed confluence page payload in {artifact_id}: {e}"),
                    );
                    continue;
                }
            };
            pages.push(data);
        }

        if pages.is_empty() {
            return Ok(());
        }

        // First pass: one object per page.
        for data in &pages {
            let id = page_kir_id(&data.space_key, &data.id);
            page_ids.insert(data.id.clone(), id);
            title_to_id.insert(data.title.clone(), id);

            let mut obj = KirObject::new(
                format!("{}: {}", data.space_key, data.title),
                ObjectKind::Custom("Page".to_string()),
            );
            obj.id = id;
            obj.properties
                .insert("space_key".into(), serde_json::json!(data.space_key));
            obj.properties.insert(
                "excerpt".into(),
                serde_json::json!(body_excerpt(&data.body)),
            );
            graph.objects.push(obj);
        }

        // Second pass: Contains (hierarchy) + References (in-page links).
        for data in &pages {
            let page_id = page_ids[&data.id];

            if let Some(parent_id) = &data.parent_id {
                let parent_kir_id = *page_ids
                    .entry(parent_id.clone())
                    .or_insert_with(|| page_kir_id(&data.space_key, parent_id));
                let ev = KirEvidence::new(
                    SourceLocation::file(format!("confluence:{}:{}", data.space_key, data.id)),
                    format!("{} is a child of {parent_id}", data.title),
                );
                let ev_id = graph.add_evidence(ev);
                let mut rel = KirRelationship::deterministic(
                    RelationshipKind::Contains,
                    parent_kir_id,
                    page_id,
                    "",
                );
                rel.evidence.push(ev_id);
                graph.relationships.push(rel);
            }

            for linked_title in find_linked_titles(&data.body) {
                let Some(&to_id) = title_to_id.get(&linked_title) else {
                    continue; // outside this batch — not resolvable (RFC 0022 v1 scope)
                };
                let ev = KirEvidence::new(
                    SourceLocation::file(format!("confluence:{}:{}", data.space_key, data.id)),
                    format!("{} links to {linked_title}", data.title),
                );
                let ev_id = graph.add_evidence(ev);
                let mut rel = KirRelationship::deterministic(
                    RelationshipKind::References,
                    page_id,
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
            pages = knowledge.content.kir.objects.len(),
            edges = knowledge.content.kir.relationships.len(),
            "confluence-analyzer complete"
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

    fn seed_page(
        ctx: &PassContext,
        id: &str,
        title: &str,
        body: &str,
        parent_id: Option<&str>,
    ) -> ArtifactId {
        let data = serde_json::json!({
            "id": id,
            "space_key": "ENG",
            "title": title,
            "body": body,
            "parent_id": parent_id,
        });
        let artifact =
            ekos_artifact::ObservationArtifact::new("confluence", &format!("ENG:{id}"), data);
        let json = serde_json::to_value(&artifact).unwrap();
        ctx.artifact_store.write(&artifact.id, &json).unwrap();
        artifact.id
    }

    async fn run_pass(pages: Vec<(&str, &str, &str, Option<&str>)>) -> ekos_kir::KirGraph {
        let (c, _dir) = ctx();
        let mut ids = Vec::new();
        for (id, title, body, parent_id) in pages {
            ids.push(seed_page(&c, id, title, body, parent_id));
        }
        let mut pass = ConfluenceAnalyzerPass::new("test", ids);
        let mut c = c;
        pass.run(&mut c).await.unwrap();

        let knowledge_id = c
            .artifact_store
            .list()
            .unwrap()
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
    fn finds_content_title_links() {
        let body = r#"See <ri:page ri:content-title="Onboarding Guide" /> for details."#;
        assert_eq!(find_linked_titles(body), vec!["Onboarding Guide"]);
    }

    #[test]
    fn ignores_body_with_no_links() {
        assert!(find_linked_titles("just plain prose, no links here").is_empty());
    }

    #[tokio::test]
    async fn child_page_emits_contains_edge_from_parent() {
        let graph = run_pass(vec![
            ("1", "Overview", "top-level page", None),
            ("2", "Details", "child page", Some("1")),
        ])
        .await;
        assert_eq!(graph.objects.len(), 2);
        let parent_id = page_kir_id("ENG", "1");
        let child_id = page_kir_id("ENG", "2");
        assert!(
            graph
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Contains
                    && r.from == parent_id
                    && r.to == child_id)
        );
    }

    #[tokio::test]
    async fn intra_batch_link_emits_references_edge() {
        let graph = run_pass(vec![
            (
                "1",
                "Overview",
                r#"See <ri:page ri:content-title="Details" /> for more."#,
                None,
            ),
            ("2", "Details", "child page", None),
        ])
        .await;
        let overview_id = page_kir_id("ENG", "1");
        let details_id = page_kir_id("ENG", "2");
        assert!(
            graph
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::References
                    && r.from == overview_id
                    && r.to == details_id)
        );
    }

    #[tokio::test]
    async fn link_to_title_outside_batch_emits_no_edge() {
        let graph = run_pass(vec![(
            "1",
            "Overview",
            r#"See <ri:page ri:content-title="Unknown Page" /> for more."#,
            None,
        )])
        .await;
        assert!(graph.relationships.is_empty());
    }

    #[tokio::test]
    async fn same_page_across_two_runs_gets_same_object_id() {
        let graph1 = run_pass(vec![("1", "Overview", "body", None)]).await;
        let graph2 = run_pass(vec![("1", "Overview", "body", None)]).await;
        assert_eq!(graph1.objects[0].id, graph2.objects[0].id);
    }
}
