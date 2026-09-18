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
use ekos_sql_dialect_sdk::SqlDialectParser;
use sqlparser::ast::{ColumnOption, Statement, TableConstraint};
use sqlparser::dialect::Dialect;
use sqlparser::parser::Parser;
use tracing::warn;
use uuid::Uuid;

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
    /// SQL DDL content, already dialect-preprocessed (e.g. MySQL `DELIMITER` stripping) —
    /// see `SqlDialectParser::preprocess`.
    sql: String,
    /// Source file path for evidence records.
    source_path: String,
    /// LLM provider (may be a `CachedLlmProvider` wrapping `AnthropicProvider`).
    llm: Arc<dyn LlmProvider>,
    /// Resolved at construction time from a `SqlDialectParser` (RFC 0031) — which
    /// `sqlparser` dialect grammar to parse `sql` with.
    dialect: Box<dyn Dialect + Send + Sync>,
    /// `COMMENT ON TABLE`/`COLUMN` text lifted from the raw SQL before `preprocess` removed it
    /// (RFC 0146 Phase 2). Applied to the graph after the structural parse, ahead of the LLM.
    comments: Vec<crate::sql_comments::SqlComment>,
    /// Explicit output-token ceiling from `[llm] max-tokens` (RFC 0146 Phase 3). `None` — the
    /// normal case — means the budget is computed per file from the size of the graph.
    max_tokens_override: Option<u32>,
}

impl SqlAnalyzerPass {
    pub fn new(
        source_path: impl Into<String>,
        sql: impl Into<String>,
        llm: Arc<dyn LlmProvider>,
        dialect_parser: &dyn SqlDialectParser,
    ) -> Self {
        let source_path = source_path.into();
        let pass_id = format!("sql-analyzer:{source_path}");
        let raw_sql: String = sql.into();
        // RFC 0146 Phase 2: harvest `COMMENT ON` text from the *raw* file first. Phase 1's
        // `preprocess` strips those statements so the rest of the file can parse, so this is the
        // only point at which the schema's own documentation is still present.
        let comments = crate::sql_comments::extract_sql_comments(&raw_sql);
        let preprocessed = dialect_parser.preprocess(&raw_sql);
        Self {
            pass_id,
            sql: preprocessed,
            comments,
            source_path,
            llm,
            dialect: dialect_parser.sqlparser_dialect(),
            max_tokens_override: None,
        }
    }

    /// Pins the LLM output-token ceiling instead of letting the pass size it per file
    /// (`[llm] max-tokens`, RFC 0146 Phase 3). Additive so no existing caller changes.
    pub fn with_max_tokens(mut self, max_tokens: Option<u32>) -> Self {
        self.max_tokens_override = max_tokens;
        self
    }
}

/// Output-token ceiling for one file's semantic-enrichment call.
///
/// The response must carry one JSON object per table and one per foreign key, so the budget has to
/// scale with the schema — a fixed ceiling is wrong in both directions at once. RFC 0146 Phase 3
/// replaced a hardcoded 4,096, which was generous for a 3-table migration and far too small for
/// LedgerSMB's `Pg-database.sql` (158 tables, 214 foreign keys): that file's enrichment came back
/// empty or covering a fraction of the schema, with no diagnostic, on three separate real runs.
///
/// `PER_ITEM_TOKENS` is measured against the prompt's own output schema — an entity line
/// (`{"table":…,"entity_name":…,"type":…,"description":"<one sentence>"}`) runs 40-60 tokens, so 80
/// leaves room for a long table name and a wordy sentence. Reasoning models spend hidden tokens
/// from the same budget, which the headroom also absorbs.
///
/// The floor keeps small files exactly where they were. The ceiling is a cost guard: a runaway
/// response is capped, and a schema big enough to need more should say so explicitly via
/// `[llm] max-tokens`.
///
/// `MAX_TOKENS` is calibrated against that same file rather than guessed. A first cut set it to
/// 16,384, which silently clamped the formula's own 30,272 estimate to half and reproduced the
/// original failure exactly — an empty response and `SQL002`. Re-running with an explicit
/// `[llm] max-tokens = 32768` named 157 of the 158 tables, so the formula had been right and the
/// guard was wrong. 32,768 is set here to let a schema of this size through untouched.
pub fn enrichment_token_budget(tables: usize, relationships: usize) -> u32 {
    const BASE_TOKENS: u32 = 512;
    const PER_ITEM_TOKENS: u32 = 80;
    const MIN_TOKENS: u32 = 4_096;
    const MAX_TOKENS: u32 = 32_768;

    let items = (tables + relationships) as u32;
    BASE_TOKENS
        .saturating_add(PER_ITEM_TOKENS.saturating_mul(items))
        .clamp(MIN_TOKENS, MAX_TOKENS)
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
        let mut graph = parse_ddl_structural(&self.sql, &self.source_path, &*self.dialect);

        if graph.objects.is_empty() {
            ctx.diagnostics
                .lock()
                .unwrap()
                .warning("SQL001", format!("no tables found in {}", self.source_path));
            return Ok(());
        }

        // ── Author-written descriptions (RFC 0146 Phase 2) ──────────────────
        // Applied before the LLM so generated prose can never displace the schema's own
        // documentation — see `apply_sql_comments`.
        let described = apply_sql_comments(&mut graph, &self.comments, &self.source_path);
        if described > 0 {
            tracing::debug!(
                "sql-analyzer: {described} description(s) recovered from COMMENT ON in {}",
                self.source_path
            );
        }

        // ── LLM semantic enrichment ─────────────────────────────────────────
        let table_count = graph
            .objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Table)
            .count();
        let max_tokens = self
            .max_tokens_override
            .unwrap_or_else(|| enrichment_token_budget(table_count, graph.relationships.len()));

        let req = LlmRequest {
            system: SYSTEM_PROMPT,
            user: &self.sql,
            prompt_version: PROMPT_VERSION,
            max_tokens,
            history: &[],
        };

        match self.llm.complete(&req).await {
            Ok(resp) => {
                let hit_ceiling = resp.output_tokens >= max_tokens;
                match apply_llm_enrichment(&mut graph, &resp.content) {
                    Ok(named) => {
                        // RFC 0146 Phase 3: partial enrichment used to be completely silent. On a
                        // real LedgerSMB run the model described 24 of 192 tables and nothing
                        // reported it — worse than an outright failure, because the gap looks
                        // exactly like a schema that simply has no descriptions.
                        if named < table_count {
                            let cause = if hit_ceiling {
                                format!(
                                    " (response hit the {max_tokens}-token ceiling — raise `[llm] max-tokens`)"
                                )
                            } else {
                                String::new()
                            };
                            ctx.diagnostics.lock().unwrap().warning(
                                "SQL004",
                                format!(
                                    "LLM enrichment named {named} of {table_count} tables in {}{cause}",
                                    self.source_path
                                ),
                            );
                        }
                    }
                    Err(e) => {
                        let cause = if hit_ceiling {
                            format!(
                                " (response hit the {max_tokens}-token ceiling — raise `[llm] max-tokens`)"
                            )
                        } else {
                            String::new()
                        };
                        ctx.diagnostics.lock().unwrap().warning(
                            "SQL002",
                            format!(
                                "LLM enrichment parse failed for {}: {e}{cause}",
                                self.source_path
                            ),
                        );
                    }
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

// ── Deterministic ids (RFC 0076) ─────────────────────────────────────────────
//
// `parse_ddl_structural` used to let `KirObject::new`/`KirRelationship::new` mint their default
// random ids — unlike every sibling analyzer in this crate (`clickhouse_analyzer.rs`,
// `crate_topology_analyzer.rs`, `local_docs_analyzer.rs`, `git_analyzer.rs`, `github_analyzer.rs`,
// `cicd_analyzer.rs`, `dependency_analyzer.rs`, `confluence_analyzer.rs`,
// `document_semantics_analyzer.rs`, `crypto_analyzer.rs`), which all assign a deterministic id.
// Found live, on a real project re-recovered a second time months after its first `ekos recover`
// run: every one of that workspace's real tables existed twice in the ledger, with two different
// random ids, because `append_object`'s `(id, content_signature)` versioning (RFC 0015) never
// recognized the freshly re-parsed `Table` as "the same one" already committed. The exact failure
// class RFC 0072 root-caused for `crate_topology_analyzer.rs`'s `DependsOn` edges — this is its
// `Table`/`ForeignKey` counterpart.

/// A table is a boolean fact per normalized name within this pass — no legitimate multiplicity,
/// so this is exactly the shape RFC 0072 established as safe to key on a stable id. Lowercased to
/// match `parse_ddl_structural`'s own internal `table_ids` lookup convention (unquoted SQL
/// identifiers are case-folded by most real dialects). Prefixed distinctly from
/// `clickhouse_analyzer.rs`'s own `table_kir_id` (`"clickhouse:"`) so a same-named table recovered
/// by each analyzer never silently collides onto one id — two tables from two different systems
/// merging is RFC 0029 cross-system identity's job, never an accidental hash collision.
fn table_kir_id(table_name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("sql-analyzer-table:{}", table_name.to_lowercase()).as_bytes(),
    ))
}

/// Unlike `table_kir_id`, `(from, to)` alone is **not** safe here — RFC 0072 found a real,
/// already-shipped case where it isn't: a table with two FK columns to the same target table
/// produces two real, distinct `ForeignKey` edges sharing the same `(from, to)` pair, distinguished
/// only by which columns are involved. `fk_desc` (`"from.col → to.col"`, already computed by every
/// caller) is exactly that distinguishing signal, so it's part of the id input, not just the
/// evidence text.
fn foreign_key_kir_id(from: KirId, to: KirId, fk_desc: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("sql-analyzer-fk:{from}:{to}:{fk_desc}").as_bytes(),
    ))
}

// ── Structural DDL parser ────────────────────────────────────────────────────

/// Parse SQL DDL and return a `KirGraph` with tables as `KirObject`s and FK
/// constraints as `KirRelationship`s. No LLM; pure structural extraction.
///
/// `dialect` (RFC 0031) is resolved by the caller — `recover.rs` via the dialect registry, or
/// `&GenericDialect {}` directly for ANSI-baseline callers/tests that don't need the full
/// `SqlDialectParser` machinery.
pub fn parse_ddl_structural(sql: &str, source_path: &str, dialect: &dyn Dialect) -> KirGraph {
    let mut graph = KirGraph::new();

    let stmts = match Parser::parse_sql(dialect, sql) {
        Ok(s) => s,
        Err(first_err) => {
            // Fallback (GitHub issue #3): some hand-written scripts omit `;` between top-level
            // statements — retry once with synthetic separators inserted. See
            // `statement_repair`'s doc comment for why this is only attempted after the
            // unmodified text has already failed to parse.
            let repaired = crate::statement_repair::ensure_statement_separators(sql);
            match Parser::parse_sql(dialect, &repaired) {
                Ok(s) => s,
                Err(_) => {
                    warn!(
                        "sqlparser failed on {source_path}: {first_err}; falling back to empty graph"
                    );
                    return graph;
                }
            }
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
            obj.id = table_kir_id(&table_name);
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
                            &mut graph,
                            source_path,
                            from_id,
                            to_id,
                            &from_name,
                            &col_names(fk_columns),
                            &to_name,
                            &col_names(referred_columns),
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
                                referred_columns
                                    .iter()
                                    .map(|c| c.value.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            };
                            add_fk_relationship(
                                &mut graph,
                                source_path,
                                from_id,
                                to_id,
                                &from_name,
                                &col.name.value,
                                &to_name,
                                &ref_cols,
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
    rel.id = foreign_key_kir_id(from_id, to_id, &fk_desc);
    rel.properties
        .insert("fk_desc".into(), serde_json::Value::String(fk_desc));
    rel.evidence.push(ev_id);
    graph.add_relationship(rel);
}

fn col_names(cols: &[sqlparser::ast::Ident]) -> String {
    cols.iter()
        .map(|c| c.value.as_str())
        .collect::<Vec<_>>()
        .join(", ")
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

// ── Author-written description application (RFC 0146 Phase 2) ────────────────

/// Property carrying a description that came from the schema itself rather than from a model.
/// Kept distinct from `description` so the provenance survives into the ledger and a consumer can
/// tell an observed fact from an inferred one without consulting evidence records.
const SQL_COMMENT_PROPERTY: &str = "sql_comment";

/// Attaches `COMMENT ON` text to the matching `Table` objects and their columns, with a
/// `KirEvidence` record pointing at the exact source line.
///
/// Returns how many descriptions were applied.
///
/// Table matching is case-insensitive and ignores schema qualification, because DDL recovery keys
/// `Table` objects on the bare name `CREATE TABLE` used while `COMMENT ON` frequently qualifies it
/// (`public.account`). A comment naming a table this file does not create is skipped rather than
/// creating a bare object — the comment is evidence *about* a table, not evidence that one exists.
fn apply_sql_comments(
    graph: &mut KirGraph,
    comments: &[crate::sql_comments::SqlComment],
    source_path: &str,
) -> usize {
    use crate::sql_comments::CommentTarget;

    let mut applied = 0;

    for comment in comments {
        let bare = comment.bare_table().to_lowercase();
        let Some(obj_index) = graph.objects.iter().position(|o| {
            o.kind == ObjectKind::Table
                && o.name
                    .rsplit('.')
                    .next()
                    .unwrap_or(&o.name)
                    .eq_ignore_ascii_case(&bare)
        }) else {
            continue;
        };

        let fragment = match &comment.target {
            CommentTarget::Table(t) => format!("COMMENT ON TABLE {t}"),
            CommentTarget::Column { table, column } => {
                format!("COMMENT ON COLUMN {table}.{column}")
            }
        };
        let ev = KirEvidence::new(
            SourceLocation::at(source_path, comment.line),
            format!("{fragment} IS {}", comment.text),
        );
        let ev_id = graph.add_evidence(ev);

        let obj = &mut graph.objects[obj_index];
        match &comment.target {
            CommentTarget::Table(_) => {
                obj.properties.insert(
                    SQL_COMMENT_PROPERTY.into(),
                    serde_json::Value::String(comment.text.clone()),
                );
                obj.properties.insert(
                    "description".into(),
                    serde_json::Value::String(comment.text.clone()),
                );
                obj.evidence.push(ev_id);
                applied += 1;
            }
            CommentTarget::Column { column, .. } => {
                if apply_column_comment(obj, column, &comment.text) {
                    obj.evidence.push(ev_id);
                    applied += 1;
                }
            }
        }
    }

    applied
}

/// Writes `text` onto the matching entry of a `Table` object's `columns` property array, which is
/// where `columns_json` records each column's name and data type. Returns whether a column
/// matched — a comment on a column the DDL does not declare is dropped rather than inventing one.
fn apply_column_comment(obj: &mut KirObject, column: &str, text: &str) -> bool {
    let Some(serde_json::Value::Array(columns)) = obj.properties.get_mut("columns") else {
        return false;
    };
    for entry in columns.iter_mut() {
        let matches = entry
            .get("name")
            .and_then(|n| n.as_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(column));
        if matches && let Some(map) = entry.as_object_mut() {
            map.insert(
                "description".into(),
                serde_json::Value::String(text.to_string()),
            );
            return true;
        }
    }
    false
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

/// Returns how many tables the model actually named, which the caller compares against the schema
/// to detect partial coverage (RFC 0146 Phase 3). Counting matched tables rather than returned
/// entities is deliberate: a model that invents a table this file never declared has not enriched
/// anything, and must not make the coverage look complete.
fn apply_llm_enrichment(graph: &mut KirGraph, llm_text: &str) -> anyhow::Result<usize> {
    let output: LlmOutput = serde_json::from_str(crate::llm_json::strip_json_fences(llm_text))?;
    let mut named = 0usize;

    // Apply entity enrichment.
    for entity in &output.entities {
        let table_lc = entity.table.to_lowercase();
        if let Some(obj) = graph
            .objects
            .iter_mut()
            .find(|o| o.name.to_lowercase() == table_lc)
        {
            named += 1;
            obj.properties.insert(
                "entity_name".into(),
                serde_json::Value::String(entity.entity_name.clone()),
            );
            obj.properties.insert(
                "entity_type".into(),
                serde_json::Value::String(entity.entity_type.clone()),
            );
            // RFC 0146 Phase 2: an author-written `COMMENT ON` description outranks a generated
            // one. The model still contributes `entity_name`/`entity_type`, which the schema does
            // not state anywhere, but it never overwrites text a human wrote about their own
            // table — that would replace an observed fact with an inferred one, which is the
            // opposite of what this compiler is for. Its version is kept under
            // `llm_description` so the two remain comparable.
            if obj.properties.contains_key(SQL_COMMENT_PROPERTY) {
                obj.properties.insert(
                    "llm_description".into(),
                    serde_json::Value::String(entity.description.clone()),
                );
            } else {
                obj.properties.insert(
                    "description".into(),
                    serde_json::Value::String(entity.description.clone()),
                );
            }
        }
    }

    // Apply relationship semantic names.
    for sem_rel in &output.relationships {
        let from_lc = sem_rel.from_table.to_lowercase();
        let to_lc = sem_rel.to_table.to_lowercase();

        // Find the KirObject IDs for from/to tables.
        let from_id = graph
            .objects
            .iter()
            .find(|o| o.name.to_lowercase() == from_lc)
            .map(|o| o.id);
        let to_id = graph
            .objects
            .iter()
            .find(|o| o.name.to_lowercase() == to_lc)
            .map(|o| o.id);

        if let (Some(fid), Some(tid)) = (from_id, to_id)
            && let Some(rel) = graph
                .relationships
                .iter_mut()
                .find(|r| r.from == fid && r.to == tid)
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

    Ok(named)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockLlmProvider;
    use crate::sql_dialect_registry::GenericDialectParser;
    use ekos_compiler_core::pass::PassContext;
    use sqlparser::dialect::GenericDialect;
    use std::sync::Arc;
    use tempfile::TempDir;

    const ECOMMERCE_SQL: &str = include_str!("../../../../tests/fixtures/ecommerce.sql");
    /// Near-real, open-source fixture: a hand-cleaned ANSI subset of Microsoft's Northwind
    /// sample schema (MIT-licensed; see the fixture file's header for provenance). Deeper
    /// FK graph (13 tables) than `ecommerce.sql`, used to test structural recovery at a more
    /// realistic scale.
    const NORTHWIND_SQL: &str = include_str!("../../../../tests/fixtures/northwind.sql");
    /// Real MySQL DDL from a public GitHub ETL repo
    /// (joseph-higaki/etl_adventureworks_sales_purchases_datamart), copied verbatim during
    /// RFC 0031's testing. Uses `#`-style line comments, which `GenericDialect` rejects —
    /// the exact regression this fixture guards against (GitHub issue #3 / devlog_31).
    const MYSQL_HASH_COMMENTS_SQL: &str =
        include_str!("../../../../tests/fixtures/mysql_hash_comments.sql");

    fn make_ctx(dir: &TempDir) -> PassContext {
        use std::sync::Arc;
        let mut config = ekos_compiler_core::EkosConfig::default();
        config.observe.ignore_patterns = vec![];
        let cwd = dir.path().to_path_buf();
        std::fs::create_dir_all(cwd.join(".ekos/artifacts")).unwrap();
        PassContext::new(Arc::new(config), cwd)
    }

    // ── RFC 0146 Phase 3: the enrichment token budget ──────────────────────────────────────

    #[test]
    fn small_schemas_keep_the_previous_fixed_budget() {
        // A 3-table migration used to get 4096 and must still get it — this change is about
        // large schemas, and must not quietly shrink anything that already worked.
        assert_eq!(enrichment_token_budget(3, 2), 4_096);
        assert_eq!(enrichment_token_budget(0, 0), 4_096);
    }

    #[test]
    /// Calibration guard, measured rather than guessed. LedgerSMB's `Pg-database.sql` is 158
    /// tables + 214 foreign keys, and the real results are unambiguous: at 16,384 the model
    /// returns nothing (`SQL002`), at 32,768 it names 157 of 158. The computed budget for that
    /// shape must therefore survive unclamped — a ceiling below it is not a cost guard, it is the
    /// original bug wearing a different number.
    fn the_real_ledgersmb_core_schema_gets_a_budget_that_actually_works() {
        let budget = enrichment_token_budget(158, 214);
        assert_eq!(budget, 30_272, "512 + 80 * 372");
        assert!(
            budget > 16_384,
            "16384 was measured to fail on this exact schema, got {budget}"
        );
    }

    #[test]
    fn the_budget_is_bounded_so_a_runaway_schema_cannot_run_up_a_bill() {
        assert_eq!(enrichment_token_budget(100_000, 100_000), 32_768);
    }

    #[test]
    fn budget_grows_monotonically_between_the_floor_and_the_ceiling() {
        let a = enrichment_token_budget(60, 60);
        let b = enrichment_token_budget(120, 120);
        assert!(
            a > 4_096 && a < 32_768,
            "expected a mid-range budget, got {a}"
        );
        assert!(
            b > a,
            "more tables must not get a smaller budget: {b} vs {a}"
        );
    }

    #[tokio::test]
    async fn an_explicit_config_ceiling_overrides_the_computed_one() {
        let dir = TempDir::new().unwrap();
        let mut ctx = make_ctx(&dir);
        let llm = Arc::new(MockLlmProvider::new(
            "{\"entities\":[],\"relationships\":[]}",
        ));
        let mut pass = SqlAnalyzerPass::new(
            "s.sql",
            "CREATE TABLE a (id INT);",
            llm,
            &GenericDialectParser,
        )
        .with_max_tokens(Some(123));
        assert_eq!(pass.max_tokens_override, Some(123));
        pass.run(&mut ctx).await.unwrap();
    }

    #[tokio::test]
    async fn partial_enrichment_is_reported_instead_of_passing_silently() {
        // The real LedgerSMB failure: the model named a fraction of the schema and nothing said
        // so, which is indistinguishable from a schema that simply has no descriptions.
        let dir = TempDir::new().unwrap();
        let mut ctx = make_ctx(&dir);
        let llm = Arc::new(MockLlmProvider::new(
            r#"{"entities":[{"table":"a","entity_name":"A","type":"core","entity_type":"core","description":"d"}],"relationships":[]}"#,
        ));
        let mut pass = SqlAnalyzerPass::new(
            "s.sql",
            "CREATE TABLE a (id INT);\nCREATE TABLE b (id INT);\nCREATE TABLE c (id INT);",
            llm,
            &GenericDialectParser,
        );
        pass.run(&mut ctx).await.unwrap();

        let diags = ctx.diagnostics.lock().unwrap();
        let rendered = format!("{:?}", diags);
        assert!(
            rendered.contains("SQL004"),
            "partial coverage must raise SQL004, got: {rendered}"
        );
        assert!(
            rendered.contains("1 of 3"),
            "the diagnostic must say how short it fell, got: {rendered}"
        );
    }

    #[tokio::test]
    async fn full_enrichment_raises_no_coverage_warning() {
        let dir = TempDir::new().unwrap();
        let mut ctx = make_ctx(&dir);
        let llm = Arc::new(MockLlmProvider::new(
            r#"{"entities":[{"table":"a","entity_name":"A","type":"core","entity_type":"core","description":"d"}],"relationships":[]}"#,
        ));
        let mut pass = SqlAnalyzerPass::new(
            "s.sql",
            "CREATE TABLE a (id INT);",
            llm,
            &GenericDialectParser,
        );
        pass.run(&mut ctx).await.unwrap();

        let rendered = format!("{:?}", ctx.diagnostics.lock().unwrap());
        assert!(
            !rendered.contains("SQL004"),
            "complete coverage must stay quiet, got: {rendered}"
        );
    }

    #[test]
    fn enrichment_count_ignores_tables_the_model_invented() {
        // A hallucinated table must not make coverage look complete.
        let mut graph = graph_with_comments("CREATE TABLE a (id INT);");
        let json = r#"{"entities":[
            {"table":"a","entity_name":"A","type":"core","entity_type":"core","description":"d"},
            {"table":"ghost","entity_name":"Ghost","type":"core","entity_type":"core","description":"d"}
        ],"relationships":[]}"#;
        assert_eq!(
            apply_llm_enrichment(&mut graph, json).unwrap(),
            1,
            "only the real table counts toward coverage"
        );
    }

    // ── RFC 0146 Phase 2: COMMENT ON becomes an evidence-backed description ────────────────

    /// Builds a graph from `sql` and applies its `COMMENT ON` statements the way
    /// `SqlAnalyzerPass::run` does, without needing an LLM or a `PassContext`.
    fn graph_with_comments(sql: &str) -> KirGraph {
        let comments = crate::sql_comments::extract_sql_comments(sql);
        let dialect = GenericDialect {};
        let mut graph = parse_ddl_structural(sql, "schema.sql", &dialect);
        apply_sql_comments(&mut graph, &comments, "schema.sql");
        graph
    }

    fn table<'a>(graph: &'a KirGraph, name: &str) -> &'a KirObject {
        graph
            .objects
            .iter()
            .find(|o| o.name.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| panic!("no table {name} in graph"))
    }

    #[test]
    fn table_comment_becomes_the_description() {
        let graph = graph_with_comments(
            "CREATE TABLE account (id INT);\nCOMMENT ON TABLE account IS 'The chart of accounts.';",
        );
        let t = table(&graph, "account");
        assert_eq!(
            t.properties.get("description").and_then(|v| v.as_str()),
            Some("The chart of accounts.")
        );
        assert_eq!(
            t.properties
                .get(SQL_COMMENT_PROPERTY)
                .and_then(|v| v.as_str()),
            Some("The chart of accounts."),
            "provenance must be recorded separately from the description itself"
        );
    }

    #[test]
    fn table_comment_carries_evidence_with_a_real_line_number() {
        let graph = graph_with_comments(
            "CREATE TABLE account (id INT);\n\nCOMMENT ON TABLE account IS 'desc';",
        );
        let t = table(&graph, "account");
        // One evidence record for CREATE TABLE, one for the COMMENT ON.
        assert_eq!(t.evidence.len(), 2, "comment must add its own evidence");
        let ev = graph
            .evidence
            .iter()
            .find(|e| e.fragment.starts_with("COMMENT ON TABLE"))
            .expect("comment evidence missing");
        assert_eq!(ev.location.line, Some(3));
        assert_eq!(ev.location.path, "schema.sql");
    }

    #[test]
    fn column_comment_lands_on_the_matching_column() {
        let graph = graph_with_comments(
            "CREATE TABLE language (code VARCHAR(6), description TEXT);\n\
             COMMENT ON COLUMN language.code IS 'ISO 639 code.';",
        );
        let cols = table(&graph, "language")
            .properties
            .get("columns")
            .and_then(|v| v.as_array())
            .expect("columns property");
        let code = cols
            .iter()
            .find(|c| c.get("name").and_then(|n| n.as_str()) == Some("code"))
            .unwrap();
        assert_eq!(
            code.get("description").and_then(|v| v.as_str()),
            Some("ISO 639 code.")
        );
        let other = cols
            .iter()
            .find(|c| c.get("name").and_then(|n| n.as_str()) == Some("description"))
            .unwrap();
        assert!(
            other.get("description").is_none(),
            "only the named column may be described"
        );
    }

    #[test]
    fn schema_qualified_comment_matches_the_bare_table() {
        let graph = graph_with_comments(
            "CREATE TABLE account (id INT);\nCOMMENT ON TABLE public.account IS 'desc';",
        );
        assert_eq!(
            table(&graph, "account")
                .properties
                .get("description")
                .and_then(|v| v.as_str()),
            Some("desc")
        );
    }

    #[test]
    fn a_comment_on_an_undeclared_table_is_dropped_not_invented() {
        let graph = graph_with_comments(
            "CREATE TABLE account (id INT);\nCOMMENT ON TABLE nowhere IS 'desc';",
        );
        assert_eq!(
            graph.objects.len(),
            1,
            "no object may be created by a comment"
        );
        assert!(
            !table(&graph, "account")
                .properties
                .contains_key("description")
        );
    }

    #[test]
    fn a_comment_on_an_undeclared_column_is_dropped() {
        let graph = graph_with_comments(
            "CREATE TABLE account (id INT);\nCOMMENT ON COLUMN account.missing IS 'desc';",
        );
        let t = table(&graph, "account");
        assert_eq!(
            t.evidence.len(),
            1,
            "no evidence for a column that does not exist"
        );
    }

    /// The point of Phase 2: a human-written description must outrank a generated one.
    #[test]
    fn llm_enrichment_does_not_overwrite_an_author_written_description() {
        let mut graph = graph_with_comments(
            "CREATE TABLE account (id INT);\nCOMMENT ON TABLE account IS 'Authoritative text.';",
        );
        let llm_json = r#"{"entities":[{"table":"account","entity_name":"Account","type":"core","entity_type":"core","description":"A generated guess."}],"relationships":[]}"#;
        apply_llm_enrichment(&mut graph, llm_json).unwrap();

        let t = table(&graph, "account");
        assert_eq!(
            t.properties.get("description").and_then(|v| v.as_str()),
            Some("Authoritative text."),
            "the schema's own words must win"
        );
        assert_eq!(
            t.properties.get("llm_description").and_then(|v| v.as_str()),
            Some("A generated guess."),
            "the model's version is kept for comparison, not discarded"
        );
        assert_eq!(
            t.properties.get("entity_name").and_then(|v| v.as_str()),
            Some("Account"),
            "the model still contributes what the schema does not state"
        );
    }

    #[test]
    fn llm_description_is_used_when_the_schema_says_nothing() {
        let mut graph = graph_with_comments("CREATE TABLE account (id INT);");
        let llm_json = r#"{"entities":[{"table":"account","entity_name":"Account","type":"core","entity_type":"core","description":"A generated guess."}],"relationships":[]}"#;
        apply_llm_enrichment(&mut graph, llm_json).unwrap();
        assert_eq!(
            table(&graph, "account")
                .properties
                .get("description")
                .and_then(|v| v.as_str()),
            Some("A generated guess."),
            "with no COMMENT ON, the model's description is the only one available"
        );
    }

    #[test]
    fn structural_parse_extracts_six_tables() {
        let graph = parse_ddl_structural(ECOMMERCE_SQL, "ecommerce.sql", &GenericDialect {});
        assert_eq!(
            graph.objects.len(),
            6,
            "ecommerce schema has 6 tables: categories, customers, products, orders, order_items, payments"
        );
    }

    #[test]
    fn table_ids_are_deterministic_across_two_independent_parses() {
        // RFC 0076: found live on a real project re-recovered months after its first `ekos
        // recover` run — every table existed twice in the ledger because `Table` objects got a
        // fresh random id every parse. Two fully independent `parse_ddl_structural` calls over
        // the same DDL (simulating two separate `recover` invocations) must agree on every id.
        let run1 = parse_ddl_structural(ECOMMERCE_SQL, "ecommerce.sql", &GenericDialect {});
        let run2 = parse_ddl_structural(ECOMMERCE_SQL, "ecommerce.sql", &GenericDialect {});

        let mut ids1: Vec<String> = run1.objects.iter().map(|o| o.id.to_string()).collect();
        let mut ids2: Vec<String> = run2.objects.iter().map(|o| o.id.to_string()).collect();
        ids1.sort();
        ids2.sort();
        assert_eq!(ids1, ids2);

        let mut rel_ids1: Vec<String> = run1
            .relationships
            .iter()
            .map(|r| r.id.to_string())
            .collect();
        let mut rel_ids2: Vec<String> = run2
            .relationships
            .iter()
            .map(|r| r.id.to_string())
            .collect();
        rel_ids1.sort();
        rel_ids2.sort();
        assert_eq!(rel_ids1, rel_ids2);
    }

    #[test]
    fn table_id_is_case_insensitive_matching_the_internal_fk_lookup_convention() {
        let a = table_kir_id("Users");
        let b = table_kir_id("users");
        let c = table_kir_id("USERS");
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn two_foreign_keys_to_the_same_target_table_via_different_columns_get_distinct_ids() {
        // RFC 0072's own counter-example, now guarded directly against `sql_analyzer.rs`'s real
        // id: a table with two FK columns to the same target table is two real, distinct edges —
        // `(from, to)` alone would collide them, so `fk_desc` must be part of the id input.
        let sql = "CREATE TABLE customers (id INT PRIMARY KEY);\n\
                   CREATE TABLE shipments (\n\
                       id INT PRIMARY KEY,\n\
                       sender_id INT REFERENCES customers(id),\n\
                       receiver_id INT REFERENCES customers(id)\n\
                   );";
        let graph = parse_ddl_structural(sql, "shipments.sql", &GenericDialect {});
        assert_eq!(
            graph.relationships.len(),
            2,
            "two distinct FK columns to customers"
        );
        assert_ne!(
            graph.relationships[0].id, graph.relationships[1].id,
            "two real, distinct FK edges must not collapse onto one id"
        );
    }

    #[test]
    fn structural_parse_extracts_fk_relationships() {
        let graph = parse_ddl_structural(ECOMMERCE_SQL, "ecommerce.sql", &GenericDialect {});
        // orders→customers, order_items→orders, order_items→products, payments→orders,
        // products→categories, categories→categories (self-ref)
        assert!(
            graph.relationships.len() >= 5,
            "expected ≥5 FK relationships, got {}",
            graph.relationships.len()
        );
    }

    #[test]
    fn northwind_structural_parse_extracts_thirteen_tables() {
        let graph = parse_ddl_structural(NORTHWIND_SQL, "northwind.sql", &GenericDialect {});
        assert_eq!(
            graph.objects.len(),
            13,
            "northwind schema has 13 tables (Employees, Categories, Customers, Shippers, \
             Suppliers, Orders, Products, Order Details, Region, Territories, \
             EmployeeTerritories, CustomerDemographics, CustomerCustomerDemo)"
        );
    }

    #[test]
    fn northwind_structural_parse_extracts_deep_fk_graph() {
        let graph = parse_ddl_structural(NORTHWIND_SQL, "northwind.sql", &GenericDialect {});
        // A much deeper FK graph than ecommerce.sql's — real Northwind has 14 FK edges
        // across its 13 tables (including Employees' self-referential ReportsTo).
        assert!(
            graph.relationships.len() >= 12,
            "expected a deep FK graph (>=12 edges), got {}",
            graph.relationships.len()
        );
    }

    #[test]
    fn northwind_structural_parse_finds_order_details_composite_pk_table() {
        let graph = parse_ddl_structural(NORTHWIND_SQL, "northwind.sql", &GenericDialect {});
        // sqlparser's ObjectName::to_string() preserves the original quote style, so a
        // double-quoted identifier's name retains literal `"` characters.
        let order_details = graph
            .objects
            .iter()
            .find(|o| o.name.to_lowercase().replace('"', "") == "order details");
        assert!(
            order_details.is_some(),
            "Order Details table (quoted, contains a space) must parse"
        );
    }

    #[test]
    fn structural_parse_table_has_columns() {
        let graph = parse_ddl_structural(ECOMMERCE_SQL, "ecommerce.sql", &GenericDialect {});
        let customers = graph
            .objects
            .iter()
            .find(|o| o.name.to_lowercase() == "customers");
        assert!(customers.is_some());
        let cols = &customers.unwrap().properties["columns"];
        assert!(cols.is_array());
        assert!(!cols.as_array().unwrap().is_empty());
    }

    /// RFC 0031 regression: this exact file, parsed with `GenericDialect` (pre-RFC-0031
    /// behavior for every `.sql` file, regardless of content), fails outright — 0 tables
    /// recovered, matching devlog_31's documented 0%-mapped finding.
    #[test]
    fn generic_dialect_fails_on_real_mysql_hash_comments_fixture() {
        let graph = parse_ddl_structural(
            MYSQL_HASH_COMMENTS_SQL,
            "mysql_hash_comments.sql",
            &GenericDialect {},
        );
        assert!(
            graph.objects.is_empty(),
            "GenericDialect is expected to fail on '#' comments, recovering 0 tables"
        );
    }

    /// The fix: selecting `MySqlDialect` (via the registry, exercised here directly) parses
    /// the same real file successfully and recovers its tables.
    #[test]
    fn mysql_dialect_parses_real_mysql_hash_comments_fixture() {
        use ekos_plugin_sql_dialect_mysql::MySqlDialectParser;

        let dialect = MySqlDialectParser.sqlparser_dialect();
        let graph = parse_ddl_structural(
            MYSQL_HASH_COMMENTS_SQL,
            "mysql_hash_comments.sql",
            &*dialect,
        );
        assert!(
            !graph.objects.is_empty(),
            "MySqlDialect must recover tables from a real MySQL DDL file with '#' comments"
        );
        let names: Vec<String> = graph
            .objects
            .iter()
            .map(|o| o.name.to_lowercase())
            .collect();
        assert!(
            names.contains(&"fact_sales".to_string()),
            "expected fact_sales among recovered tables, got {names:?}"
        );
        assert!(
            names.contains(&"dim_date".to_string()),
            "expected dim_date among recovered tables, got {names:?}"
        );
    }

    /// GitHub issue #3's second root cause: a DDL script with multiple `CREATE TABLE`
    /// statements and no `;` separating them fails to parse at all (not just the second
    /// statement) — `parse_ddl_structural`'s fallback (`statement_repair`) recovers both tables.
    #[test]
    fn recovers_tables_from_ddl_script_missing_semicolons_between_statements() {
        let sql = "\
CREATE TABLE customers (id INT PRIMARY KEY, name VARCHAR(100))

CREATE TABLE orders (id INT PRIMARY KEY, customer_id INT REFERENCES customers(id))
";
        let graph = parse_ddl_structural(sql, "no_semicolons.sql", &GenericDialect {});
        let names: Vec<String> = graph.objects.iter().map(|o| o.name.clone()).collect();
        assert!(
            names.contains(&"customers".to_string()) && names.contains(&"orders".to_string()),
            "expected both tables recovered despite missing statement separators, got {names:?}"
        );
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
        let mut pass =
            SqlAnalyzerPass::new("ecommerce.sql", ECOMMERCE_SQL, mock, &GenericDialectParser);
        let mut ctx = make_ctx(&dir);
        pass.run(&mut ctx).await.unwrap();
        assert!(
            !ctx.diagnostics.lock().unwrap().has_errors(),
            "no errors expected with mock llm"
        );
    }

    #[tokio::test]
    async fn pass_tolerates_bad_llm_json() {
        let dir = TempDir::new().unwrap();
        let mock = Arc::new(MockLlmProvider::new("not valid json at all!!"));
        let mut pass =
            SqlAnalyzerPass::new("ecommerce.sql", ECOMMERCE_SQL, mock, &GenericDialectParser);
        let mut ctx = make_ctx(&dir);
        // Should not return an error — bad LLM response degrades to structural-only.
        pass.run(&mut ctx).await.unwrap();
        assert!(!ctx.diagnostics.lock().unwrap().has_errors());
    }

    #[test]
    fn llm_enrichment_applies_entity_names() {
        let mut graph = parse_ddl_structural(ECOMMERCE_SQL, "ecommerce.sql", &GenericDialect {});
        let llm_json = serde_json::json!({
            "entities": [
                {"table": "customers", "entity_name": "Customer", "type": "core", "description": "desc"}
            ],
            "relationships": []
        })
        .to_string();
        apply_llm_enrichment(&mut graph, &llm_json).unwrap();
        let customers = graph
            .objects
            .iter()
            .find(|o| o.name.to_lowercase() == "customers")
            .unwrap();
        assert_eq!(customers.properties["entity_name"], "Customer");
    }
}
