//! ClickHouse `SqlDialectParser` (RFC 0031 pattern, added for RFC 0056).
//!
//! Wraps `sqlparser::dialect::ClickHouseDialect`, already available in the pinned
//! `sqlparser = "0.53"` (confirmed present at `sqlparser-0.53.0/src/dialect/clickhouse.rs`) — no
//! new dependency. No preprocessing is needed: ClickHouse SQL has no client-tool convention like
//! MySQL's `DELIMITER` that needs stripping before `sqlparser` sees it.
//!
//! Used in two places: RFC 0031's existing file-based `SqlAnalyzerPass`/`SqlTransformAnalyzerPass`
//! registry (so `.sql` files authored against ClickHouse parse correctly), and, new in RFC 0056,
//! as the SELECT-only validation gate in front of Stage 2's live query execution — an
//! LLM-generated query is parsed with this dialect and rejected unless it is a single
//! `Statement::Query`.

use ekos_sql_dialect_sdk::SqlDialectParser;
use sqlparser::dialect::{ClickHouseDialect, Dialect};

pub struct ClickHouseDialectParser;

impl SqlDialectParser for ClickHouseDialectParser {
    fn name(&self) -> &str {
        "clickhouse"
    }

    fn sqlparser_dialect(&self) -> Box<dyn Dialect + Send + Sync> {
        Box::new(ClickHouseDialect {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::parser::Parser;

    #[test]
    fn name_is_clickhouse() {
        assert_eq!(ClickHouseDialectParser.name(), "clickhouse");
    }

    #[test]
    fn preprocess_is_identity() {
        let sql = "CREATE TABLE t (id UInt64) ENGINE = MergeTree ORDER BY id;";
        assert_eq!(ClickHouseDialectParser.preprocess(sql), sql);
    }

    #[test]
    fn clickhouse_dialect_parses_engine_and_order_by_clause() {
        let sql = "CREATE TABLE orders (id UInt64, created_at DateTime) \
                    ENGINE = MergeTree ORDER BY id;";
        let dialect = ClickHouseDialectParser.sqlparser_dialect();
        assert!(
            Parser::parse_sql(&*dialect, sql).is_ok(),
            "ClickHouseDialect is expected to parse ENGINE/ORDER BY table options"
        );
    }

    #[test]
    fn clickhouse_dialect_parses_a_select_statement() {
        let sql = "SELECT id, created_at FROM orders WHERE id > 10 LIMIT 100;";
        let dialect = ClickHouseDialectParser.sqlparser_dialect();
        assert!(Parser::parse_sql(&*dialect, sql).is_ok());
    }
}
