//! Databricks (Spark SQL) `SqlDialectParser` (RFC 0039, extending RFC 0031's pattern).
//!
//! Wraps `sqlparser::dialect::DatabricksDialect` — already available in the pinned
//! `sqlparser = "0.53"` dependency, zero new dependency, and already used internally by
//! `sql_transform_analyzer.rs`'s private `dialect_for` before this crate existed. This crate
//! makes it independently selectable and testable via the shared dialect registry, and — new —
//! usable by `SqlAnalyzerPass` (DDL/`CREATE TABLE` recovery), which previously had no path to it
//! at all.
//!
//! No preprocessing needed: no MySQL-`DELIMITER`-style client-tool convention applies to
//! Databricks SQL text.
//!
//! Coverage note carried over from `sql_transform_analyzer.rs`'s original doc comment: not
//! independently verified against real Spark SQL extensions beyond `sqlparser`'s own
//! `DatabricksDialect` coverage (e.g. backtick-delimited identifiers, per its
//! `is_delimited_identifier_start` — verified below). Flagged, not blocking.

use ekos_sql_dialect_sdk::SqlDialectParser;
use sqlparser::dialect::{DatabricksDialect, Dialect};

pub struct DatabricksDialectParser;

impl SqlDialectParser for DatabricksDialectParser {
    fn name(&self) -> &str {
        "databricks"
    }

    fn sqlparser_dialect(&self) -> Box<dyn Dialect + Send + Sync> {
        Box::new(DatabricksDialect {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::parser::Parser;

    #[test]
    fn name_is_databricks() {
        assert_eq!(DatabricksDialectParser.name(), "databricks");
    }

    #[test]
    fn preprocess_is_identity() {
        let sql = "SELECT * FROM t;";
        assert_eq!(DatabricksDialectParser.preprocess(sql), sql);
    }

    #[test]
    fn databricks_dialect_parses_backtick_delimited_identifiers() {
        let sql = "SELECT `col name` FROM `my table`";
        let dialect = DatabricksDialectParser.sqlparser_dialect();
        assert!(
            Parser::parse_sql(&*dialect, sql).is_ok(),
            "DatabricksDialect is expected to parse backtick-delimited identifiers"
        );
    }
}
