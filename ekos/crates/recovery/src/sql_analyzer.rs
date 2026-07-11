//! `SqlAnalyzerPass` — extracts `KirObject`s (tables as entities) and
//! `KirRelationship`s (FK edges) from SQL DDL, then uses LLM to add
//! semantic names and descriptions.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use sqlparser::ast::{ColumnOption, Statement, TableConstraint};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use tracing::warn;

use crate::llm::{LlmProvider, LlmRequest};

const SYSTEM_PROMPT: &str = r#"You are an expert database and domain modeler.
Given SQL DDL, identify the business domain entities and their semantic relationships.

For each table determine:
1. Whether it is a core business entity, a junction/mapping table, or a lookup/reference table.
2. The singular business-concept name (e.g. "Customer" not "customers").
3. The semantic meaning of each foreign-key relationship.

Respond ONLY with valid JSON in this exact schema — no markdown fences, no commentary:
{
  "entities": [
    {"table": "<table_name>", "entity_name": "<PascalCase>", "type": "core|junction|lookup", "description": "<one sentence>"}
  ],
  "relationships": [
    {"from_table": "<table>", "to_table": "<table>", "semantic_name": "<snake_case>", "description": "<one sentence>"}
  ]
}"#;

const PROMPT_VERSION: &str = "sql-analyzer-v1";

// ── Compiler pass ────────────────────────────────────────────────────────────

pub struct SqlAnalyzerPass {
    /// Human-readable name (usually the source file path).
    pass_id: String,
    /// Raw SQL DDL content.
    sql: String,
    /// Source file path for evidence records.
    source_path: String,
    /// LLM provider (may be a `CachedLlmProvider` wrapping `AnthropicProvider`).
    llm: Arc<dyn LlmProvider>,
}

impl SqlAnalyzerPass {
    pub fn new(
        source_path: impl Into<String>,
        sql: impl Into<String>,
        llm: Arc<dyn LlmProvider>,
    ) -> Self {
        let source_path = source_path.into();
        let pass_id = format!("sql-analyzer:{source_path}");
        Self { pass_id, sql: sql.into(), source_path, llm }
    }
}

#[async_trait]
impl CompilerPass for SqlAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.sql.as_bytes());
        vec![hex::encode(hasher.finalize())]
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        // ── Structural parse ────────────────────────────────────────────────
        let mut graph = parse_ddl_structural(&self.sql, &self.source_path);

        if graph.objects.is_empty() {
            ctx.diagnostics.lock().unwrap().warning(
                "SQL001",
                format!("no tables found in {}", self.source_path),
            );
            return Ok(());
        }

        // ── LLM semantic enrichment ─────────────────────────────────────────
        let req = LlmRequest {
            system: SYSTEM_PROMPT,
            user: &self.sql,
            prompt_version: PROMPT_VERSION,
            max_tokens: 4096,
        };

        match self.llm.complete(&req).await {
            Ok(resp) => {
                if let Err(e) = apply_llm_enrichment(&mut graph, &resp.content) {
                    ctx.diagnostics.lock().unwrap().warning(
                        "SQL002",
                        format!("LLM enrichment parse failed for {}: {e}", self.source_path),
                    );
                }
            }
            Err(e) => {
                ctx.diagnostics.lock().unwrap().warning(
                    "SQL003",
                    format!(
                        "LLM call failed for {} (structural analysis still applied): {e}",
                        self.source_path
                    ),
                );
            }
        }

        // ── Write KnowledgeArtifact ─────────────────────────────────────────
        let knowledge = ekos_artifact::KnowledgeArtifact::new(
            &self.pass_id,
            vec![], // input IDs wired in Phase 9 when we thread artifact IDs through
            graph,
        );
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            objects = knowledge.content.kir.objects.len(),
            relationships = knowledge.content.kir.relationships.len(),
            "sql-analyzer complete"
        );
        Ok(())
    }
}

// ── Structural DDL parser ────────────────────────────────────────────────────

/// Parse SQL DDL and return a `KirGraph` with tables as `KirObject`s and FK
/// constraints as `KirRelationship`s. No LLM; pure structural extraction.
pub fn parse_ddl_structural(sql: &str, source_path: &str) -> KirGraph {
    let mut graph = KirGraph::new();

    let dialect = GenericDialect {};
    let stmts = match Parser::parse_sql(&dialect, sql) {
        Ok(s) => s,
        Err(e) => {
            warn!("sqlparser failed on {source_path}: {e}; falling back to empty graph");
            return graph;
        }
    };

    // First pass: create KirObject per table, collect name → KirId mapping.
    let mut table_ids: HashMap<String, KirId> = HashMap::new();

    for stmt in &stmts {
        if let Statement::CreateTable(ct) = stmt {
            let table_name = ct.name.to_string();
            let ev = KirEvidence::new(
                SourceLocation::file(source_path),
                format!("CREATE TABLE {table_name}"),
            );
            let ev_id = graph.add_evidence(ev);

            let mut obj = KirObject::new(&table_name, ObjectKind::Table).with_evidence(ev_id);
            obj.properties.insert("columns".into(), columns_json(ct));
            let obj_id = graph.add_object(obj);
            table_ids.insert(table_name.to_lowercase(), obj_id);
        }
    }

    // Second pass: extract FK constraints into KirRelationship.
    // Handles both table-level CONSTRAINT FOREIGN KEY and inline column REFERENCES.
    for stmt in &stmts {
        if let Statement::CreateTable(ct) = stmt {
            let from_name = ct.name.to_string().to_lowercase();
            let from_id = match table_ids.get(&from_name) {
                Some(&id) => id,
                None => continue,
            };

            // Table-level: FOREIGN KEY (col) REFERENCES tbl(col)
            for constraint in &ct.constraints {
                if let TableConstraint::ForeignKey {
                    foreign_table,
                    referred_columns,
                    columns: fk_columns,
                    ..
                } = constraint
                {
                    let to_name = foreign_table.to_string().to_lowercase();
                    if let Some(&to_id) = table_ids.get(&to_name) {
                        add_fk_relationship(
                            &mut graph, source_path, from_id, to_id,
                            &from_name, &col_names(fk_columns),
                            &to_name, &col_names(referred_columns),
                        );
                    }
                }
            }

            // Inline: col_name INT REFERENCES other_table(id)
            for col in &ct.columns {
                for opt in &col.options {
                    if let ColumnOption::ForeignKey {
                        foreign_table,
                        referred_columns,
                        ..
                    } = &opt.option
                    {
                        let to_name = foreign_table.to_string().to_lowercase();
                        if let Some(&to_id) = table_ids.get(&to_name) {
                            let ref_cols = if referred_columns.is_empty() {
                                "id".to_string()
                            } else {
                                referred_columns.iter().map(|c| c.value.as_str()).collect::<Vec<_>>().join(", ")
                            };
                            add_fk_relationship(
                                &mut graph, source_path, from_id, to_id,
                                &from_name, &col.name.value,
                                &to_name, &ref_cols,
                            );
                        }
                    }
                }
            }
        }
    }

    graph
}

#[allow(clippy::too_many_arguments)]
fn add_fk_relationship(
    graph: &mut KirGraph,
    source_path: &str,
    from_id: KirId,
    to_id: KirId,
    from_name: &str,
    from_col: &str,
    to_name: &str,
    to_col: &str,
) {
    let fk_desc = format!("{from_name}.{from_col} → {to_name}.{to_col}");
    let ev = KirEvidence::new(SourceLocation::file(source_path), fk_desc.clone());
    let ev_id = graph.add_evidence(ev);
    let mut rel = KirRelationship::new(RelationshipKind::ForeignKey, from_id, to_id);
    rel.properties.insert("fk_desc".into(), serde_json::Value::String(fk_desc));
    rel.evidence.push(ev_id);
    graph.add_relationship(rel);
}

fn col_names(cols: &[sqlparser::ast::Ident]) -> String {
    cols.iter().map(|c| c.value.as_str()).collect::<Vec<_>>().join(", ")
}

fn columns_json(ct: &sqlparser::ast::CreateTable) -> serde_json::Value {
    let cols: Vec<serde_json::Value> = ct
        .columns
        .iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name.value,
                "data_type": c.data_type.to_string(),
            })
        })
        .collect();
    serde_json::Value::Array(cols)
}

// ── LLM enrichment application ───────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct LlmOutput {
    entities: Vec<LlmEntity>,
    relationships: Vec<LlmRelationship>,
}

#[derive(serde::Deserialize)]
struct LlmEntity {
    table: String,
    entity_name: String,
    #[serde(rename = "type")]
    entity_type: String,
    description: String,
}

#[derive(serde::Deserialize)]
struct LlmRelationship {
    from_table: String,
    to_table: String,
    semantic_name: String,
    description: String,
}

fn apply_llm_enrichment(graph: &mut KirGraph, llm_text: &str) -> anyhow::Result<()> {
    // Strip markdown fences if present.
    let json_str = llm_text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let output: LlmOutput = serde_json::from_str(json_str)?;

    // Apply entity enrichment.
    for entity in &output.entities {
        let table_lc = entity.table.to_lowercase();
        if let Some(obj) = graph
            .objects
            .iter_mut()
            .find(|o| o.name.to_lowercase() == table_lc)
        {
            obj.properties.insert(
                "entity_name".into(),
                serde_json::Value::String(entity.entity_name.clone()),
            );
            obj.properties.insert(
                "entity_type".into(),
                serde_json::Value::String(entity.entity_type.clone()),
            );
            obj.properties.insert(
                "description".into(),
                serde_json::Value::String(entity.description.clone()),
            );
        }
    }

    // Apply relationship semantic names.
    for sem_rel in &output.relationships {
        let from_lc = sem_rel.from_table.to_lowercase();
        let to_lc = sem_rel.to_table.to_lowercase();

        // Find the KirObject IDs for from/to tables.
        let from_id =
            graph.objects.iter().find(|o| o.name.to_lowercase() == from_lc).map(|o| o.id);
        let to_id =
            graph.objects.iter().find(|o| o.name.to_lowercase() == to_lc).map(|o| o.id);

        if let (Some(fid), Some(tid)) = (from_id, to_id)
            && let Some(rel) = graph.relationships.iter_mut().find(|r| r.from == fid && r.to == tid)
        {
            rel.properties.insert(
                "semantic_name".into(),
                serde_json::Value::String(sem_rel.semantic_name.clone()),
            );
            rel.properties.insert(
                "description".into(),
                serde_json::Value::String(sem_rel.description.clone()),
            );
        }
    }

    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockLlmProvider;
    use ekos_compiler_core::pass::PassContext;
    use std::sync::Arc;
    use tempfile::TempDir;

    const ECOMMERCE_SQL: &str = include_str!("../../../../tests/fixtures/ecommerce.sql");

    fn make_ctx(dir: &TempDir) -> PassContext {
        use std::sync::Arc;
        let mut config = ekos_compiler_core::EkosConfig::default();
        config.observe.ignore_patterns = vec![];
        let cwd = dir.path().to_path_buf();
        std::fs::create_dir_all(cwd.join(".ekos/artifacts")).unwrap();
        PassContext::new(Arc::new(config), cwd)
    }

    #[test]
    fn structural_parse_extracts_six_tables() {
        let graph = parse_ddl_structural(ECOMMERCE_SQL, "ecommerce.sql");
        assert_eq!(
            graph.objects.len(),
            6,
            "ecommerce schema has 6 tables: categories, customers, products, orders, order_items, payments"
        );
    }

    #[test]
    fn structural_parse_extracts_fk_relationships() {
        let graph = parse_ddl_structural(ECOMMERCE_SQL, "ecommerce.sql");
        // orders→customers, order_items→orders, order_items→products, payments→orders,
        // products→categories, categories→categories (self-ref)
        assert!(graph.relationships.len() >= 5, "expected ≥5 FK relationships, got {}", graph.relationships.len());
    }

    #[test]
    fn structural_parse_table_has_columns() {
        let graph = parse_ddl_structural(ECOMMERCE_SQL, "ecommerce.sql");
        let customers = graph.objects.iter().find(|o| o.name.to_lowercase() == "customers");
        assert!(customers.is_some());
        let cols = &customers.unwrap().properties["columns"];
        assert!(cols.is_array());
        assert!(!cols.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn pass_runs_with_mock_llm() {
        let dir = TempDir::new().unwrap();
        let mock_resp = serde_json::json!({
            "entities": [
                {"table": "customers", "entity_name": "Customer", "type": "core", "description": "A person who buys things."}
            ],
            "relationships": [
                {"from_table": "orders", "to_table": "customers", "semantic_name": "placed_by", "description": "An order placed by a customer."}
            ]
        });
        let mock = Arc::new(MockLlmProvider::new(mock_resp.to_string()));
        let mut pass = SqlAnalyzerPass::new("ecommerce.sql", ECOMMERCE_SQL, mock);
        let mut ctx = make_ctx(&dir);
        pass.run(&mut ctx).await.unwrap();
        assert!(!ctx.diagnostics.lock().unwrap().has_errors(), "no errors expected with mock llm");
    }

    #[tokio::test]
    async fn pass_tolerates_bad_llm_json() {
        let dir = TempDir::new().unwrap();
        let mock = Arc::new(MockLlmProvider::new("not valid json at all!!"));
        let mut pass = SqlAnalyzerPass::new("ecommerce.sql", ECOMMERCE_SQL, mock);
        let mut ctx = make_ctx(&dir);
        // Should not return an error — bad LLM response degrades to structural-only.
        pass.run(&mut ctx).await.unwrap();
        assert!(!ctx.diagnostics.lock().unwrap().has_errors());
    }

    #[test]
    fn llm_enrichment_applies_entity_names() {
        let mut graph = parse_ddl_structural(ECOMMERCE_SQL, "ecommerce.sql");
        let llm_json = serde_json::json!({
            "entities": [
                {"table": "customers", "entity_name": "Customer", "type": "core", "description": "desc"}
            ],
            "relationships": []
        })
        .to_string();
        apply_llm_enrichment(&mut graph, &llm_json).unwrap();
        let customers = graph.objects.iter().find(|o| o.name.to_lowercase() == "customers").unwrap();
        assert_eq!(customers.properties["entity_name"], "Customer");
    }
}
