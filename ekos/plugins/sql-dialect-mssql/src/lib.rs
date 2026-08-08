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

use ekos_sql_dialect_sdk::SqlDialectParser;
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
