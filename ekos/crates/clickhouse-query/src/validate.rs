//! SELECT-only validation gate (RFC 0056 Stage 2, "run query" step) — the hard invariant that
//! keeps a live ClickHouse query engine from ever writing. An LLM-generated query is parsed with
//! `ClickHouseDialectParser`'s dialect and rejected unless it is exactly one
//! `sqlparser::ast::Statement::Query` — no `INSERT`/`UPDATE`/`DELETE`/`ALTER`/`DROP`/`SYSTEM`/
//! `KILL`, no multi-statement batches. A `LIMIT` is injected if the query doesn't already have
//! one, so this is the one path that touches a live enterprise system and it can never become a
//! data-export pipe either.

use ekos_sql_dialect_sdk::SqlDialectParser;
use sqlparser::ast::{Expr, Statement, Value};
use sqlparser::parser::Parser;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValidateError {
    #[error("cannot parse generated SQL as ClickHouse: {0}")]
    Parse(String),
    #[error("expected exactly one SQL statement, got {0}")]
    NotSingleStatement(usize),
    #[error("only SELECT statements are permitted, got: {0}")]
    NotASelect(String),
}

/// Parses `sql`, rejects anything but a single `SELECT`, and returns the statement re-rendered
/// with a `LIMIT max_rows` injected if none was already present.
pub fn validate_select_only(sql: &str, max_rows: usize) -> Result<String, ValidateError> {
    let dialect = ekos_plugin_sql_dialect_clickhouse::ClickHouseDialectParser.sqlparser_dialect();
    let statements =
        Parser::parse_sql(&*dialect, sql).map_err(|e| ValidateError::Parse(e.to_string()))?;

    if statements.len() != 1 {
        return Err(ValidateError::NotSingleStatement(statements.len()));
    }

    let stmt = statements.into_iter().next().expect("checked len == 1");
    let Statement::Query(mut query) = stmt else {
        return Err(ValidateError::NotASelect(stmt.to_string()));
    };

    if query.limit.is_none() {
        query.limit = Some(Expr::Value(Value::Number(max_rows.to_string(), false)));
    }

    Ok(query.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_select_passes_with_limit_injected() {
        let sql = validate_select_only("SELECT id, name FROM orders", 100).unwrap();
        assert!(sql.contains("LIMIT 100"), "sql was: {sql}");
    }

    #[test]
    fn a_select_with_its_own_limit_is_left_alone() {
        let sql = validate_select_only("SELECT id FROM orders LIMIT 10", 1000).unwrap();
        assert!(sql.contains("LIMIT 10"));
        assert!(!sql.contains("LIMIT 1000"));
    }

    #[test]
    fn insert_is_rejected() {
        let err = validate_select_only("INSERT INTO orders VALUES (1)", 100).unwrap_err();
        assert!(matches!(err, ValidateError::NotASelect(_)));
    }

    #[test]
    fn update_is_rejected() {
        let err = validate_select_only("UPDATE orders SET id = 1", 100).unwrap_err();
        assert!(matches!(err, ValidateError::NotASelect(_)));
    }

    #[test]
    fn delete_is_rejected() {
        let err = validate_select_only("DELETE FROM orders", 100).unwrap_err();
        assert!(matches!(err, ValidateError::NotASelect(_)));
    }

    #[test]
    fn drop_table_is_rejected() {
        let err = validate_select_only("DROP TABLE orders", 100).unwrap_err();
        assert!(matches!(err, ValidateError::NotASelect(_)));
    }

    #[test]
    fn multi_statement_batch_is_rejected() {
        let err = validate_select_only("SELECT 1; DROP TABLE orders", 100).unwrap_err();
        assert!(matches!(err, ValidateError::NotSingleStatement(2)));
    }

    #[test]
    fn unparseable_sql_is_rejected() {
        let err = validate_select_only("not even sql", 100).unwrap_err();
        assert!(matches!(err, ValidateError::Parse(_)));
    }
}
