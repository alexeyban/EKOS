//! Text-to-SQL checks and a tool-using agent over what EKOS knows about a schema (RFC 0242).
//!
//! The crate is dialect-generic and holds no configuration, credentials or provider
//! construction — callers (the BIRD bench, `ekos sql check`, the MCP tool) build those.
//!
//! - [`schema`]: the [`Schema`] a check knows — tables, columns, keys, foreign keys, and when a
//!   profile exists, per-column facts (nulls, distinct, examples, list-valued, sentinels).
//! - [`analyze`]: one walk over a statement's AST that resolves column references through
//!   aliases and scopes and records what the checks need.
//! - [`lint`]: static findings (`L001`–`L008`), gold-free.
//! - [`probe`]: findings that run cheap queries through a [`probe::Probe`] (`P001`–`P005`).
//! - [`agent`]: a tool loop over `LlmProvider::complete`, with checks on every submission.

pub mod agent;
pub mod analyze;
pub mod lint;
pub mod probe;
pub mod schema;

/// The single-SELECT guard and row wrapping (RFC 0243), shared with the database plugins.
pub use ekos_sql_dialect_sdk::guard;

pub use lint::{Fetch, Finding, Severity};
pub use schema::{Column, ColumnStats, ForeignKey, Schema, Table};

/// The SQL a model answered with: the last ```sql block, else the last code block, else the
/// text from the first `SELECT`/`WITH`. Trailing semicolons are dropped.
pub fn extract_sql(answer: &str) -> Option<String> {
    let blocks: Vec<&str> = answer.split("```").collect();
    let mut best: Option<String> = None;
    for (i, b) in blocks.iter().enumerate() {
        if i % 2 == 1 {
            if b.starts_with("tool") {
                continue;
            }
            let body = b
                .strip_prefix("sqlite")
                .or_else(|| b.strip_prefix("sql"))
                .or_else(|| b.strip_prefix("SQL"))
                .unwrap_or(b);
            if !body.trim().is_empty() {
                best = Some(body.to_string());
            }
        }
    }
    let sql = match best {
        Some(b) => b,
        // A reply of tool calls holds no answer, even when a call carries SQL.
        None if answer.contains("```tool") => return None,
        None => {
            let upper = answer.to_uppercase();
            let start = ["SELECT", "WITH"]
                .iter()
                .filter_map(|k| upper.find(k))
                .min()?;
            answer[start..].to_string()
        }
    };
    let sql = strip_comment_lines(&sql);
    let sql = sql.trim().trim_end_matches(';').trim().to_string();
    (!sql.is_empty()).then_some(sql)
}

fn strip_comment_lines(sql: &str) -> String {
    sql.lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `name` as a double-quoted identifier — accepted by SQLite, PostgreSQL, Trino and Oracle.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// `s` as a single-quoted SQL string literal.
pub fn quote_literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
