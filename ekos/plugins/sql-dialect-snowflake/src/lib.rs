//! Snowflake `SqlDialectParser` (RFC 0039, extending RFC 0031's pattern).
//!
//! Wraps `sqlparser::dialect::SnowflakeDialect` — already available in the pinned
//! `sqlparser = "0.53"` dependency, zero new dependency. Previously only reachable via
//! `sql_transform_analyzer.rs`'s private `dialect_for` (which had no `"snowflake"` arm at all —
//! it silently fell through to `GenericDialect`); this crate makes it independently selectable,
//! testable, and usable by both `SqlAnalyzerPass` and `SqlTransformAnalyzerPass` via the shared
//! dialect registry.
//!
//! No preprocessing needed: Snowflake has no client-tool text convention comparable to MySQL's
//! `DELIMITER`. `SnowflakeDialect` accepts real Snowflake syntax extensions `GenericDialect`
//! rejects — verified directly (trailing commas in a `SELECT` projection list, below).
//! `$$ ... $$`-quoted procedure/function bodies (real Snowflake syntax) are *not* covered:
//! `sqlparser` 0.53's `CREATE PROCEDURE`/`CREATE FUNCTION` grammar only accepts the MSSQL-style
//! `AS BEGIN ... END`/Postgres-style single-`Expr`-body shapes respectively, not Snowflake's own
//! `$$`-quoted scripting body — found while writing this crate's tests, not assumed. A Snowflake
//! procedure/function with a `$$`-quoted body compiles to `Unmapped` today (same honest-gap
//! posture as `pentaho_analyzer.rs`'s documented approximations), not a parser crash.

use ekos_sql_dialect_sdk::SqlDialectParser;
use sqlparser::dialect::{Dialect, SnowflakeDialect};

pub struct SnowflakeDialectParser;

impl SqlDialectParser for SnowflakeDialectParser {
    fn name(&self) -> &str {
        "snowflake"
    }

    fn sqlparser_dialect(&self) -> Box<dyn Dialect + Send + Sync> {
        Box::new(SnowflakeDialect {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::parser::Parser;

    #[test]
    fn name_is_snowflake() {
        assert_eq!(SnowflakeDialectParser.name(), "snowflake");
    }

    #[test]
    fn preprocess_is_identity() {
        let sql = "CREATE TABLE t (id INT);";
        assert_eq!(SnowflakeDialectParser.preprocess(sql), sql);
    }

    #[test]
    fn snowflake_dialect_parses_trailing_comma_in_projection() {
        // A real Snowflake syntax extension (`supports_projection_trailing_commas`) —
        // `GenericDialect` rejects this (verified directly: "Expected an expression, found:
        // FROM"), `SnowflakeDialect` accepts it.
        let sql = "SELECT a, b, FROM t";
        let dialect = SnowflakeDialectParser.sqlparser_dialect();
        assert!(
            Parser::parse_sql(&*dialect, sql).is_ok(),
            "SnowflakeDialect is expected to accept a trailing comma in a SELECT projection"
        );
    }
}
