//! Best-effort repair for SQL scripts missing `;` between top-level statements
//! (GitHub issue #3's second root cause).
//!
//! `sqlparser::Parser::parse_sql` requires an explicit `;` separating statements; hand-written
//! scripts that instead rely on blank lines between statements (no trailing `;` at all) fail
//! with `Expected: end of statement, found: <next keyword>` on the *whole file*, not just the
//! offending statement. This is line-oriented, not a real tokenizer — it can misfire on a
//! top-level keyword used mid-construct (e.g. a `UNION ALL SELECT ...` chain, which legitimately
//! starts a line with `SELECT` while still being one statement) — so callers must only invoke it
//! as a fallback *after* the unmodified text has already failed to parse, never unconditionally.
//! That keeps well-formed multi-line constructs untouched and only risks the heuristic on input
//! that was already unparseable.

/// Top-level statement-starting keywords this heuristic looks for at the start of a line.
const STATEMENT_KEYWORDS: &[&str] = &[
    "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "ALTER", "DROP", "WITH", "MERGE", "TRUNCATE",
];

/// Inserts a synthetic `;` immediately before any line that starts a new top-level statement
/// (per [`STATEMENT_KEYWORDS`]) while still inside an unterminated statement at paren-depth 0.
pub(crate) fn ensure_statement_separators(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut depth: i32 = 0;
    let mut open_statement = false;
    let mut prev_line_is_set_op = false;

    for line in sql.lines() {
        let trimmed = line.trim();
        let starts_new_statement = depth == 0
            && open_statement
            && !prev_line_is_set_op
            && !trimmed.is_empty()
            && STATEMENT_KEYWORDS
                .iter()
                .any(|kw| starts_with_keyword(trimmed, kw));

        if starts_new_statement {
            out.push_str(";\n");
            open_statement = false;
        }

        out.push_str(line);
        out.push('\n');

        depth += line.matches('(').count() as i32 - line.matches(')').count() as i32;
        depth = depth.max(0);
        if !trimmed.is_empty() {
            open_statement = true;
            prev_line_is_set_op = ends_with_set_op_keyword(trimmed);
        }
        if depth == 0 && trimmed.ends_with(';') {
            open_statement = false;
        }
    }

    out
}

/// Whether `trimmed` ends with a set-operation keyword (`UNION`, `UNION ALL`, `INTERSECT`,
/// `EXCEPT`) — a `SELECT` on the following line is a continuation of the same statement, not a
/// new one, and must never be split.
fn ends_with_set_op_keyword(trimmed: &str) -> bool {
    let upper = trimmed.to_ascii_uppercase();
    upper.ends_with("UNION")
        || upper.ends_with("UNION ALL")
        || upper.ends_with("INTERSECT")
        || upper.ends_with("EXCEPT")
}

fn starts_with_keyword(trimmed: &str, kw: &str) -> bool {
    trimmed.len() >= kw.len()
        && trimmed[..kw.len()].eq_ignore_ascii_case(kw)
        && trimmed[kw.len()..]
            .chars()
            .next()
            .map(|c| !c.is_alphanumeric() && c != '_')
            .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::dialect::MsSqlDialect;
    use sqlparser::parser::Parser;

    #[test]
    fn inserts_semicolons_between_statements_missing_them() {
        let sql = "\
CREATE TABLE t (id INT, status VARCHAR(50))

UPDATE t SET status = 'active' WHERE id = 1

SELECT * FROM t
";
        let repaired = ensure_statement_separators(sql);
        let stmts = Parser::parse_sql(&MsSqlDialect {}, &repaired).unwrap();
        assert_eq!(stmts.len(), 3);
    }

    #[test]
    fn leaves_already_well_formed_multi_statement_sql_unchanged_in_effect() {
        let sql = "CREATE TABLE t (id INT);\n\nSELECT * FROM t;\n";
        let repaired = ensure_statement_separators(sql);
        let stmts = Parser::parse_sql(&MsSqlDialect {}, &repaired).unwrap();
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn does_not_split_a_union_chain_across_lines() {
        let sql = "SELECT a FROM t1\nUNION ALL\nSELECT b FROM t2\n";
        let repaired = ensure_statement_separators(sql);
        let stmts = Parser::parse_sql(&MsSqlDialect {}, &repaired).unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn does_not_split_inside_open_parens() {
        let sql = "CREATE TABLE t (\n  id INT,\n  status VARCHAR(50)\n)\n\nSELECT * FROM t\n";
        let repaired = ensure_statement_separators(sql);
        let stmts = Parser::parse_sql(&MsSqlDialect {}, &repaired).unwrap();
        assert_eq!(stmts.len(), 2);
    }
}
