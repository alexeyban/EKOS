//! MSSQL (T-SQL) `SqlDialectParser` (RFC 0039, extending RFC 0031's pattern).
//!
//! Wraps `sqlparser::dialect::MsSqlDialect` — already available in the pinned
//! `sqlparser = "0.53"` dependency, zero new dependency, and already used internally by
//! `sql_transform_analyzer.rs`'s private `dialect_for` before this crate existed. Written as
//! part of RFC 0039's unification of `SqlAnalyzerPass`/`SqlTransformAnalyzerPass` onto one
//! resolved `SqlDialectParser`: MSSQL was reachable *only* through the private `dialect_for`,
//! never through the shared dialect registry (`sql_dialect_registry.rs`'s
//! `build_dialect_registry()` had no `"mssql"` entry at all) — unifying both passes on the
//! registry without adding this crate first would have silently broken existing `dialect =
//! "mssql"` workspace configs.
//!
//! Registered under three keys in the registry (`"mssql"`, `"tsql"`, `"synapse"`), preserving
//! the exact alias set `sql_transform_analyzer.rs`'s old private `dialect_for` recognized.
//!
//! No preprocessing needed: MSSQL's `CREATE PROCEDURE ... AS BEGIN ... END` body is real T-SQL
//! grammar `sqlparser` already parses into a structured statement list, no client-tool text
//! convention to strip first.

use ekos_sql_dialect_sdk::{DialectFacts, RowLimit, SqlDialectParser};
use sqlparser::dialect::{Dialect, MsSqlDialect};

pub struct MsSqlDialectParser {
    name: &'static str,
}

impl MsSqlDialectParser {
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }
}

impl SqlDialectParser for MsSqlDialectParser {
    fn name(&self) -> &str {
        self.name
    }

    fn sqlparser_dialect(&self) -> Box<dyn Dialect + Send + Sync> {
        Box::new(MsSqlDialect {})
    }

    /// RFC 0242/0243: the engine behaviour the text-to-SQL checks consult.
    fn facts(&self) -> DialectFacts {
        DialectFacts {
            integer_division: true,
            nulls_first_ascending: true,
            like_case_insensitive: true,
            row_limit: RowLimit::TopOrFetch,
            identifier_quote: Some('['),
            ..DialectFacts::GENERIC
        }
    }

    /// RFC 0243: how to write correct SQL for this engine.
    fn generation_notes(&self) -> &'static [&'static str] {
        &[
            "Limit rows with SELECT TOP (n) …, or ORDER BY … OFFSET 0 ROWS FETCH NEXT n ROWS ONLY; there is no LIMIT.",
            "/ between two integers truncates: multiply by 1.0 or CAST(x AS DECIMAL(18, 4)) first.",
            "Concatenate with + or CONCAT(), which treats NULL as empty.",
            "Comparison follows the collation, usually case-insensitive; there is no ILIKE.",
            "Quote names with [brackets] or double quotes.",
            "Dates: CAST('2024-01-31' AS date), YEAR(col), DATEADD(day, n, d), DATEDIFF(day, a, b), FORMAT(col, 'yyyy-MM').",
            "NULL sorts first in ascending order.",
            "A condition is not a value: write CASE WHEN … THEN 1 ELSE 0 END to count or select it.",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::parser::Parser;

    #[test]
    fn name_reports_the_configured_alias() {
        assert_eq!(MsSqlDialectParser::new("mssql").name(), "mssql");
        assert_eq!(MsSqlDialectParser::new("tsql").name(), "tsql");
        assert_eq!(MsSqlDialectParser::new("synapse").name(), "synapse");
    }

    #[test]
    fn preprocess_is_identity() {
        let sql = "SELECT * FROM t;";
        assert_eq!(MsSqlDialectParser::new("mssql").preprocess(sql), sql);
    }

    #[test]
    fn mssql_dialect_parses_create_procedure_with_begin_end_body() {
        let sql = "CREATE PROCEDURE load_active AS BEGIN \
                    SELECT id FROM customers WHERE status = 'active' \
                    END";
        let dialect = MsSqlDialectParser::new("mssql").sqlparser_dialect();
        assert!(
            Parser::parse_sql(&*dialect, sql).is_ok(),
            "MsSqlDialect is expected to parse a CREATE PROCEDURE ... AS BEGIN ... END body"
        );
    }
}
