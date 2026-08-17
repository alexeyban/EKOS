//! `ekos clickhouse ask` — live NL-to-SQL query engine over a compiled ClickHouse schema
//! (RFC 0056, Stage 2). Always available from the CLI regardless of the
//! `[clickhouse].enable-mcp-query` config flag — that flag only gates the MCP tool
//! (`commands/mcp.rs`), which is a materially different risk surface (any connected AI agent,
//! not a human operator explicitly invoking a command).

use super::recover::build_llm_provider;
use super::store::open_store;
use anyhow::Result;
use ekos_clickhouse_query::{ClickHouseAnswer, ClickHouseHttpQueryClient, ask_clickhouse};
use ekos_compiler_core::EkosConfig;
use ekos_runtime::Runtime;
use std::path::Path;
use std::sync::Arc;

/// Same env var naming convention `build.rs` uses for the Observer connector (RFC 0056) —
/// kept as its own small constant set here rather than shared, since the two call sites (compile-
/// time schema scan vs. request-time live query) construct different client types.
pub(crate) const CLICKHOUSE_URL_ENV: &str = "EKOS_CLICKHOUSE_URL";
pub(crate) const CLICKHOUSE_DATABASE_ENV: &str = "EKOS_CLICKHOUSE_DATABASE";
pub(crate) const CLICKHOUSE_USER_ENV: &str = "EKOS_CLICKHOUSE_USER";
pub(crate) const CLICKHOUSE_PASSWORD_ENV: &str = "EKOS_CLICKHOUSE_PASSWORD";
const DEFAULT_MAX_ROWS: usize = 1000;

/// Shared by `ekos clickhouse ask` (this module) and the gated `ekos_clickhouse_query` MCP tool
/// (`mcp.rs`) — both entry points run the identical six-stage pipeline; only how the result is
/// surfaced (stdout vs. an MCP tool result) differs.
pub(crate) async fn run_query(
    config: &EkosConfig,
    cwd: &Path,
    question: &str,
) -> Result<ClickHouseAnswer> {
    let url = std::env::var(CLICKHOUSE_URL_ENV).map_err(|_| {
        anyhow::anyhow!("{CLICKHOUSE_URL_ENV} not set — cannot run a live ClickHouse query")
    })?;
    let database = std::env::var(CLICKHOUSE_DATABASE_ENV).map_err(|_| {
        anyhow::anyhow!("{CLICKHOUSE_DATABASE_ENV} not set — cannot run a live ClickHouse query")
    })?;
    let user = std::env::var(CLICKHOUSE_USER_ENV).unwrap_or_default();
    let password = std::env::var(CLICKHOUSE_PASSWORD_ENV).unwrap_or_default();

    let artifact_dir = config.artifact_dir(cwd);
    let llm = build_llm_provider(config, &artifact_dir);
    let query_client = Arc::new(ClickHouseHttpQueryClient::new(url, user, password));
    let redaction_config = config.redaction_config();

    let ledger = open_store(config, cwd)?;
    let runtime = Runtime::over(&*ledger);

    let answer = ask_clickhouse(
        &runtime,
        &*ledger,
        llm,
        query_client,
        &database,
        DEFAULT_MAX_ROWS,
        &redaction_config,
        question,
    )
    .await?;

    Ok(answer)
}

pub async fn ask(config: &EkosConfig, cwd: &Path, question: &str, json: bool) -> Result<()> {
    let answer = run_query(config, cwd, question).await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "sql": answer.sql,
                "columns": answer.columns,
                "rows": answer.rows,
                "row_count": answer.row_count,
                "summary": answer.summary,
                "audit_event_id": answer.audit_event_id.to_string(),
            }))?
        );
        return Ok(());
    }

    println!("SQL: {}", answer.sql);
    println!("{}", answer.summary);
    for row in &answer.rows {
        println!("  {row}");
    }

    Ok(())
}
