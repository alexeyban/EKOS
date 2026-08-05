//! PostgreSQL `SqlDialectParser` (RFC 0031).
//!
//! Wraps `sqlparser::dialect::PostgreSqlDialect`. Previously only used inside
//! `sql_transform_analyzer.rs`'s private `dialect_for` for `SELECT`/`CREATE VIEW`/`CREATE
//! PROCEDURE`/`CREATE FUNCTION` recovery — this crate makes it independently selectable and
//! testable, and — new — lets `SqlAnalyzerPass` (DDL/`CREATE TABLE` recovery) use it too, which
//! previously had no dialect awareness at all and always parsed with `GenericDialect`.
//!
//! No preprocessing is needed for Postgres: unlike MySQL's `DELIMITER` convention, Postgres
//! function bodies use `$$ ... $$` dollar-quoting, which is real Postgres grammar `sqlparser`
//! already tokenizes as a `DollarQuotedString` — nothing needs stripping before parsing.

use ekos_sql_dialect_sdk::SqlDialectParser;
use sqlparser::dialect::{Dialect, PostgreSqlDialect};

pub struct PostgresDialectParser;

impl SqlDialectParser for PostgresDialectParser {
    fn name(&self) -> &str {
        "postgres"
    }

    fn sqlparser_dialect(&self) -> Box<dyn Dialect + Send + Sync> {
        Box::new(PostgreSqlDialect {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::parser::Parser;

    #[test]
    fn name_is_postgres() {
        assert_eq!(PostgresDialectParser.name(), "postgres");
    }

    #[test]
    fn preprocess_is_identity() {
        let sql = "CREATE TABLE t (id SERIAL PRIMARY KEY);";
        assert_eq!(PostgresDialectParser.preprocess(sql), sql);
    }

    #[test]
    fn postgres_dialect_parses_dollar_quoted_function_body() {
        let sql = "\
CREATE FUNCTION add_one(x INT) RETURNS INT AS $$
BEGIN
  RETURN x + 1;
END;
$$ LANGUAGE plpgsql;";

        let dialect = PostgresDialectParser.sqlparser_dialect();
        assert!(
            Parser::parse_sql(&*dialect, sql).is_ok(),
            "PostgreSqlDialect is expected to parse a dollar-quoted function body header"
        );
    }
}
