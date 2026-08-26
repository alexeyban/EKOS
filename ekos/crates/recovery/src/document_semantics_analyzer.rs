//! `DocumentSemanticsAnalyzerPass` — RFC 0026. Reads the `Custom("Section")`
//! objects produced by `LocalDocAnalyzerPass` (RFC 0023/0024) and puts each
//! section's prose through an LLM to extract the real-world concepts it
//! discusses and the relationships between them. Produces:
//! - `KirObject(kind=Custom("Concept"))` per extracted concept, carrying the
//!   LLM's one-sentence description in `properties["excerpt"]` — the property
//!   `KirObject::indexed_content()` reads, i.e. what makes a Concept findable
//!   through `ekos_search`/`ekos ask`
//! - `KirRelationship(kind=References)` from the source Section to each Concept
//! - `KirRelationship(kind=Custom(<verb phrase>))` between Concepts
//!
//! Unlike `SqlAnalyzerPass`, which only *enriches* objects a structural pass
//! already created, this pass creates objects outright: free prose has no
//! schema, so there is no pre-existing per-concept object to match against.
//!
//! Opt-in via `[document-semantics] enabled = true` — it makes one LLM call per
//! section.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use uuid::Uuid;

use crate::llm::{LlmProvider, LlmRequest};
use crate::llm_json::strip_json_fences;

const SYSTEM_PROMPT: &str = r#"You are a knowledge-extraction assistant. Given a passage of
prose from an enterprise document, identify the real-world entities/concepts it discusses and
the relationships between them.

Respond ONLY with valid JSON in this exact schema — no markdown fences, no commentary:
{
  "concepts": [{"name": "<canonical short name>", "description": "<one sentence>"}],
  "relationships": [{"from": "<concept name>", "to": "<concept name>", "kind": "<snake_case verb phrase>", "description": "<one sentence>"}]
}"#;

const PROMPT_VERSION: &str = "doc-semantics-v1";

/// Deterministic id for a concept, scoped per (section, concept) rather than
/// globally per concept name.
///
/// This is load-bearing, not just consistency with `local_docs_analyzer.rs`'s
/// `section_kir_id`: `Ledger::append_object` versions by `(id,
/// content_signature)`, so if two genuinely different mentions of the same
/// concept shared one id, the second write would silently version-overwrite the
/// first instead of leaving `DefaultResolver` two pending objects to propose a
/// merge between. Per-mention ids keep each source mention a distinct,
/// separately evidenced object, and make the merge explicit and auditable.
fn concept_kir_id(section_path: &str, section_index: usize, normalized_name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("docsem:{section_path}:section:{section_index}:concept:{normalized_name}")
            .as_bytes(),
    ))
}

/// Trim and collapse internal whitespace. Deliberately case-preserving: the
/// canonical casing the LLM chose is what a reader sees, and identity
/// resolution lowercases on its own before comparing.
fn normalize_concept_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, serde::Deserialize)]
struct LlmOutput {
    #[serde(default)]
    concepts: Vec<LlmConcept>,
    #[serde(default)]
    relationships: Vec<LlmRelationship>,
}

#[derive(Debug, serde::Deserialize)]
struct LlmConcept {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, serde::Deserialize)]
struct LlmRelationship {
    from: String,
    to: String,
    kind: String,
    #[serde(default)]
    description: String,
}

/// Counts from one run, readable by the caller after the pass has been consumed
/// by the `PassManager`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DocumentSemanticsStats {
    pub sections_processed: usize,
    pub concepts: usize,
    pub relationships: usize,
}

/// One `Custom("Section")` object resolved against its owning document.
struct SectionInput {
    id: KirId,
    /// Path of the document that contains this section — the id-scheme's
    /// `{path}` component, taken from the owning `Custom("Document")` object.
    path: String,
    index: usize,
    excerpt: String,
}

pub struct DocumentSemanticsAnalyzerPass {
    pass_id: String,
    local_docs_pass_id: String,
    /// `CompilerPass::dependencies` hands out `&[&str]`, which cannot borrow
    /// from a `String` field. The dependency name is known only at runtime, so
    /// it is leaked once per pass instance (a few dozen bytes, for the process
    /// lifetime) rather than widening the trait signature.
    deps: Vec<&'static str>,
    llm: Arc<dyn LlmProvider>,
    max_sections: Option<u32>,
    stats: Arc<Mutex<DocumentSemanticsStats>>,
}

impl DocumentSemanticsAnalyzerPass {
    pub fn new(local_docs_pass_id: impl Into<String>, llm: Arc<dyn LlmProvider>) -> Self {
        let local_docs_pass_id = local_docs_pass_id.into();
        let dep: &'static str = Box::leak(local_docs_pass_id.clone().into_boxed_str());
        Self {
            pass_id: format!("document-semantics-analyzer:{local_docs_pass_id}"),
            local_docs_pass_id,
            deps: vec![dep],
            llm,
            max_sections: None,
            stats: Arc::new(Mutex::new(DocumentSemanticsStats::default())),
        }
    }

    pub fn with_max_sections(mut self, max_sections: Option<u32>) -> Self {
        self.max_sections = max_sections;
        self
    }

    /// Handle onto this pass's counters, for printing a summary after the
    /// `PassManager` has taken ownership of the pass.
    pub fn stats_handle(&self) -> Arc<Mutex<DocumentSemanticsStats>> {
        Arc::clone(&self.stats)
    }

    /// Locate this pass's input by scanning the artifact store for
    /// `KnowledgeArtifact`s tagged with the `LocalDocAnalyzerPass` instance this
    /// pass depends on. `KnowledgeArtifact::id` is content-addressed and so is
    /// not derivable from the producing pass's name, but `pass_name` is stored
    /// verbatim inside the artifact.
    fn collect_sections(&self, ctx: &PassContext) -> Vec<SectionInput> {
        let ids = match ctx.artifact_store.list() {
            Ok(ids) => ids,
            Err(e) => {
                ctx.diagnostics
                    .lock()
                    .unwrap()
                    .warning("DOCSEM001", format!("cannot list artifact store: {e}"));
                return Vec::new();
            }
        };

        let mut sections = Vec::new();
        for id in ids {
            let Ok(Some(json)) = ctx.artifact_store.read(&id) else {
                continue;
            };
            if json.get("pass_name").and_then(|v| v.as_str()) != Some(&self.local_docs_pass_id) {
                continue;
            }
            let Ok(artifact) =
                serde_json::from_value::<ekos_artifact::KnowledgeArtifact>(json.clone())
            else {
                ctx.diagnostics.lock().unwrap().warning(
                    "DOCSEM002",
                    format!("malformed KnowledgeArtifact {id} from {}", self.pass_id),
                );
                continue;
            };
            sections.extend(sections_from_graph(&artifact.content.kir));
        }
        // Content-addressed ids come back in arbitrary order; the pass must be
        // deterministic, including which sections `max_sections` truncates.
        sections.sort_by(|a, b| (&a.path, a.index).cmp(&(&b.path, b.index)));
        sections
    }
}

/// Pull every `Custom("Section")` object out of a graph, resolving each one's
/// document path through the `Contains` edge that owns it.
fn sections_from_graph(graph: &KirGraph) -> Vec<SectionInput> {
    let doc_paths: HashMap<KirId, String> = graph
        .objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == "Document"))
        .filter_map(|o| {
            let path = o.properties.get("path")?.as_str()?.to_string();
            Some((o.id, path))
        })
        .collect();

    let owner: HashMap<KirId, &String> = graph
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Contains)
        .filter_map(|r| Some((r.to, doc_paths.get(&r.from)?)))
        .collect();

    graph
        .objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == "Section"))
        .map(|o| SectionInput {
            id: o.id,
            // A Section with no owning Document is malformed input rather than
            // a reason to skip it; its own name still scopes the id uniquely.
            path: owner
                .get(&o.id)
                .map(|p| p.to_string())
                .unwrap_or_else(|| o.name.clone()),
            index: o
                .properties
                .get("section_index")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize,
            excerpt: o
                .properties
                .get("excerpt")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        })
        .collect()
}

#[async_trait]
impl CompilerPass for DocumentSemanticsAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn dependencies(&self) -> &[&str] {
        &self.deps
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut sections = self.collect_sections(ctx);
        if let Some(max) = self.max_sections {
            sections.truncate(max as usize);
        }

        let mut graph = KirGraph::new();
        let mut stats = DocumentSemanticsStats::default();

        for section in &sections {
            if section.excerpt.trim().is_empty() {
                continue;
            }

            let req = LlmRequest {
                system: SYSTEM_PROMPT,
                user: &section.excerpt,
                prompt_version: PROMPT_VERSION,
                max_tokens: 2048,
                history: &[],
            };

            let resp = match self.llm.complete(&req).await {
                Ok(resp) => resp,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "DOCSEM003",
                        format!(
                            "LLM call failed for {} section {} (skipped): {e}",
                            section.path, section.index
                        ),
                    );
                    continue;
                }
            };

            let output: LlmOutput = match serde_json::from_str(strip_json_fences(&resp.content)) {
                Ok(o) => o,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "DOCSEM004",
                        format!(
                            "LLM response parse failed for {} section {} (skipped): {e}",
                            section.path, section.index
                        ),
                    );
                    continue;
                }
            };

            stats.sections_processed += 1;
            let mut by_name: HashMap<String, KirId> = HashMap::new();

            for concept in &output.concepts {
                let name = normalize_concept_name(&concept.name);
                if name.is_empty() {
                    continue;
                }
                let concept_id = concept_kir_id(&section.path, section.index, &name);
                if by_name.contains_key(&name) {
                    continue;
                }

                let ev = KirEvidence::new(
                    SourceLocation::file(section.path.clone()),
                    format!(
                        "concept '{name}' extracted from {} section {} by {PROMPT_VERSION}",
                        section.path,
                        section.index + 1
                    ),
                );
                let ev_id = graph.add_evidence(ev);

                let mut obj = KirObject::new(name.clone(), ObjectKind::Custom("Concept".into()));
                obj.id = concept_id;
                // `excerpt` specifically: it is the property `indexed_content()`
                // reads, so anything written under another key is invisible to
                // `ekos_search`.
                obj.properties
                    .insert("excerpt".into(), serde_json::json!(concept.description));
                obj.properties.insert(
                    "source_prompt_version".into(),
                    serde_json::json!(PROMPT_VERSION),
                );
                obj.evidence.push(ev_id);
                graph.objects.push(obj);
                stats.concepts += 1;

                let mut rel =
                    KirRelationship::new(RelationshipKind::References, section.id, concept_id);
                rel.evidence.push(ev_id);
                graph.relationships.push(rel);

                by_name.insert(name, concept_id);
            }

            for rel in &output.relationships {
                let from = normalize_concept_name(&rel.from);
                let to = normalize_concept_name(&rel.to);
                let (Some(&from_id), Some(&to_id)) = (by_name.get(&from), by_name.get(&to)) else {
                    ctx.diagnostics.lock().unwrap().warning(
                        "DOCSEM005",
                        format!(
                            "relationship '{from}' -> '{to}' names a concept absent from the same \
                             extraction for {} section {} (skipped)",
                            section.path, section.index
                        ),
                    );
                    continue;
                };

                let ev = KirEvidence::new(
                    SourceLocation::file(section.path.clone()),
                    format!(
                        "relationship '{from}' -{}-> '{to}' extracted from {} section {} by \
                         {PROMPT_VERSION}",
                        rel.kind,
                        section.path,
                        section.index + 1
                    ),
                );
                let ev_id = graph.add_evidence(ev);

                let mut edge = KirRelationship::new(
                    RelationshipKind::Custom(rel.kind.clone()),
                    from_id,
                    to_id,
                );
                edge.properties
                    .insert("description".into(), serde_json::json!(rel.description));
                edge.evidence.push(ev_id);
                graph.relationships.push(edge);
                stats.relationships += 1;
            }
        }

        *self.stats.lock().unwrap() = stats;

        if graph.objects.is_empty() {
            return Ok(());
        }

        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], graph);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            sections = stats.sections_processed,
            concepts = stats.concepts,
            relationships = stats.relationships,
            "document-semantics-analyzer complete"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockLlmProvider;
    use crate::local_docs_analyzer::LocalDocAnalyzerPass;
    use ekos_compiler_core::{EkosConfig, pass::PassContext};

    const LOCAL_DOCS_PASS_ID: &str = "local-docs-analyzer:test";

    fn ctx() -> (PassContext, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (
            PassContext::new(Arc::new(EkosConfig::default()), dir.path().to_path_buf()),
            dir,
        )
    }

    /// Seed the store the way the real pipeline does: run the actual
    /// `LocalDocAnalyzerPass` over an observation artifact, so this pass is
    /// tested against genuine upstream output rather than a hand-built graph
    /// that could drift from it.
    async fn seed_sections(ctx: &PassContext, path: &str, sections: serde_json::Value) {
        let data = serde_json::json!({
            "path": path,
            "doc_format": "pdf",
            "page_count": 3,
            "excerpt": "cover page",
            "tables": [],
            "sections": sections,
            "ocr_text": null,
        });
        let artifact = ekos_artifact::ObservationArtifact::new("localdocs", path, data);
        ctx.artifact_store
            .write(&artifact.id, &serde_json::to_value(&artifact).unwrap())
            .unwrap();

        let mut pass = LocalDocAnalyzerPass::new("test", vec![artifact.id]);
        let mut ctx = ctx.clone();
        pass.run(&mut ctx).await.unwrap();
    }

    /// Read back the `KnowledgeArtifact` this pass wrote, identified by
    /// `pass_name` (artifact ids are content-addressed, not derivable).
    fn read_output(ctx: &PassContext, pass_id: &str) -> Option<KirGraph> {
        let id = ctx.artifact_store.list().unwrap().into_iter().find(|id| {
            ctx.artifact_store
                .read(id)
                .unwrap()
                .and_then(|j| j.get("pass_name")?.as_str().map(|s| s == pass_id))
                .unwrap_or(false)
        })?;
        let json = ctx.artifact_store.read(&id).unwrap().unwrap();
        let artifact: ekos_artifact::KnowledgeArtifact = serde_json::from_value(json).unwrap();
        Some(artifact.content.kir)
    }

    fn two_concept_response() -> String {
        serde_json::json!({
            "concepts": [
                {"name": "Data Replication", "description": "Copying data between stores to keep them consistent."},
                {"name": "Eventual Consistency", "description": "Replicas converge to the same state over time."}
            ],
            "relationships": [
                {"from": "Data Replication", "to": "Eventual Consistency",
                 "kind": "results_in", "description": "Asynchronous replication yields eventual consistency."}
            ]
        })
        .to_string()
    }

    #[tokio::test]
    async fn extracts_concepts_edges_and_evidence_from_a_section() {
        let (c, _dir) = ctx();
        seed_sections(
            &c,
            "Cloud Design Patterns.pdf",
            serde_json::json!([
                { "index": 0, "page": 213, "text": "replication keeps copies in sync" }
            ]),
        )
        .await;

        let mock = Arc::new(MockLlmProvider::new(two_concept_response()));
        let mut pass = DocumentSemanticsAnalyzerPass::new(LOCAL_DOCS_PASS_ID, mock);
        let stats = pass.stats_handle();
        let pass_id = pass.name().to_string();
        let mut c2 = c.clone();
        pass.run(&mut c2).await.unwrap();

        assert!(!c.diagnostics.lock().unwrap().has_errors());
        let graph = read_output(&c, &pass_id).expect("pass must write a KnowledgeArtifact");

        let concepts: Vec<_> = graph
            .objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Custom("Concept".into()))
            .collect();
        assert_eq!(concepts.len(), 2);
        assert!(
            concepts.iter().all(|o| !o.evidence.is_empty()),
            "every Concept must carry evidence"
        );

        let replication = concepts
            .iter()
            .find(|o| o.name == "Data Replication")
            .unwrap();
        assert!(
            replication
                .indexed_content()
                .to_lowercase()
                .contains("copying data"),
            "the LLM description must land in properties[\"excerpt\"] to be searchable"
        );
        assert_eq!(
            replication.properties["source_prompt_version"],
            serde_json::json!(PROMPT_VERSION)
        );

        let refs: Vec<_> = graph
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::References)
            .collect();
        assert_eq!(refs.len(), 2, "one Section->Concept edge per concept");
        assert!(refs.iter().all(|r| !r.evidence.is_empty()));

        let custom: Vec<_> = graph
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Custom("results_in".into()))
            .collect();
        assert_eq!(custom.len(), 1, "Concept<->Concept edge keeps its own kind");
        assert!(!custom[0].evidence.is_empty());

        let stats = *stats.lock().unwrap();
        assert_eq!(stats.sections_processed, 1);
        assert_eq!(stats.concepts, 2);
        assert_eq!(stats.relationships, 1);
    }

    #[tokio::test]
    async fn pass_tolerates_bad_llm_json() {
        let (c, _dir) = ctx();
        seed_sections(
            &c,
            "spec.pdf",
            serde_json::json!([{ "index": 0, "page": 1, "text": "some prose" }]),
        )
        .await;

        let mock = Arc::new(MockLlmProvider::new("not valid json at all!!"));
        let mut pass = DocumentSemanticsAnalyzerPass::new(LOCAL_DOCS_PASS_ID, mock);
        let stats = pass.stats_handle();
        let pass_id = pass.name().to_string();
        let mut c2 = c.clone();
        // Degrades to zero extraction, never a hard pass failure (RFC 0008).
        pass.run(&mut c2).await.unwrap();

        assert!(!c.diagnostics.lock().unwrap().has_errors());
        assert!(
            c.diagnostics.lock().unwrap().has_warnings(),
            "a parse failure must still be reported as a warning"
        );
        assert_eq!(stats.lock().unwrap().concepts, 0);
        assert!(read_output(&c, &pass_id).is_none());
    }

    #[tokio::test]
    async fn relationship_referencing_unknown_concept_is_skipped_with_diagnostic() {
        let (c, _dir) = ctx();
        seed_sections(
            &c,
            "spec.pdf",
            serde_json::json!([{ "index": 0, "page": 1, "text": "some prose" }]),
        )
        .await;

        let resp = serde_json::json!({
            "concepts": [{"name": "Data Replication", "description": "d"}],
            "relationships": [
                {"from": "Data Replication", "to": "Never Mentioned",
                 "kind": "relates_to", "description": "d"}
            ]
        })
        .to_string();
        let mut pass = DocumentSemanticsAnalyzerPass::new(
            LOCAL_DOCS_PASS_ID,
            Arc::new(MockLlmProvider::new(resp)),
        );
        let stats = pass.stats_handle();
        let pass_id = pass.name().to_string();
        let mut c2 = c.clone();
        pass.run(&mut c2).await.unwrap();

        assert!(!c.diagnostics.lock().unwrap().has_errors());
        assert!(c.diagnostics.lock().unwrap().has_warnings());
        assert_eq!(stats.lock().unwrap().relationships, 0);

        let graph = read_output(&c, &pass_id).unwrap();
        assert!(
            graph
                .relationships
                .iter()
                .all(|r| r.kind == RelationshipKind::References),
            "no Concept<->Concept edge may survive a dangling endpoint"
        );
    }

    #[tokio::test]
    async fn same_section_across_two_runs_produces_same_concept_ids() {
        let sections = serde_json::json!([{ "index": 0, "page": 1, "text": "stable prose" }]);

        let mut ids = Vec::new();
        for _ in 0..2 {
            let (c, _dir) = ctx();
            seed_sections(&c, "spec.pdf", sections.clone()).await;
            let mock = Arc::new(MockLlmProvider::new(two_concept_response()));
            let mut pass = DocumentSemanticsAnalyzerPass::new(LOCAL_DOCS_PASS_ID, mock);
            let pass_id = pass.name().to_string();
            let mut c2 = c.clone();
            pass.run(&mut c2).await.unwrap();

            let graph = read_output(&c, &pass_id).unwrap();
            let mut run_ids: Vec<KirId> = graph
                .objects
                .iter()
                .filter(|o| o.kind == ObjectKind::Custom("Concept".into()))
                .map(|o| o.id)
                .collect();
            run_ids.sort_by_key(|id| id.to_string());
            ids.push(run_ids);
        }

        assert_eq!(ids[0], ids[1], "concept ids must be run-independent");
        assert_eq!(ids[0].len(), 2);
    }

    #[tokio::test]
    async fn max_sections_caps_llm_calls() {
        let (c, _dir) = ctx();
        seed_sections(
            &c,
            "big.pdf",
            serde_json::json!([
                { "index": 0, "page": 1, "text": "one" },
                { "index": 1, "page": 2, "text": "two" },
                { "index": 2, "page": 3, "text": "three" },
            ]),
        )
        .await;

        let mock = Arc::new(MockLlmProvider::new(two_concept_response()));
        let mut pass =
            DocumentSemanticsAnalyzerPass::new(LOCAL_DOCS_PASS_ID, mock).with_max_sections(Some(1));
        let stats = pass.stats_handle();
        let mut c2 = c.clone();
        pass.run(&mut c2).await.unwrap();

        assert_eq!(stats.lock().unwrap().sections_processed, 1);
    }

    #[tokio::test]
    async fn ignores_artifacts_from_other_passes() {
        let (c, _dir) = ctx();
        seed_sections(
            &c,
            "spec.pdf",
            serde_json::json!([{ "index": 0, "page": 1, "text": "prose" }]),
        )
        .await;

        let mock = Arc::new(MockLlmProvider::new(two_concept_response()));
        let mut pass =
            DocumentSemanticsAnalyzerPass::new("local-docs-analyzer:some-other-workspace", mock);
        let stats = pass.stats_handle();
        let mut c2 = c.clone();
        pass.run(&mut c2).await.unwrap();

        assert_eq!(
            stats.lock().unwrap().sections_processed,
            0,
            "only the named LocalDocAnalyzerPass instance's output is input here"
        );
    }

    #[test]
    fn declares_its_local_docs_pass_as_a_dependency() {
        let pass = DocumentSemanticsAnalyzerPass::new(
            LOCAL_DOCS_PASS_ID,
            Arc::new(MockLlmProvider::new("{}")),
        );
        assert_eq!(pass.dependencies(), &[LOCAL_DOCS_PASS_ID]);
    }
}
