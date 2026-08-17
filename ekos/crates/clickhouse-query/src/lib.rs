//! Live NL-to-SQL query engine over a compiled ClickHouse schema (RFC 0056, Stage 2).
//!
//! This is the one path in EKOS that crosses the "AI systems consume knowledge through the
//! Runtime only... they never touch raw enterprise systems directly" invariant stated in
//! `CLAUDE.md`/`README.md` — deliberately, scoped, and audited, not silently. Six stages, mapping
//! directly onto the flow requested in RFC 0056:
//!
//! 1. **Source question** — a plain string, from `ekos clickhouse ask` or the gated
//!    `ekos_clickhouse_query` MCP tool.
//! 2. **Analyze context / 3. Analyze metadata** — [`schema::gather_clickhouse_schema`] reads the
//!    *compiled* ClickHouse tables straight from the ledger via [`ekos_runtime::Runtime`] — no
//!    live schema fetch, so this stays fast and offline-capable at the cost of staleness until
//!    the next `ekos build`/`ekos recover`.
//! 4. **Build query** — one [`ekos_recovery::llm::LlmProvider::complete`] call, schema-grounded.
//! 5. **Run query** — [`validate::validate_select_only`] hard-rejects anything but a single
//!    `SELECT` before [`client::ClickHouseQueryClient::run_select`] ever executes it live.
//! 6. **Return dataset** — [`ekos_common::redaction::redact_json`] scrubs every returned row
//!    before it reaches the caller; [`audit::record_query_event`] appends the fact that the query
//!    ran (not the row data) to the ledger as Evidence/Event.
//!
//! Stage 7 ("enrich with EKOS-loaded data") is intentionally left to the caller for v1 — a CLI/
//! MCP surface can look up `ClickHouseAnswer::columns` against compiled KIR objects itself; see
//! RFC 0056's Non-goals for why this crate doesn't build a full join engine.

pub mod audit;
pub mod client;
pub mod schema;
pub mod validate;

use std::sync::Arc;

use ekos_common::redaction::RedactionConfig;
use ekos_kir::KirId;
use ekos_ledger::{KnowledgeStore, LedgerError};
use ekos_recovery::llm::{LlmError, LlmProvider, LlmRequest};
use ekos_runtime::{Runtime, RuntimeError};
use thiserror::Error;

pub use client::{
    ClickHouseHttpQueryClient, ClickHouseQueryClient, MockClickHouseQueryClient, QueryResult,
};
pub use schema::TableSchema;
pub use validate::ValidateError;

const SYSTEM_PROMPT: &str = r#"You are a ClickHouse SQL query generator for EKOS. You are given a
compiled schema (tables, columns, engine, sort key) and a natural-language question. Respond with
exactly one ClickHouse SELECT statement that answers the question, referencing only the tables and
columns listed in the schema. Do not include any explanation, markdown fences, or SQL other than
the single SELECT statement."#;
const PROMPT_VERSION: &str = "clickhouse-query-v1";
const DEFAULT_MAX_TOKENS: u32 = 512;

#[derive(Debug, Error)]
pub enum ClickHouseQueryError {
    #[error(
        "no ClickHouse tables compiled into the ledger yet — run `ekos build && ekos recover` first"
    )]
    NoCompiledSchema,
    #[error("runtime error: {0}")]
    Runtime(#[from] RuntimeError),
    #[error("llm error: {0}")]
    Llm(#[from] LlmError),
    #[error("generated SQL rejected: {0}")]
    Validation(#[from] ValidateError),
    #[error("query execution failed: {0}")]
    Execution(#[from] client::ClickHouseQueryClientError),
    #[error("ledger error: {0}")]
    Ledger(#[from] LedgerError),
}

/// Result of one end-to-end `ask_clickhouse` call.
#[derive(Debug, Clone)]
pub struct ClickHouseAnswer {
    pub sql: String,
    pub columns: Vec<String>,
    pub rows: Vec<serde_json::Value>,
    pub row_count: usize,
    pub summary: String,
    /// The ledger Event id recording that this query ran (RFC 0056 auditability).
    pub audit_event_id: KirId,
}

/// Strips markdown code fences (` ```sql ` or bare ` ``` `) and a trailing `;` from an LLM
/// response — models routinely wrap generated SQL in fences even when asked not to.
fn strip_sql_fences(s: &str) -> String {
    s.trim()
        .trim_start_matches("```sql")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_string()
}

/// Runs the full six-stage pipeline: question -> compiled schema context -> LLM-built SQL ->
/// SELECT-only validation -> live execution -> redaction -> audited dataset.
#[allow(clippy::too_many_arguments)]
pub async fn ask_clickhouse(
    runtime: &Runtime<'_>,
    store: &dyn KnowledgeStore,
    llm: Arc<dyn LlmProvider>,
    query_client: Arc<dyn ClickHouseQueryClient>,
    database: &str,
    max_rows: usize,
    redaction_config: &RedactionConfig,
    question: &str,
) -> Result<ClickHouseAnswer, ClickHouseQueryError> {
    let tables = schema::gather_clickhouse_schema(runtime)?;
    if tables.is_empty() {
        return Err(ClickHouseQueryError::NoCompiledSchema);
    }
    let schema_prompt = schema::render_schema_prompt(&tables);

    let user = format!("ClickHouse schema:\n{schema_prompt}\nQuestion: {question}");
    let req = LlmRequest {
        system: SYSTEM_PROMPT,
        user: &user,
        prompt_version: PROMPT_VERSION,
        max_tokens: DEFAULT_MAX_TOKENS,
    };
    let resp = llm.complete(&req).await?;
    let raw_sql = strip_sql_fences(&resp.content);

    let validated_sql = validate::validate_select_only(&raw_sql, max_rows)?;

    let result = query_client.run_select(&validated_sql).await?;
    let mut rows = result.rows;
    for row in &mut rows {
        ekos_common::redaction::redact_json(row, redaction_config);
    }

    let audit_event_id =
        audit::record_query_event(store, database, question, &validated_sql, &rows)?;

    let row_count = rows.len();
    let summary = format!("{row_count} row(s) returned for: {validated_sql}");

    Ok(ClickHouseAnswer {
        sql: validated_sql,
        columns: result.columns,
        rows,
        row_count,
        summary,
        audit_event_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirObject, ObjectKind};
    use ekos_ledger::Ledger;
    use ekos_recovery::MockLlmProvider;
    use tempfile::TempDir;

    fn temp_ledger_with_orders_table() -> (Ledger, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ledger.db");
        let ledger = Ledger::open(&path).unwrap();

        let mut table = KirObject::new("analytics.orders", ObjectKind::Table);
        table.properties.insert(
            "columns".into(),
            serde_json::json!([{"name": "id", "data_type": "UInt64"}]),
        );
        table
            .properties
            .insert("engine".into(), serde_json::json!("MergeTree"));
        table
            .properties
            .insert("order_by".into(), serde_json::json!(["id"]));
        table
            .properties
            .insert("source_system".into(), serde_json::json!("clickhouse"));
        ledger.append_object(&table).unwrap();

        (ledger, dir)
    }

    #[tokio::test]
    async fn full_pipeline_returns_redacted_audited_dataset() {
        let (ledger, _dir) = temp_ledger_with_orders_table();
        let runtime = Runtime::new(&ledger);
        let llm = Arc::new(MockLlmProvider::new("SELECT id FROM orders"));
        let query_client = Arc::new(MockClickHouseQueryClient::new(QueryResult {
            columns: vec!["id".to_string()],
            rows: vec![serde_json::json!({"id": 1}), serde_json::json!({"id": 2})],
        }));

        let answer = ask_clickhouse(
            &runtime,
            &ledger,
            llm,
            query_client,
            "analytics",
            1000,
            &RedactionConfig::default(),
            "how many orders are there?",
        )
        .await
        .unwrap();

        assert_eq!(answer.row_count, 2);
        assert!(answer.sql.contains("LIMIT 1000"));
        assert!(ledger.get_event(&answer.audit_event_id).unwrap().is_some());
    }

    #[tokio::test]
    async fn no_compiled_schema_is_a_named_error_not_a_panic() {
        let dir = TempDir::new().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let runtime = Runtime::new(&ledger);
        let llm = Arc::new(MockLlmProvider::new("SELECT 1"));
        let query_client = Arc::new(MockClickHouseQueryClient::new(QueryResult::default()));

        let err = ask_clickhouse(
            &runtime,
            &ledger,
            llm,
            query_client,
            "analytics",
            1000,
            &RedactionConfig::default(),
            "anything?",
        )
        .await
        .unwrap_err();

        assert!(matches!(err, ClickHouseQueryError::NoCompiledSchema));
    }

    #[tokio::test]
    async fn llm_generated_write_statement_is_rejected_before_execution() {
        let (ledger, _dir) = temp_ledger_with_orders_table();
        let runtime = Runtime::new(&ledger);
        let llm = Arc::new(MockLlmProvider::new("DROP TABLE orders"));
        let query_client = Arc::new(MockClickHouseQueryClient::new(QueryResult::default()));

        let err = ask_clickhouse(
            &runtime,
            &ledger,
            llm,
            query_client,
            "analytics",
            1000,
            &RedactionConfig::default(),
            "delete everything",
        )
        .await
        .unwrap_err();

        assert!(matches!(err, ClickHouseQueryError::Validation(_)));
    }

    #[test]
    fn strip_sql_fences_handles_fenced_and_bare_and_trailing_semicolon() {
        assert_eq!(strip_sql_fences("SELECT 1"), "SELECT 1");
        assert_eq!(strip_sql_fences("SELECT 1;"), "SELECT 1");
        assert_eq!(strip_sql_fences("```sql\nSELECT 1;\n```"), "SELECT 1");
        assert_eq!(strip_sql_fences("```\nSELECT 1\n```"), "SELECT 1");
    }
}
