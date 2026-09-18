//! `LocalDocAnalyzerPass` — converts local-document observation artifacts
//! (RFC 0023, RFC 0024) into KIR. Produces:
//! - `KirObject(kind=Custom("Document"))` per PDF/DOCX file
//! - `KirObject(kind=Table)` per extracted table, plus a
//!   `KirRelationship(kind=Contains)` from the document to each table
//! - `KirObject(kind=Custom("Section"))` per page (PDF) or chunk (DOCX),
//!   plus a `Contains` edge from the document — RFC 0024's fix for deep
//!   content being unsearchable behind the document's single whole-file
//!   excerpt. `Custom("Section")`, not "segment": RFC 0016 already uses
//!   "segment" for an unrelated storage-layer concept.
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

/// Cap on the searchable `excerpt` property written per Section KirObject
/// (RFC 0024) — larger than a whole document's own excerpt budget because
/// the scope here is one page/chunk, not an entire book. Independently
/// declared from `ekos-plugin-localdocs`'s `SECTION_TEXT_MAX_CHARS` (the
/// artifact-storage bound): this crate doesn't depend on plugin crates,
/// same as it doesn't import the Document-level `EXCERPT_MAX_CHARS`
/// either — each layer re-truncates independently, defense in depth.
///
/// RFC 0144: raised from 1,200 to the observer's own 3,000-char storage bound. At 1,200 more than
/// half of every 2,500-char Markdown/text chunk never reached the search index — `excerpt` is the
/// only property `KirObject::indexed_content` reads for a Section.
const SECTION_EXCERPT_MAX_CHARS: usize = 3000;

#[derive(Debug, Deserialize)]
struct TableData {
    page: Option<u32>,
    rows: Vec<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct SectionData {
    index: usize,
    page: Option<u32>,
    text: String,
    /// RFC 0144 — Markdown only; absent from every older artifact and every other format.
    #[serde(default)]
    heading: Option<String>,
    #[serde(default)]
    heading_path: Vec<String>,
    #[serde(default)]
    line_start: Option<u32>,
    #[serde(default)]
    line_end: Option<u32>,
}

/// RFC 0144: coarse document classification from the path alone — lets retrieval and EKL tell an
/// RFC's prose apart from a devlog's or the project's own agent instructions.
fn doc_type_for(path: &str) -> &'static str {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    let file = lower.rsplit('/').next().unwrap_or(&lower);
    if file == "claude.md" || file == "agents.md" {
        "claude_md"
    } else if file.starts_with("readme") {
        "readme"
    } else if lower.contains("rfcs/") || file.starts_with("rfc") {
        "rfc"
    } else if lower.contains("devlogs/") || file.starts_with("devlog") {
        "devlog"
    } else {
        "doc"
    }
}

/// RFC 0144: `(rfc_number, rfc_title, rfc_status)` read from an RFC's own header — the first
/// `# RFC NNNN — Title` line, and either `**Status:** X` or `| **Status** | X |` (both forms exist
/// in this repo). `rfc_number` is zero-padded to 4 digits so it matches however prose writes it.
/// Status is cut at the first ` (` — `Accepted (per user direction…)` becomes `Accepted`.
#[derive(Debug, Default, PartialEq)]
struct RfcHeader {
    number: Option<String>,
    title: Option<String>,
    status: Option<String>,
}

fn parse_rfc_header(text: &str) -> RfcHeader {
    static TITLE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?m)^#\s+RFC[\s-]*(\d{1,4})\s*(?:[—–:-]\s*(.+?))?\s*$").unwrap()
    });
    static STATUS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?m)^(?:\*\*Status:\*\*\s*(.+?)|\|\s*\*\*Status\*\*\s*\|\s*(.+?)\s*\|)\s*$",
        )
        .unwrap()
    });
    let mut header = RfcHeader::default();
    if let Some(c) = TITLE.captures(text) {
        header.number = c[1].parse::<u32>().ok().map(|n| format!("{n:04}"));
        header.title = c.get(2).map(|m| m.as_str().trim().to_string());
    }
    if let Some(c) = STATUS.captures(text) {
        let raw = c
            .get(1)
            .or_else(|| c.get(2))
            .map(|m| m.as_str())
            .unwrap_or("");
        let status = raw
            .split(" (")
            .next()
            .unwrap_or(raw)
            .trim()
            .trim_matches('*')
            .trim();
        if !status.is_empty() {
            header.status = Some(status.to_string());
        }
    }
    header
}

#[derive(Debug, Deserialize)]
struct DocumentData {
    path: String,
    /// RFC 0079: present only in a multi-`[observe] paths` workspace — `build.rs`'s own choke
    /// point, not this analyzer's connector. Qualifies id hashing only; `path` above stays the
    /// bare, human-readable path everywhere it's displayed (object name, evidence text).
    #[serde(default)]
    project: Option<String>,
    doc_format: String,
    #[serde(default)]
    page_count: Option<u32>,
    #[serde(default)]
    excerpt: String,
    #[serde(default)]
    tables: Vec<TableData>,
    #[serde(default)]
    sections: Vec<SectionData>,
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

/// Deterministic id for a section object, scoped to its document + index
/// within that document's sections (RFC 0024).
fn section_kir_id(path: &str, index: usize) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("localdocs:{path}:section:{index}").as_bytes(),
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

            // RFC 0079: id hashing only — `data.path` (display name, evidence text, properties)
            // stays the bare, human-readable path everywhere below.
            let id_path =
                ekos_common::project::project_qualify(&data.path, data.project.as_deref());
            let doc_id = document_kir_id(&id_path);
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
            // RFC 0144: document classification + RFC header, shared onto every Section below.
            let doc_type = doc_type_for(&data.path);
            let rfc = if doc_type == "rfc" {
                let head: String = data
                    .sections
                    .iter()
                    .take(2)
                    .map(|s| s.text.as_str())
                    .chain(std::iter::once(data.excerpt.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n");
                parse_rfc_header(&head)
            } else {
                RfcHeader::default()
            };
            let mut doc_attrs: Vec<(&str, serde_json::Value)> =
                vec![("doc_type", serde_json::json!(doc_type))];
            if let Some(n) = &rfc.number {
                doc_attrs.push(("rfc_number", serde_json::json!(n)));
            }
            if let Some(t) = &rfc.title {
                doc_attrs.push(("rfc_title", serde_json::json!(t)));
            }
            if let Some(st) = &rfc.status {
                doc_attrs.push(("rfc_status", serde_json::json!(st)));
            }
            for (k, v) in &doc_attrs {
                obj.properties.insert((*k).into(), v.clone());
            }
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
                let tbl_id = table_kir_id(&id_path, index);
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

                let mut rel =
                    KirRelationship::deterministic(RelationshipKind::Contains, doc_id, tbl_id, "");
                rel.evidence.push(tbl_ev_id);
                graph.relationships.push(rel);
            }

            let mut previous_path: Option<&[String]> = None;
            let mut part = 0usize;
            for section in &data.sections {
                let sec_id = section_kir_id(&id_path, section.index);
                // RFC 0144: consecutive sub-chunks of one heading share its path; number them.
                if !section.heading_path.is_empty()
                    && previous_path == Some(section.heading_path.as_slice())
                {
                    part += 1;
                } else {
                    part = 1;
                }
                previous_path = Some(section.heading_path.as_slice());
                let name = match section.page {
                    Some(p) => format!("{}: page {p}", data.path),
                    None if !section.heading_path.is_empty() => {
                        let path = section.heading_path.join(" › ");
                        if part > 1 {
                            format!("{} § {path} (part {part})", data.path)
                        } else {
                            format!("{} § {path}", data.path)
                        }
                    }
                    None => format!("{}: section {}", data.path, section.index + 1),
                };
                let mut sec_obj = KirObject::new(name, ObjectKind::Custom("Section".to_string()));
                sec_obj.id = sec_id;
                let excerpt: String = section
                    .text
                    .chars()
                    .take(SECTION_EXCERPT_MAX_CHARS)
                    .collect();
                sec_obj
                    .properties
                    .insert("excerpt".into(), serde_json::json!(excerpt));
                sec_obj
                    .properties
                    .insert("page".into(), serde_json::json!(section.page));
                sec_obj
                    .properties
                    .insert("section_index".into(), serde_json::json!(section.index));
                for (k, v) in &doc_attrs {
                    sec_obj.properties.insert((*k).into(), v.clone());
                }
                if let Some(heading) = &section.heading {
                    sec_obj
                        .properties
                        .insert("heading".into(), serde_json::json!(heading));
                    sec_obj.properties.insert(
                        "heading_path".into(),
                        serde_json::json!(section.heading_path),
                    );
                }
                let line_range = section.line_start.zip(section.line_end);
                if let Some((start, end)) = line_range {
                    sec_obj
                        .properties
                        .insert("line_start".into(), serde_json::json!(start));
                    sec_obj
                        .properties
                        .insert("line_end".into(), serde_json::json!(end));
                }

                let page_note = section
                    .page
                    .map(|p| format!(" (page {p})"))
                    .unwrap_or_default();
                // RFC 0144: a Markdown section's evidence points at its real line range.
                let sec_ev = match line_range {
                    Some((start, end)) => KirEvidence::new(
                        SourceLocation::at(data.path.clone(), start),
                        format!(
                            "section {} extracted from {}:{start}-{end}",
                            section.index + 1,
                            data.path
                        ),
                    ),
                    None => KirEvidence::new(
                        SourceLocation::file(data.path.clone()),
                        format!(
                            "section {}{page_note} extracted from {}",
                            section.index + 1,
                            data.path
                        ),
                    ),
                };
                let sec_ev_id = graph.add_evidence(sec_ev);
                sec_obj.evidence.push(sec_ev_id);
                graph.objects.push(sec_obj);

                let mut rel =
                    KirRelationship::deterministic(RelationshipKind::Contains, doc_id, sec_id, "");
                rel.evidence.push(sec_ev_id);
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
        seed_doc_with_sections(
            ctx,
            path,
            doc_format,
            excerpt,
            tables,
            serde_json::json!([]),
        )
    }

    fn seed_doc_with_sections(
        ctx: &PassContext,
        path: &str,
        doc_format: &str,
        excerpt: &str,
        tables: serde_json::Value,
        sections: serde_json::Value,
    ) -> ArtifactId {
        let data = serde_json::json!({
            "path": path,
            "doc_format": doc_format,
            "page_count": 3,
            "excerpt": excerpt,
            "tables": tables,
            "sections": sections,
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

    #[tokio::test]
    async fn a_project_field_qualifies_the_document_id_but_not_its_displayed_path() {
        // RFC 0079: real multi-project-workspace shape — `build.rs` writes `data.project` when
        // `[observe] paths` has more than one entry; this analyzer must fold it into the id hash
        // while leaving the human-readable path (name, properties) untouched.
        let (c, _dir) = ctx();
        let data = serde_json::json!({
            "path": "notes.md",
            "project": "service-a",
            "doc_format": "md",
            "page_count": null,
            "excerpt": "hello",
            "tables": [],
            "sections": [],
            "ocr_text": null,
        });
        let artifact = ekos_artifact::ObservationArtifact::new("localdocs", "notes.md", data);
        let id = artifact.id.clone();
        let json = serde_json::to_value(&artifact).unwrap();
        c.artifact_store.write(&artifact.id, &json).unwrap();

        let graph = run_pass(vec![id], c).await;
        assert_eq!(graph.objects.len(), 1);
        assert_eq!(
            graph.objects[0].id,
            document_kir_id("service-a:notes.md"),
            "id must be qualified by the project field"
        );
        assert_eq!(
            graph.objects[0].name, "notes.md",
            "the displayed path must stay bare, unqualified"
        );
        assert_ne!(
            graph.objects[0].id,
            document_kir_id("notes.md"),
            "must not collide with the same path from an unqualified/different project"
        );
    }

    /// Real table content extracted by `PdfParser` from a public MLOps
    /// white paper's table of contents, verified via an end-to-end run
    /// against a real document library (RFC 0023's devlog) — confirms
    /// genuine book table content survives into a `Table` KirObject
    /// unmodified, not just synthetic two-cell fixtures.
    #[tokio::test]
    async fn real_book_table_content_produces_matching_table_object() {
        let (c, _dir) = ctx();
        let tables = serde_json::json!([
            { "page": null, "rows": [
                ["Putting it all together", "34"],
                ["Additional resources", "36"]
            ] }
        ]);
        let id = seed_doc(
            &c,
            "Practitioner's Guide to MLOps.pdf",
            "pdf",
            "Practitioners guide to MLOps: a framework for continuous delivery and automation of machine learning.",
            tables,
        );
        let graph = run_pass(vec![id], c).await;

        let tbl_id = table_kir_id("Practitioner's Guide to MLOps.pdf", 0);
        let tbl_obj = graph.objects.iter().find(|o| o.id == tbl_id).unwrap();
        assert_eq!(
            tbl_obj.properties["rows"],
            serde_json::json!([
                ["Putting it all together", "34"],
                ["Additional resources", "36"]
            ])
        );
    }

    #[tokio::test]
    async fn section_produces_child_object_and_contains_edge() {
        let (c, _dir) = ctx();
        let sections = serde_json::json!([
            { "index": 0, "page": 5, "text": "page five prose about replication" }
        ]);
        let id = seed_doc_with_sections(
            &c,
            "Cloud Design Patterns.pdf",
            "pdf",
            "cover page",
            serde_json::json!([]),
            sections,
        );
        let graph = run_pass(vec![id], c).await;

        assert_eq!(graph.objects.len(), 2);
        let doc_id = document_kir_id("Cloud Design Patterns.pdf");
        let sec_id = section_kir_id("Cloud Design Patterns.pdf", 0);
        let sec_obj = graph.objects.iter().find(|o| o.id == sec_id).unwrap();
        assert_eq!(sec_obj.kind, ObjectKind::Custom("Section".into()));
        assert_eq!(sec_obj.name, "Cloud Design Patterns.pdf: page 5");
        assert!(
            graph
                .relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Contains
                    && r.from == doc_id
                    && r.to == sec_id)
        );
    }

    /// The direct regression test for RFC 0024's bug: a term buried past
    /// the whole-document excerpt's 600-char budget must be findable once
    /// it rides on its own Section object's `excerpt` property, since
    /// `indexed_content()` reads `properties["excerpt"]` on any object.
    #[tokio::test]
    async fn section_excerpt_is_searchable_via_indexed_content() {
        let (c, _dir) = ctx();
        let sections = serde_json::json!([
            { "index": 0, "page": 1, "text": "cover page, no relevant content here" },
            { "index": 1, "page": 213, "text": "Data Replication and Synchronization Guidance: this section covers replication patterns in depth." }
        ]);
        let id = seed_doc_with_sections(
            &c,
            "Cloud Design Patterns.pdf",
            "pdf",
            "CLOUD DESIGN PATTERNS cover page, authors, and publisher information",
            serde_json::json!([]),
            sections,
        );
        let graph = run_pass(vec![id], c).await;

        let sec_id = section_kir_id("Cloud Design Patterns.pdf", 1);
        let sec_obj = graph.objects.iter().find(|o| o.id == sec_id).unwrap();
        assert!(
            sec_obj
                .indexed_content()
                .to_lowercase()
                .contains("replication")
        );

        // And the document object's own excerpt does NOT contain it —
        // proving the fix is real, not incidental.
        let doc_id = document_kir_id("Cloud Design Patterns.pdf");
        let doc_obj = graph.objects.iter().find(|o| o.id == doc_id).unwrap();
        assert!(
            !doc_obj
                .indexed_content()
                .to_lowercase()
                .contains("replication")
        );
    }

    // ── RFC 0144: section attributes ────────────────────────────────────

    #[test]
    fn doc_type_is_derived_from_the_path() {
        assert_eq!(doc_type_for("CLAUDE.md"), "claude_md");
        assert_eq!(doc_type_for("ekos/README.md"), "readme");
        assert_eq!(doc_type_for("docs/rfcs/0001-compiler-core.md"), "rfc");
        assert_eq!(doc_type_for("devlogs/devlog_183.md"), "devlog");
        assert_eq!(doc_type_for("notes/retention.md"), "doc");
    }

    #[test]
    fn rfc_header_parses_both_status_forms() {
        let bold =
            "# RFC 0143 — MCP over Streamable HTTP\n\n**Status:** Accepted (per user direction)\n";
        assert_eq!(
            parse_rfc_header(bold),
            RfcHeader {
                number: Some("0143".into()),
                title: Some("MCP over Streamable HTTP".into()),
                status: Some("Accepted".into()),
            }
        );
        let table = "# RFC 0001 — Compiler Core Architecture\n\n| Field | Value |\n|-------|-------|\n| **Status** | Accepted |\n";
        let h = parse_rfc_header(table);
        assert_eq!(h.number.as_deref(), Some("0001"));
        assert_eq!(h.status.as_deref(), Some("Accepted"));
        assert_eq!(parse_rfc_header("# Just a doc\n"), RfcHeader::default());
    }

    #[tokio::test]
    async fn markdown_sections_get_heading_names_attributes_and_line_evidence() {
        let (c, _dir) = ctx();
        let long_motivation = format!(
            "## Motivation\n{}",
            "Define abstractions before domain logic to avoid API churn. ".repeat(40)
        );
        let sections = serde_json::json!([
            { "index": 0, "page": null, "text": "# RFC 0001 — Compiler Core\n\n**Status:** Accepted",
              "heading": "RFC 0001 — Compiler Core", "heading_path": ["RFC 0001 — Compiler Core"],
              "line_start": 1, "line_end": 3 },
            { "index": 1, "page": null, "text": long_motivation,
              "heading": "Motivation", "heading_path": ["RFC 0001 — Compiler Core", "Motivation"],
              "line_start": 4, "line_end": 9 },
            { "index": 2, "page": null, "text": "more motivation",
              "heading": "Motivation", "heading_path": ["RFC 0001 — Compiler Core", "Motivation"],
              "line_start": 10, "line_end": 12 }
        ]);
        let path = "docs/rfcs/0001-compiler-core.md";
        let id = seed_doc_with_sections(
            &c,
            path,
            "md",
            "# RFC 0001",
            serde_json::json!([]),
            sections,
        );
        let graph = run_pass(vec![id], c).await;

        let doc = graph
            .objects
            .iter()
            .find(|o| o.id == document_kir_id(path))
            .unwrap();
        assert_eq!(doc.properties["doc_type"], "rfc");
        assert_eq!(doc.properties["rfc_number"], "0001");
        assert_eq!(doc.properties["rfc_status"], "Accepted");

        let motivation = graph
            .objects
            .iter()
            .find(|o| o.id == section_kir_id(path, 1))
            .unwrap();
        assert_eq!(
            motivation.name,
            "docs/rfcs/0001-compiler-core.md § RFC 0001 — Compiler Core › Motivation"
        );
        assert_eq!(motivation.properties["heading"], "Motivation");
        assert_eq!(motivation.properties["rfc_number"], "0001");
        assert_eq!(motivation.properties["line_start"], 4);
        // Text past the old 1,200-char cap is now indexed.
        assert!(
            motivation.properties["excerpt"]
                .as_str()
                .unwrap()
                .chars()
                .count()
                > 1200
        );

        let part2 = graph
            .objects
            .iter()
            .find(|o| o.id == section_kir_id(path, 2))
            .unwrap();
        assert!(
            part2.name.ends_with("› Motivation (part 2)"),
            "{}",
            part2.name
        );

        let ev = graph
            .evidence
            .iter()
            .find(|e| e.id == motivation.evidence[0])
            .unwrap();
        assert_eq!(ev.location.line, Some(4));
        assert!(
            ev.fragment.contains("docs/rfcs/0001-compiler-core.md:4-9"),
            "{}",
            ev.fragment
        );
    }

    #[tokio::test]
    async fn pre_rfc_0144_section_artifacts_still_parse_with_old_names() {
        let (c, _dir) = ctx();
        let sections = serde_json::json!([{ "index": 0, "page": null, "text": "old chunk" }]);
        let id =
            seed_doc_with_sections(&c, "notes.md", "md", "old", serde_json::json!([]), sections);
        let graph = run_pass(vec![id], c).await;
        let sec = graph
            .objects
            .iter()
            .find(|o| o.id == section_kir_id("notes.md", 0))
            .unwrap();
        assert_eq!(sec.name, "notes.md: section 1");
        assert!(!sec.properties.contains_key("heading"));
        assert_eq!(sec.properties["doc_type"], "doc");
    }

    #[tokio::test]
    async fn zero_sections_produce_zero_section_objects() {
        let (c, _dir) = ctx();
        let id = seed_doc(
            &c,
            "notes.pdf",
            "pdf",
            "no sections here",
            serde_json::json!([]),
        );
        let graph = run_pass(vec![id], c).await;
        assert_eq!(graph.objects.len(), 1);
        assert!(graph.relationships.is_empty());
    }

    #[tokio::test]
    async fn same_document_across_two_runs_gets_same_section_id() {
        let sections = serde_json::json!([{ "index": 0, "page": 1, "text": "stable text" }]);

        let (c1, _dir1) = ctx();
        let id1 = seed_doc_with_sections(
            &c1,
            "spec.pdf",
            "pdf",
            "hello",
            serde_json::json!([]),
            sections.clone(),
        );
        let graph1 = run_pass(vec![id1], c1).await;

        let (c2, _dir2) = ctx();
        let id2 = seed_doc_with_sections(
            &c2,
            "spec.pdf",
            "pdf",
            "hello",
            serde_json::json!([]),
            sections,
        );
        let graph2 = run_pass(vec![id2], c2).await;

        let sec_id1 = graph1
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Custom("Section".into()))
            .unwrap()
            .id;
        let sec_id2 = graph2
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Custom("Section".into()))
            .unwrap()
            .id;
        assert_eq!(sec_id1, sec_id2);
    }

    /// RFC 0025's central claim, tested directly: `doc_format` is opaque to
    /// this pass, so the formats it added (`txt`/`md`/`html`/`htm`/`eml`)
    /// produce exactly the same Document+Section shape a `pdf` artifact
    /// does, with zero code changes here. Asserted against the `pdf` case
    /// rather than restated, so a future format-specific branch introduced
    /// upstream breaks this test.
    #[tokio::test]
    async fn new_document_formats_produce_the_same_kir_shape_as_pdf() {
        let sections = serde_json::json!([
            { "index": 0, "page": null, "text": "first chunk of prose" },
            { "index": 1, "page": null, "text": "second chunk of prose" }
        ]);

        let (c_pdf, _d1) = ctx();
        let pdf_id = seed_doc_with_sections(
            &c_pdf,
            "doc",
            "pdf",
            "excerpt",
            serde_json::json!([]),
            sections.clone(),
        );
        let pdf_graph = run_pass(vec![pdf_id], c_pdf).await;

        for format in ["txt", "md", "html", "htm", "eml"] {
            let (c, _d) = ctx();
            let id = seed_doc_with_sections(
                &c,
                "doc",
                format,
                "excerpt",
                serde_json::json!([]),
                sections.clone(),
            );
            let graph = run_pass(vec![id], c).await;

            assert_eq!(
                graph.objects.len(),
                pdf_graph.objects.len(),
                ".{format} must produce the same object count as .pdf"
            );
            assert_eq!(
                graph.relationships.len(),
                pdf_graph.relationships.len(),
                ".{format} must produce the same edge count as .pdf"
            );

            // Same ids and kinds — the only intended difference is the
            // `doc_format` property itself.
            let doc_id = document_kir_id("doc");
            let doc_obj = graph.objects.iter().find(|o| o.id == doc_id).unwrap();
            assert_eq!(doc_obj.kind, ObjectKind::Custom("Document".into()));
            assert_eq!(doc_obj.properties["doc_format"], format);

            for index in 0..2 {
                let sec_id = section_kir_id("doc", index);
                let sec_obj = graph.objects.iter().find(|o| o.id == sec_id).unwrap();
                assert_eq!(sec_obj.kind, ObjectKind::Custom("Section".into()));
                assert!(!sec_obj.evidence.is_empty());
                assert!(
                    graph
                        .relationships
                        .iter()
                        .any(|r| r.kind == RelationshipKind::Contains
                            && r.from == doc_id
                            && r.to == sec_id)
                );
            }
        }
    }

    /// RFC 0024's search-depth regression, re-proven for RFC 0025's
    /// page-less formats: prose past the Document excerpt's 600-char budget
    /// is findable because it rides on its own Section object. The
    /// page-less naming path (`section N`, not `page N`) is exercised here
    /// too — every new format has `page: None`.
    #[tokio::test]
    async fn markdown_content_past_char_600_is_searchable_via_indexed_content() {
        let filler = "Boilerplate front matter that fills the document excerpt budget. ".repeat(12); // > 600 chars on its own
        assert!(filler.chars().count() > 600);
        let deep_text = "## Exceptions\n\nAn exception requires a written justification recorded against \
             the table's entry in the catalogue.";

        let (c, _dir) = ctx();
        let id = seed_doc_with_sections(
            &c,
            "notes.md",
            "md",
            // What the observer would have captured: only the first 600 chars.
            &filler.chars().take(600).collect::<String>(),
            serde_json::json!([]),
            serde_json::json!([
                { "index": 0, "page": null, "text": filler },
                { "index": 1, "page": null, "text": deep_text }
            ]),
        );
        let graph = run_pass(vec![id], c).await;

        let sec_obj = graph
            .objects
            .iter()
            .find(|o| o.id == section_kir_id("notes.md", 1))
            .unwrap();
        assert_eq!(sec_obj.name, "notes.md: section 2");
        assert!(
            sec_obj
                .indexed_content()
                .to_lowercase()
                .contains("written justification")
        );

        // The document object alone would not have found it — the same
        // demonstrated-bug shape RFC 0024 fixed for PDF pages.
        let doc_obj = graph
            .objects
            .iter()
            .find(|o| o.id == document_kir_id("notes.md"))
            .unwrap();
        assert!(
            !doc_obj
                .indexed_content()
                .to_lowercase()
                .contains("written justification")
        );
    }
}
