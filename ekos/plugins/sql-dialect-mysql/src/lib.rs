//! MySQL `SqlDialectParser` (RFC 0031).
//!
//! Wraps `sqlparser::dialect::MySqlDialect`, which already tokenizes `#`-style line comments
//! natively (confirmed against the pinned `sqlparser = "0.53"` — its tokenizer treats `#` as a
//! MySQL/Snowflake/BigQuery single-line comment starter) — this alone fixes the `#`-comment
//! parse failures from GitHub issue #3 simply by selecting the right dialect instead of
//! `GenericDialect`, which has no such rule.
//!
//! The one piece of real preprocessing this dialect needs is stripping `DELIMITER` directives:
//! MySQL dump/script tooling conventionally wraps a stored procedure/function body in
//! `DELIMITER $$ ... $$ DELIMITER ;` so the `mysql` CLI client doesn't split the body on its
//! internal `;` statements. `DELIMITER` is a client-tool convention, not SQL grammar — no
//! `sqlparser` dialect (or any real SQL parser) understands it, so it must be stripped, and the
//! custom delimiter it declares must be normalized back to `;`, before parsing.

use ekos_sql_dialect_sdk::SqlDialectParser;
use sqlparser::dialect::{Dialect, MySqlDialect};

pub struct MySqlDialectParser;

impl SqlDialectParser for MySqlDialectParser {
    fn name(&self) -> &str {
        "mysql"
    }

    fn sqlparser_dialect(&self) -> Box<dyn Dialect + Send + Sync> {
        Box::new(MySqlDialect {})
    }

    fn preprocess(&self, sql: &str) -> String {
        strip_delimiter_directives(sql)
    }
}

/// Removes `DELIMITER <token>` lines and rewrites any occurrence of the custom delimiter token
/// back to `;`, so the rest of the file parses as ordinary semicolon-terminated statements.
/// Line-oriented on purpose — `DELIMITER` is always its own statement on its own line by
/// convention (the `mysql` CLI requires this), so this doesn't need a full tokenizer.
fn strip_delimiter_directives(sql: &str) -> String {
    let mut current_delimiter = ";".to_string();
    let mut out = String::with_capacity(sql.len());

    for line in sql.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed
            .strip_prefix("DELIMITER ")
            .or_else(|| trimmed.strip_prefix("delimiter "))
        {
            current_delimiter = rest.trim().to_string();
            continue;
        }

        if current_delimiter != ";" && trimmed.ends_with(current_delimiter.as_str()) {
            let without_delim = &line[..line.len() - current_delimiter.len()];
            out.push_str(without_delim);
            out.push(';');
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_mysql() {
        assert_eq!(MySqlDialectParser.name(), "mysql");
    }

    #[test]
    fn preprocess_is_identity_when_no_delimiter_directive_present() {
        let sql = "CREATE TABLE t (id INT);\nSELECT * FROM t;\n";
        assert_eq!(MySqlDialectParser.preprocess(sql), sql);
    }

    #[test]
    fn preprocess_strips_delimiter_directives_and_restores_semicolons() {
        let sql = "\
DELIMITER $$
CREATE PROCEDURE p()
BEGIN
  SELECT 1$$
END$$
DELIMITER ;
";
        let out = MySqlDialectParser.preprocess(sql);
        assert!(
            !out.contains("DELIMITER"),
            "DELIMITER lines must be removed:\n{out}"
        );
        assert!(
            out.contains("SELECT 1;"),
            "custom delimiter must be rewritten to ';':\n{out}"
        );
        assert!(
            out.contains("END;"),
            "custom delimiter must be rewritten to ';':\n{out}"
        );
    }

    #[test]
    fn mysql_dialect_parses_hash_comment_that_generic_dialect_rejects() {
        use sqlparser::dialect::GenericDialect;
        use sqlparser::parser::Parser;

        let sql = "# a mysql-style comment\nCREATE TABLE t (id INT);";

        assert!(
            Parser::parse_sql(&GenericDialect {}, sql).is_err(),
            "GenericDialect is expected to reject a leading '#' comment"
        );

        let dialect = MySqlDialectParser.sqlparser_dialect();
        assert!(
            Parser::parse_sql(&*dialect, sql).is_ok(),
            "MySqlDialect is expected to accept a leading '#' comment"
        );
    }
}
