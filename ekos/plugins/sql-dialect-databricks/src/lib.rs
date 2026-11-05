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

use ekos_sql_dialect_sdk::{DialectFacts, RowLimit, SqlDialectParser};
use sqlparser::dialect::{DatabricksDialect, Dialect};

pub struct DatabricksDialectParser;

impl SqlDialectParser for DatabricksDialectParser {
    fn name(&self) -> &str {
        "databricks"
    }

    fn sqlparser_dialect(&self) -> Box<dyn Dialect + Send + Sync> {
        Box::new(DatabricksDialect {})
    }

    /// RFC 0242/0243: the engine behaviour the text-to-SQL checks consult.
    fn facts(&self) -> DialectFacts {
        DialectFacts {
            nulls_first_ascending: true,
            has_ilike: true,
            row_limit: RowLimit::Limit,
            identifier_quote: Some('`'),
            backtick_identifiers: true,
            ..DialectFacts::GENERIC
        }
    }

    /// RFC 0243: how to write correct SQL for this engine.
    fn generation_notes(&self) -> &'static [&'static str] {
        &[
            "Limit rows with LIMIT n.",
            "/ gives a double; a div b is integer division.",
            "Concatenate with || or concat().",
            "= and LIKE are case-sensitive; ILIKE ignores case.",
            "Quote names with backticks.",
            "Dates: DATE'2024-01-31', date_trunc('MONTH', ts), year(ts), datediff(end, start) in days.",
            "NULL sorts first in ascending order.",
            "QUALIFY filters on window functions.",
        ]
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
