//! SQLite `SqlDialectParser` (RFC 0242).
//!
//! Wraps `sqlparser::dialect::SQLiteDialect`. No preprocessing: SQLite scripts carry no
//! client-tool syntax. What this crate adds is [`SqliteDialectParser::facts`] — the engine
//! behaviour generated SQL most often gets wrong on SQLite (integer division, NULL ordering,
//! dates stored as text), which the text-to-SQL checks in `ekos-sqlgen` consult.

use ekos_sql_dialect_sdk::{DialectFacts, RowLimit, SqlDialectParser};
use sqlparser::dialect::{Dialect, SQLiteDialect};

/// SQLite's documented behaviour: `/` on two integers truncates, NULL is smaller than every
/// value, `LIKE` folds ASCII case, there is no `ILIKE`, and dates are text (or numbers) compared
/// as stored.
pub const SQLITE_FACTS: DialectFacts = DialectFacts {
    integer_division: true,
    nulls_first_ascending: true,
    like_case_insensitive: true,
    has_ilike: false,
    dates_are_text: true,
    row_limit: RowLimit::Limit,
    identifier_quote: Some('"'),
    double_quote_is_string: false,
    backtick_identifiers: true,
};

pub struct SqliteDialectParser;

impl SqlDialectParser for SqliteDialectParser {
    fn name(&self) -> &str {
        "sqlite"
    }

    fn sqlparser_dialect(&self) -> Box<dyn Dialect + Send + Sync> {
        Box::new(SQLiteDialect {})
    }

    fn facts(&self) -> DialectFacts {
        SQLITE_FACTS
    }

    /// RFC 0243: how to write correct SQL for this engine.
    fn generation_notes(&self) -> &'static [&'static str] {
        &[
            "Limit rows with LIMIT n; there is no TOP and no FETCH FIRST.",
            "/ between two integers truncates (7 / 2 = 3): write CAST(x AS REAL) / y for a ratio or percentage.",
            "Concatenate with ||.",
            "= is case-sensitive; LIKE ignores ASCII case; there is no ILIKE.",
            "Dates are text (or numbers): compare in the stored format; take parts with strftime('%Y', col) or substr(col, 1, 4); julianday(a) - julianday(b) gives days.",
            "Quote names with double quotes (backticks and [ ] also work) and text values with single quotes: a double-quoted text that matches no column is silently read as a string.",
            "NULL sorts first in ascending order: filter col IS NOT NULL before ORDER BY col LIMIT n.",
            "There is no boolean type: conditions give 1 or 0, so SUM(cond) counts rows where cond holds.",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::parser::Parser;

    #[test]
    fn name_is_sqlite() {
        assert_eq!(SqliteDialectParser.name(), "sqlite");
    }

    #[test]
    fn parses_sqlite_quoting_and_functions() {
        let sql = "SELECT `col name`, \"other\", [third], strftime('%Y', d) FROM t \
                   WHERE x LIKE '%a%' ORDER BY 1 LIMIT 1";
        let dialect = SqliteDialectParser.sqlparser_dialect();
        assert!(Parser::parse_sql(&*dialect, sql).is_ok());
    }

    #[test]
    fn facts_state_integer_division_and_null_order() {
        let f = SqliteDialectParser.facts();
        assert!(f.integer_division && f.nulls_first_ascending && f.dates_are_text);
        assert!(!f.has_ilike);
    }
}
