//! `DbtAnalyzerPass` — structural extraction of `Table` KIR objects from a dbt project's own
//! checked-in metadata (RFC 0117): model `.sql` files (Jinja `ref()`/`source()` macro calls) and
//! `schema.yml`/`sources.yml`-shaped YAML config. Deliberately never reads `manifest.json`/
//! `catalog.json` — both live under `dbt/target/`, a build artifact directory confirmed gitignored
//! on a real inspected project, not checked-in source of truth — and never connects to a live
//! warehouse: dbt itself can point at any database, so the only stable, version-controlled source
//! of truth is dbt's own project files.
//!
//! A `.sql` file under `models/**/` *is* a model regardless of whether any YAML documents it — YAML
//! only adds description/columns on top of a model that already exists. Source tables (`sources:`
//! YAML) have no backing `.sql` file at all — they're pre-existing tables dbt only references, so
//! their only existence signal is the YAML.
//!
//! Uses `ObjectKind::Table`, not a new `Custom(_)` kind, deliberately (RFC 0117): a dbt model is a
//! real table, and letting `DefaultResolver`'s real column-Jaccard structural scoring fuse it with
//! an independently-discovered DDL-based `Table` of the same name is desired, not the over-merge
//! risk `Custom(_)`'s blanket kind-exclusion list (`ekos_identity`) exists to prevent. Mirrors
//! `python_analyzer.rs`'s SQLAlchemy-ORM-to-`Table` precedent (RFC 0091): a distinct id namespace
//! (`"dbt-table:"`) keeps this analyzer's ids from ever colliding with a same-named DDL table's id,
//! while both stay mergeable by real identity resolution.
//!
//! Dependency edges use the built-in `RelationshipKind::DependsOn`, not the Transformation IR's
//! `Custom("FeedsInto")` — this is whole-table-to-whole-table dependency (the same relationship
//! kind `concentration_risks`, RFC 0094, already scans the whole graph for), not step-level lineage
//! within one transformation.

use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

fn dbt_table_kir_id(dbt_root: &str, name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("dbt-table:{dbt_root}:{}", name.to_lowercase()).as_bytes(),
    ))
}

/// `(from, to)` alone is a safe id input here, unlike `sql_analyzer.rs`'s FK ids — a dbt model
/// either depends on a target table or it doesn't; repeated `ref()`/`source()` calls to the same
/// target within one model (common — the same upstream table referenced from more than one CTE)
/// are one real dependency fact, not several, so they're deduplicated before this is ever called.
fn dbt_depends_on_kir_id(from: KirId, to: KirId) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("dbt-depends-on:{from}:{to}").as_bytes(),
    ))
}

fn model_name_from_path(rel_path: &str) -> String {
    std::path::Path::new(rel_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| rel_path.to_string())
}

fn line_at(content: &str, byte_offset: usize) -> u32 {
    content[..byte_offset.min(content.len())]
        .matches('\n')
        .count() as u32
        + 1
}

/// Best-effort `{{ config(materialized='...') }}` extraction — omitted, never guessed, when absent.
fn extract_materialized(sql: &str) -> Option<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re =
        RE.get_or_init(|| regex::Regex::new(r#"materialized\s*=\s*['"]([a-zA-Z_]+)['"]"#).unwrap());
    re.captures(sql).map(|c| c[1].to_string())
}

/// One resolved `ref('model')` or `source('src', 'table')` macro call found in a model's raw SQL.
struct MacroRef {
    /// The name to resolve against `known` — the model name for `ref()`, the table name for
    /// `source()` (dbt addresses source tables by table name, not `source.table`).
    target_name: String,
    byte_offset: usize,
    fragment: String,
}

fn find_macro_refs(sql: &str) -> Vec<MacroRef> {
    static REF_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static SOURCE_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let ref_re = REF_RE
        .get_or_init(|| regex::Regex::new(r#"ref\(\s*['"]([A-Za-z0-9_]+)['"]\s*\)"#).unwrap());
    let source_re = SOURCE_RE.get_or_init(|| {
        regex::Regex::new(
            r#"source\(\s*['"]([A-Za-z0-9_\-]+)['"]\s*,\s*['"]([A-Za-z0-9_\-]+)['"]\s*\)"#,
        )
        .unwrap()
    });

    let mut refs = Vec::new();
    for m in ref_re.find_iter(sql) {
        let caps = ref_re.captures(m.as_str()).unwrap();
        refs.push(MacroRef {
            target_name: caps[1].to_string(),
            byte_offset: m.start(),
            fragment: m.as_str().to_string(),
        });
    }
    for m in source_re.find_iter(sql) {
        let caps = source_re.captures(m.as_str()).unwrap();
        refs.push(MacroRef {
            target_name: caps[2].to_string(),
            byte_offset: m.start(),
            fragment: m.as_str().to_string(),
        });
    }
    refs
}

/// One `models[].columns[]` or `sources[].tables[]` entry's declared (best-effort, partial —
/// dbt's own `schema.yml` typically only documents tested/described columns, not every column a
/// model actually produces) metadata.
#[derive(Default)]
struct YamlModelDoc {
    columns: Vec<serde_json::Value>,
    description: Option<String>,
}

struct YamlSourceTable {
    name: String,
    source_name: String,
    description: Option<String>,
}

fn parse_yaml_doc(
    doc: &serde_yaml::Value,
) -> (HashMap<String, YamlModelDoc>, Vec<YamlSourceTable>) {
    let mut models = HashMap::new();
    if let Some(seq) = doc.get("models").and_then(|v| v.as_sequence()) {
        for entry in seq {
            let Some(name) = entry.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let columns = entry
                .get("columns")
                .and_then(|v| v.as_sequence())
                .map(|cols| {
                    cols.iter()
                        .filter_map(|c| c.get("name").and_then(|n| n.as_str()))
                        .map(|n| serde_json::json!({ "name": n }))
                        .collect()
                })
                .unwrap_or_default();
            let description = entry
                .get("description")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            models.insert(
                name.to_string(),
                YamlModelDoc {
                    columns,
                    description,
                },
            );
        }
    }

    let mut sources = Vec::new();
    if let Some(seq) = doc.get("sources").and_then(|v| v.as_sequence()) {
        for src in seq {
            let Some(source_name) = src.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(tables) = src.get("tables").and_then(|v| v.as_sequence()) else {
                continue;
            };
            for table in tables {
                let Some(name) = table.get("name").and_then(|v| v.as_str()) else {
                    continue;
                };
                let description = table
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                sources.push(YamlSourceTable {
                    name: name.to_string(),
                    source_name: source_name.to_string(),
                    description,
                });
            }
        }
    }

    (models, sources)
}

pub struct DbtAnalyzerPass {
    pass_id: String,
    /// RFC 0079-qualified identifier for this dbt project (its `dbt_project.yml`'s parent
    /// directory) — namespaces every `Table` id this pass mints so two dbt projects in one
    /// workspace never collide on a shared model name, and is stored on each `Table` as
    /// `properties["dbt_project"]` for display.
    dbt_root: String,
    /// (path relative to the dbt project root, raw YAML content) for every YAML file found under
    /// `models/**/` whose top level has a `models:` and/or `sources:` key.
    yml_files: Vec<(String, String)>,
    /// (path relative to the dbt project root, raw SQL content) for every `models/**/*.sql` file
    /// — one dbt model per file, regardless of whether any YAML documents it.
    sql_files: Vec<(String, String)>,
}

impl DbtAnalyzerPass {
    pub fn new(
        dbt_root: impl Into<String>,
        yml_files: Vec<(String, String)>,
        sql_files: Vec<(String, String)>,
    ) -> Self {
        let dbt_root = dbt_root.into();
        Self {
            pass_id: format!("dbt-analyzer:{dbt_root}"),
            dbt_root,
            yml_files,
            sql_files,
        }
    }
}

#[async_trait]
impl CompilerPass for DbtAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        let mut all: Vec<&(String, String)> =
            self.yml_files.iter().chain(self.sql_files.iter()).collect();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        for (path, content) in all {
            hasher.update(path.as_bytes());
            hasher.update(content.as_bytes());
        }
        vec![hex::encode(hasher.finalize())]
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut graph = KirGraph::new();

        // ── Parse every YAML file up front — documentation to merge onto models, plus the full
        //    source-table list (sources have no `.sql` file of their own). ──────────────────────
        let mut model_docs: HashMap<String, YamlModelDoc> = HashMap::new();
        let mut source_tables: Vec<(YamlSourceTable, String)> = Vec::new(); // (table, yml rel_path)
        for (rel_path, content) in &self.yml_files {
            let doc: serde_yaml::Value = match serde_yaml::from_str(content) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("cannot parse {rel_path} as YAML: {e}");
                    continue;
                }
            };
            let (docs, sources) = parse_yaml_doc(&doc);
            for (name, doc) in docs {
                model_docs.insert(name, doc);
            }
            for source in sources {
                source_tables.push((source, rel_path.clone()));
            }
        }

        // ── Models: one `Table` per `.sql` file, regardless of YAML documentation. ──────────────
        let mut known: HashMap<String, KirId> = HashMap::new();
        for (rel_path, _content) in &self.sql_files {
            let name = model_name_from_path(rel_path);
            known
                .entry(name)
                .or_insert_with_key(|name| dbt_table_kir_id(&self.dbt_root, name));
        }
        for (rel_path, content) in &self.sql_files {
            let name = model_name_from_path(rel_path);
            let Some(&id) = known.get(&name) else {
                continue;
            };
            if graph.objects.iter().any(|o| o.id == id) {
                continue; // a duplicate model filename within this project — keep the first
            }

            let ev = KirEvidence::new(
                SourceLocation::file(rel_path),
                format!("dbt model {rel_path}"),
            );
            let mut obj = KirObject::new(&name, ObjectKind::Table)
                .with_property("dbt_kind", serde_json::json!("model"))
                .with_property("dbt_project", serde_json::json!(self.dbt_root))
                .with_evidence(graph.add_evidence(ev));
            obj.id = id;

            if let Some(materialized) = extract_materialized(content) {
                obj = obj.with_property("materialized", serde_json::json!(materialized));
            }
            if let Some(doc) = model_docs.get(&name) {
                if !doc.columns.is_empty() {
                    obj = obj.with_property("columns", serde_json::json!(doc.columns));
                }
                if let Some(desc) = &doc.description {
                    obj = obj.with_property("description", serde_json::json!(desc));
                }
            }

            graph.add_object(obj);
        }

        // ── Sources: one `Table` per declared `sources[].tables[]` entry — no `.sql` file backs
        //    these, so YAML is their only existence signal. ─────────────────────────────────────
        for (source, yml_rel_path) in &source_tables {
            if known.contains_key(&source.name) {
                continue; // a source name collided with a model name — keep the model, honestly rare
            }
            let id = dbt_table_kir_id(&self.dbt_root, &source.name);
            known.insert(source.name.clone(), id);

            let ev = KirEvidence::new(
                SourceLocation::file(yml_rel_path),
                format!("dbt source {}.{}", source.source_name, source.name),
            );
            let mut obj = KirObject::new(&source.name, ObjectKind::Table)
                .with_property("dbt_kind", serde_json::json!("source"))
                .with_property("dbt_source", serde_json::json!(source.source_name))
                .with_property("dbt_project", serde_json::json!(self.dbt_root))
                .with_evidence(graph.add_evidence(ev));
            obj.id = id;
            if let Some(desc) = &source.description {
                obj = obj.with_property("description", serde_json::json!(desc));
            }
            graph.add_object(obj);
        }

        // ── Dependencies: regex-scan each model's raw SQL for `ref()`/`source()` macro calls,
        //    resolved against `known`. Unresolvable refs (cross-package `ref()` into
        //    `dbt_packages/`, itself gitignored) are honestly skipped, never fabricated. ─────────
        for (rel_path, content) in &self.sql_files {
            let name = model_name_from_path(rel_path);
            let Some(&from_id) = known.get(&name) else {
                continue;
            };
            let mut emitted: HashSet<KirId> = HashSet::new();
            for macro_ref in find_macro_refs(content) {
                let Some(&to_id) = known.get(&macro_ref.target_name) else {
                    tracing::debug!(
                        model = %name,
                        target = %macro_ref.target_name,
                        "dbt-analyzer: unresolved ref()/source() — likely a cross-package \
                         reference (dbt_packages/, gitignored) — skipped, not fabricated"
                    );
                    continue;
                };
                if to_id == from_id || !emitted.insert(to_id) {
                    continue;
                }
                let ev = KirEvidence::new(
                    SourceLocation::at(rel_path, line_at(content, macro_ref.byte_offset)),
                    macro_ref.fragment,
                );
                let mut rel = KirRelationship::new(RelationshipKind::DependsOn, from_id, to_id);
                rel.id = dbt_depends_on_kir_id(from_id, to_id);
                rel.evidence.push(graph.add_evidence(ev));
                graph.add_relationship(rel);
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
            tables = knowledge.content.kir.objects.len(),
            dependencies = knowledge.content.kir.relationships.len(),
            "dbt-analyzer complete"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_compiler_core::EkosConfig;
    use std::sync::Arc;

    fn ctx() -> (PassContext, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (
            PassContext::new(Arc::new(EkosConfig::default()), dir.path().to_path_buf()),
            dir,
        )
    }

    async fn run_pass(
        yml_files: Vec<(&str, &str)>,
        sql_files: Vec<(&str, &str)>,
    ) -> ekos_kir::KirGraph {
        let (mut c, _dir) = ctx();
        let yml = yml_files
            .into_iter()
            .map(|(p, s)| (p.to_string(), s.to_string()))
            .collect();
        let sql = sql_files
            .into_iter()
            .map(|(p, s)| (p.to_string(), s.to_string()))
            .collect();
        let mut pass = DbtAnalyzerPass::new("dbt", yml, sql);
        pass.run(&mut c).await.unwrap();

        let ids = c.artifact_store.list().unwrap();
        assert_eq!(ids.len(), 1, "exactly one KnowledgeArtifact expected");
        let json = c.artifact_store.read(&ids[0]).unwrap().unwrap();
        let knowledge: ekos_artifact::KnowledgeArtifact = serde_json::from_value(json).unwrap();
        knowledge.content.kir
    }

    const SILVER_MODELS_YML: &str = r#"
version: 2
models:
  - name: silver_customer
    columns:
      - name: customer_id
        tests: [not_null, unique]
      - name: is_active
        tests: [not_null]
    description: "Cleaned customer records."
"#;

    const SILVER_SOURCES_YML: &str = r#"
version: 2
sources:
  - name: bronze
    schema: dvdrental
    tables:
      - name: bronze_customer
        description: "Raw customer rows from dvdrental."
"#;

    const SILVER_CUSTOMER_SQL: &str = r#"
{{ config(materialized='incremental', unique_key='customer_id') }}
SELECT * FROM {{ source('bronze', 'bronze_customer') }}
WHERE _is_deleted = false
"#;

    const SEM_CUSTOMER_CONTEXT_SQL: &str = r#"
WITH customer AS (
    SELECT * FROM {{ ref('silver_customer') }}
),
again AS (
    SELECT * FROM {{ ref('silver_customer') }}
)
SELECT * FROM customer
"#;

    #[tokio::test]
    async fn model_without_any_yaml_doc_still_becomes_a_table() {
        let graph = run_pass(
            vec![],
            vec![("models/silver/silver_customer.sql", SILVER_CUSTOMER_SQL)],
        )
        .await;
        assert_eq!(graph.objects.len(), 1);
        let table = &graph.objects[0];
        assert_eq!(table.name, "silver_customer");
        assert_eq!(table.kind, ObjectKind::Table);
        assert_eq!(table.properties["dbt_kind"], "model");
        assert!(
            table.properties.get("columns").is_none(),
            "no fabricated columns when no YAML documents this model"
        );
        assert_eq!(table.properties["materialized"], "incremental");
    }

    #[tokio::test]
    async fn yaml_documented_columns_and_description_are_merged_onto_the_model() {
        let graph = run_pass(
            vec![("models/silver/_silver_models.yml", SILVER_MODELS_YML)],
            vec![("models/silver/silver_customer.sql", SILVER_CUSTOMER_SQL)],
        )
        .await;
        let table = graph
            .objects
            .iter()
            .find(|o| o.name == "silver_customer")
            .unwrap();
        let cols = table.properties["columns"].as_array().unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(table.properties["description"], "Cleaned customer records.");
    }

    #[tokio::test]
    async fn source_table_has_no_sql_file_but_still_becomes_a_table() {
        let graph = run_pass(
            vec![("models/silver/_silver_sources.yml", SILVER_SOURCES_YML)],
            vec![],
        )
        .await;
        assert_eq!(graph.objects.len(), 1);
        let table = &graph.objects[0];
        assert_eq!(table.name, "bronze_customer");
        assert_eq!(table.properties["dbt_kind"], "source");
        assert_eq!(table.properties["dbt_source"], "bronze");
    }

    #[tokio::test]
    async fn source_macro_call_resolves_to_a_real_depends_on_edge() {
        let graph = run_pass(
            vec![("models/silver/_silver_sources.yml", SILVER_SOURCES_YML)],
            vec![("models/silver/silver_customer.sql", SILVER_CUSTOMER_SQL)],
        )
        .await;
        assert_eq!(graph.relationships.len(), 1);
        let rel = &graph.relationships[0];
        assert_eq!(rel.kind, RelationshipKind::DependsOn);
        let from = graph.objects.iter().find(|o| o.id == rel.from).unwrap();
        let to = graph.objects.iter().find(|o| o.id == rel.to).unwrap();
        assert_eq!(from.name, "silver_customer");
        assert_eq!(to.name, "bronze_customer");
    }

    #[tokio::test]
    async fn repeated_ref_to_the_same_model_produces_one_edge_not_two() {
        let graph = run_pass(
            vec![],
            vec![
                ("models/silver/silver_customer.sql", SILVER_CUSTOMER_SQL),
                (
                    "models/semantic/sem_customer_context.sql",
                    SEM_CUSTOMER_CONTEXT_SQL,
                ),
            ],
        )
        .await;
        let deps: Vec<_> = graph
            .relationships
            .iter()
            .filter(|r| {
                let from = graph.objects.iter().find(|o| o.id == r.from).unwrap();
                from.name == "sem_customer_context"
            })
            .collect();
        assert_eq!(
            deps.len(),
            1,
            "two ref('silver_customer') calls in one model must dedupe to one DependsOn edge"
        );
    }

    #[tokio::test]
    async fn ref_to_an_undeclared_model_is_honestly_skipped_not_fabricated() {
        const ORPHAN_SQL: &str = "SELECT * FROM {{ ref('some_package_model') }}";
        let graph = run_pass(vec![], vec![("models/gold/gold_thing.sql", ORPHAN_SQL)]).await;
        assert_eq!(
            graph.objects.len(),
            1,
            "gold_thing itself is still a real model"
        );
        assert_eq!(
            graph.relationships.len(),
            0,
            "an unresolvable ref() (e.g. a cross-package reference) must not fabricate an edge"
        );
    }

    #[tokio::test]
    async fn malformed_yaml_is_skipped_without_aborting_the_rest() {
        const BROKEN: &str = "not: [valid yaml: {{{";
        let graph = run_pass(
            vec![("models/silver/_broken.yml", BROKEN)],
            vec![("models/silver/silver_customer.sql", SILVER_CUSTOMER_SQL)],
        )
        .await;
        assert_eq!(graph.objects.len(), 1);
    }

    #[tokio::test]
    async fn table_ids_are_deterministic_and_project_namespaced() {
        let graph1 = run_pass(
            vec![],
            vec![("models/silver/silver_customer.sql", SILVER_CUSTOMER_SQL)],
        )
        .await;
        let graph2 = run_pass(
            vec![],
            vec![("models/silver/silver_customer.sql", SILVER_CUSTOMER_SQL)],
        )
        .await;
        assert_eq!(graph1.objects[0].id, graph2.objects[0].id);
        assert_eq!(
            graph1.objects[0].id,
            dbt_table_kir_id("dbt", "silver_customer")
        );
    }

    #[tokio::test]
    async fn nothing_found_emits_nothing() {
        let (mut c, _dir) = ctx();
        let mut pass = DbtAnalyzerPass::new("dbt", vec![], vec![]);
        pass.run(&mut c).await.unwrap();
        assert!(c.artifact_store.list().unwrap().is_empty());
    }
}
