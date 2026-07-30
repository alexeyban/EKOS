//! `LocalDocAnalyzerPass` — converts local-document observation artifacts
//! (RFC 0023) into KIR. Produces:
//! - `KirObject(kind=Custom("Document"))` per PDF/DOCX file
//! - `KirObject(kind=Table)` per extracted table, plus a
//!   `KirRelationship(kind=Contains)` from the document to each table
//!
//! Pure structural mapping — no LLM in the loop, same shape as
//! `ConfluenceAnalyzerPass`/`GitHubAnalyzerPass`.

use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct TableData {
    page: Option<u32>,
    rows: Vec<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct DocumentData {
    path: String,
    doc_format: String,
    #[serde(default)]
    page_count: Option<u32>,
    #[serde(default)]
    excerpt: String,
    #[serde(default)]
    tables: Vec<TableData>,
    #[serde(default)]
    ocr_text: Option<String>,
    artifact_id: Option<String>,
}

/// Deterministic id for a document object — stable across passes and
/// `ekos recover` runs.
fn document_kir_id(path: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("localdocs:{path}").as_bytes(),
    ))
}

/// Deterministic id for a table object, scoped to its document + index
/// within that document's extracted tables.
fn table_kir_id(path: &str, index: usize) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("localdocs:{path}:table:{index}").as_bytes(),
    ))
}

pub struct LocalDocAnalyzerPass {
    pass_id: String,
    /// Local-document ObservationArtifact IDs to process.
    doc_artifact_ids: Vec<ArtifactId>,
}

impl LocalDocAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, doc_artifact_ids: Vec<ArtifactId>) -> Self {
        Self {
            pass_id: format!("local-docs-analyzer:{}", workspace_name.into()),
            doc_artifact_ids,
        }
    }
}

#[async_trait]
impl CompilerPass for LocalDocAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .doc_artifact_ids
            .iter()
            .map(|id| id.to_string())
            .collect();
        ids.sort();
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut graph = KirGraph::new();

        for artifact_id in &self.doc_artifact_ids {
            let json = match ctx.artifact_store.read(artifact_id) {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "LOCALDOCS001",
                        format!("cannot read artifact {artifact_id}: {e}"),
                    );
                    continue;
                }
            };
            let data: DocumentData = match serde_json::from_value(json["data"].clone()) {
                Ok(d) => d,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "LOCALDOCS002",
                        format!("malformed local-document payload in {artifact_id}: {e}"),
                    );
                    continue;
                }
            };

            let doc_id = document_kir_id(&data.path);
            let mut obj = KirObject::new(
                data.path.clone(),
                ObjectKind::Custom("Document".to_string()),
            );
            obj.id = doc_id;
            obj.properties
                .insert("path".into(), serde_json::json!(data.path));
            obj.properties
                .insert("doc_format".into(), serde_json::json!(data.doc_format));
            obj.properties
                .insert("page_count".into(), serde_json::json!(data.page_count));
            obj.properties
                .insert("excerpt".into(), serde_json::json!(data.excerpt));
            if let Some(artifact_id_str) = &data.artifact_id {
                obj.properties
                    .insert("artifact_id".into(), serde_json::json!(artifact_id_str));
            }
            if let Some(ocr_text) = &data.ocr_text
                && !ocr_text.is_empty()
            {
                obj.properties
                    .insert("ocr_text".into(), serde_json::json!(ocr_text));
            }

            let doc_ev = KirEvidence::new(
                SourceLocation::file(data.path.clone()),
                format!("local document: {} ({})", data.path, data.doc_format),
            );
            let doc_ev_id = graph.add_evidence(doc_ev);
            obj.evidence.push(doc_ev_id);
            graph.objects.push(obj);

            for (index, table) in data.tables.iter().enumerate() {
                let tbl_id = table_kir_id(&data.path, index);
                let mut tbl_obj = KirObject::new(
                    format!("{}: table {}", data.path, index + 1),
                    ObjectKind::Table,
                );
                tbl_obj.id = tbl_id;
                tbl_obj
                    .properties
                    .insert("rows".into(), serde_json::json!(table.rows));
                tbl_obj
                    .properties
                    .insert("page".into(), serde_json::json!(table.page));

                let page_note = table
                    .page
                    .map(|p| format!(" (page {p})"))
                    .unwrap_or_default();
                let tbl_ev = KirEvidence::new(
                    SourceLocation::file(data.path.clone()),
                    format!(
                        "table {}{page_note} extracted from {}",
                        index + 1,
                        data.path
                    ),
                );
                let tbl_ev_id = graph.add_evidence(tbl_ev);
                tbl_obj.evidence.push(tbl_ev_id);
                graph.objects.push(tbl_obj);

                let mut rel = KirRelationship::new(RelationshipKind::Contains, doc_id, tbl_id);
                rel.evidence.push(tbl_ev_id);
                graph.relationships.push(rel);
            }
        }

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
            objects = knowledge.content.kir.objects.len(),
            edges = knowledge.content.kir.relationships.len(),
            "local-docs-analyzer complete"
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

    fn seed_doc(
        ctx: &PassContext,
        path: &str,
        doc_format: &str,
        excerpt: &str,
        tables: serde_json::Value,
    ) -> ArtifactId {
        let data = serde_json::json!({
            "path": path,
            "doc_format": doc_format,
            "page_count": 3,
            "excerpt": excerpt,
            "tables": tables,
            "ocr_text": null,
            "image_count": 0,
            "ocr_image_count": 0,
        });
        let artifact = ekos_artifact::ObservationArtifact::new("localdocs", path, data);
        let json = serde_json::to_value(&artifact).unwrap();
        ctx.artifact_store.write(&artifact.id, &json).unwrap();
        artifact.id
    }

    async fn run_pass(ids: Vec<ArtifactId>, ctx: PassContext) -> ekos_kir::KirGraph {
        let mut pass = LocalDocAnalyzerPass::new("test", ids);
        let mut ctx = ctx;
        pass.run(&mut ctx).await.unwrap();

        let knowledge_id = ctx
            .artifact_store
            .list()
            .unwrap()
            .into_iter()
            .find(|id| {
                let json = ctx.artifact_store.read(id).unwrap().unwrap();
                json.get("kir").is_some()
            })
            .expect("pass must have written a KnowledgeArtifact");
        let json = ctx.artifact_store.read(&knowledge_id).unwrap().unwrap();
        let knowledge: ekos_artifact::KnowledgeArtifact = serde_json::from_value(json).unwrap();
        knowledge.content.kir
    }

    #[tokio::test]
    async fn one_document_object_per_artifact() {
        let (c, _dir) = ctx();
        let id = seed_doc(&c, "spec.pdf", "pdf", "hello world", serde_json::json!([]));
        let graph = run_pass(vec![id], c).await;
        assert_eq!(graph.objects.len(), 1);
        assert_eq!(graph.objects[0].kind, ObjectKind::Custom("Document".into()));
        assert_eq!(graph.objects[0].properties["excerpt"], "hello world");
    }

    #[tokio::test]
    async fn table_produces_child_object_and_contains_edge() {
        let (c, _dir) = ctx();
        let tables = serde_json::json!([
            { "page": 1, "rows": [["Name", "Value"], ["a", "1"]] }
        ]);
        let id = seed_doc(&c, "report.docx", "docx", "a report", tables);
        let graph = run_pass(vec![id], c).await;

        assert_eq!(graph.objects.len(), 2);
        let doc_id = document_kir_id("report.docx");
        let tbl_id = table_kir_id("report.docx", 0);
        let tbl_obj = graph.objects.iter().find(|o| o.id == tbl_id).unwrap();
        assert_eq!(tbl_obj.kind, ObjectKind::Table);
        assert!(
            graph
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Contains
                    && r.from == doc_id
                    && r.to == tbl_id)
        );
    }

    #[tokio::test]
    async fn zero_tables_produce_zero_table_objects() {
        let (c, _dir) = ctx();
        let id = seed_doc(
            &c,
            "notes.pdf",
            "pdf",
            "no tables here",
            serde_json::json!([]),
        );
        let graph = run_pass(vec![id], c).await;
        assert_eq!(graph.objects.len(), 1);
        assert!(graph.relationships.is_empty());
    }

    #[tokio::test]
    async fn same_document_across_two_runs_gets_same_object_id() {
        let (c1, _dir1) = ctx();
        let id1 = seed_doc(&c1, "spec.pdf", "pdf", "hello", serde_json::json!([]));
        let graph1 = run_pass(vec![id1], c1).await;

        let (c2, _dir2) = ctx();
        let id2 = seed_doc(&c2, "spec.pdf", "pdf", "hello", serde_json::json!([]));
        let graph2 = run_pass(vec![id2], c2).await;

        assert_eq!(graph1.objects[0].id, graph2.objects[0].id);
    }
}
