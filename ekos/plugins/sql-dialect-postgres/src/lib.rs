//! PostgreSQL `SqlDialectParser` (RFC 0031).
//!
//! Wraps `sqlparser::dialect::PostgreSqlDialect`. Previously only used inside
//! `sql_transform_analyzer.rs`'s private `dialect_for` for `SELECT`/`CREATE VIEW`/`CREATE
//! PROCEDURE`/`CREATE FUNCTION` recovery — this crate makes it independently selectable and
//! testable, and — new — lets `SqlAnalyzerPass` (DDL/`CREATE TABLE` recovery) use it too, which
//! previously had no dialect awareness at all and always parsed with `GenericDialect`.
//!
//! **Preprocessing (RFC 0059):** dollar-quoted function bodies need no help — `sqlparser` already
//! tokenizes `$$ ... $$` as a real `DollarQuotedString`. But `CREATE SEQUENCE`/`ALTER SEQUENCE`
//! do: `sqlparser`'s `parse_create_sequence_options` (`parser/mod.rs:12699`) checks
//! `INCREMENT`/`MINVALUE`/`MAXVALUE`/`START`/`CACHE`/`CYCLE` in that fixed order, once each, with
//! no loop — real `pg_dump` output (confirmed on `analytics/priv/repo/structure.sql`, a real,
//! unmodified Postgres schema dump) emits `START WITH` *before* `INCREMENT BY`
//! (`CREATE SEQUENCE ... START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;`), which
//! this single-pass, fixed-order checker can't handle: it matches `START WITH 1` on its `START`
//! check, then finds no further match for any of the checks after it (`CACHE`, `CYCLE` — already
//! past in the fixed order), and returns with `INCREMENT BY 1 ...` still unconsumed, which then
//! fails the caller's own end-of-statement expectation — the exact real error this file produced:
//! `Expected: end of statement, found: INCREMENT at Line: 116, Column: 5`. This is a real,
//! still-open upstream `sqlparser` ordering bug, not a missing grammar rule (confirmed: `CREATE
//! SEQUENCE`/`ALTER SEQUENCE` both have real, if order-fragile, grammar in the pinned `sqlparser =
//! "0.53"`). `preprocess` strips whole `CREATE SEQUENCE ... ;`/`ALTER SEQUENCE ... ;` statements
//! rather than trying to reorder every clause combination `pg_dump` might emit — matching RFC
//! 0058's `CREATE DICTIONARY` precedent, sequences were never modeled in EKOS's KIR either (only
//! `Table`/`Column` facts come from DDL recovery), so nothing already captured is lost, and every
//! other statement in the same file is unblocked — `SqlAnalyzerPass` parses a whole file in one
//! `Parser::parse_sql` call and discards everything on any single statement's failure.

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

    fn preprocess(&self, sql: &str) -> String {
        preprocess_postgres_ddl(sql)
    }
}

/// Runs every preprocessing pass, in order. `CREATE SEQUENCE`/`ALTER SEQUENCE` stripping first
/// (whole-statement removal — independent of the `UNLOGGED` fix, which only ever touches
/// `CREATE TABLE`); `UNLOGGED` keyword removal last, since it operates on whatever `CREATE TABLE`
/// statements remain.
fn preprocess_postgres_ddl(sql: &str) -> String {
    let sql = strip_statements_starting_with(sql, &["CREATE", "SEQUENCE"]);
    let sql = strip_statements_starting_with(&sql, &["ALTER", "SEQUENCE"]);
    let sql = strip_unlogged_before_table(&sql);
    strip_not_valid_clause(&sql)
}

/// Removes a trailing `NOT VALID` clause — real Postgres grammar on `ADD CONSTRAINT ... CHECK
/// (...)` meaning "don't validate against existing rows" — that `sqlparser` has zero grammar for
/// anywhere (confirmed: no `NOT VALID`/`NotValid` hit anywhere in the crate). Confirmed on
/// `analytics/`: `ALTER TABLE ... ADD CONSTRAINT check_event_name_or_page_path CHECK (...) NOT
/// VALID;` — the constraint name and `CHECK` expression parse fine on their own; only the
/// trailing `NOT VALID` breaks the statement. `crates/recovery/src/sql_analyzer.rs` doesn't model
/// `CHECK` constraints as KIR facts at all (only columns/foreign keys are), so stripping just this
/// clause — keeping the rest of the `ALTER TABLE` statement intact, unlike whole-statement
/// stripping — loses nothing already captured, same reasoning as `strip_unlogged_before_table`.
fn strip_not_valid_clause(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while i < chars.len() {
        let c = chars[i];

        if in_single_quote {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '\'' {
                in_single_quote = false;
            }
            i += 1;
            continue;
        }
        if in_double_quote {
            out.push(c);
            if c == '"' {
                in_double_quote = false;
            }
            i += 1;
            continue;
        }
        if c == '\'' {
            in_single_quote = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '"' {
            in_double_quote = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }

        if is_word_boundary_match(&chars, i, "NOT") {
            let mut j = i + "NOT".chars().count();
            let ws_start = j;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if is_word_boundary_match(&chars, j, "VALID") && j > ws_start {
                let after = j + "VALID".chars().count();
                while matches!(out.chars().last(), Some(' ') | Some('\t')) {
                    out.pop();
                }
                i = after;
                continue;
            }
        }

        out.push(c);
        i += 1;
    }

    out
}

/// Removes the `UNLOGGED` keyword from `CREATE UNLOGGED TABLE` — `sqlparser`'s `parse_create`
/// dispatcher (`parser/mod.rs:3847`) has no case for it at all: after the `TEMP`/`TEMPORARY`
/// check it goes straight to `if self.parse_keyword(Keyword::TABLE)`, so `UNLOGGED` (a real,
/// tokenizable `Keyword` — `keywords.rs:821` — just never consulted here) makes the dispatcher
/// fall through every `else if` to `self.expected("an object type after CREATE", ...)`. Unlike
/// `CREATE SEQUENCE`, this drops a real user table (confirmed on `analytics/`: `CREATE UNLOGGED
/// TABLE public.oban_peers (...)`, a real table with real columns) if the whole statement were
/// stripped — `UNLOGGED` only changes storage durability (no WAL, no crash-safety), never the
/// schema, so removing just the keyword and keeping `CREATE TABLE ...` intact is strictly better:
/// no information lost, not even the durability nuance beyond what DDL recovery ever modeled to
/// begin with (nothing in EKOS's KIR represents `UNLOGGED`/WAL behavior today).
fn strip_unlogged_before_table(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while i < chars.len() {
        let c = chars[i];

        if in_single_quote {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '\'' {
                in_single_quote = false;
            }
            i += 1;
            continue;
        }
        if in_double_quote {
            out.push(c);
            if c == '"' {
                in_double_quote = false;
            }
            i += 1;
            continue;
        }
        if c == '\'' {
            in_single_quote = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '"' {
            in_double_quote = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }

        if is_word_boundary_match(&chars, i, "UNLOGGED") {
            let mut j = i + "UNLOGGED".chars().count();
            let ws_start = j;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if is_word_boundary_match(&chars, j, "TABLE") && j > ws_start {
                // Drop "UNLOGGED" and the whitespace around it; keep exactly one separating
                // space before "TABLE" so "CREATE" + "TABLE" don't glue together.
                while matches!(out.chars().last(), Some(' ') | Some('\t')) {
                    out.pop();
                }
                i = j;
                out.push(' ');
                continue;
            }
        }

        out.push(c);
        i += 1;
    }

    out
}

/// Strips whole statements whose leading keywords (in order, e.g. `["CREATE", "SEQUENCE"]` to
/// also match `CREATE TEMPORARY SEQUENCE`'s optional `TEMPORARY`/`TEMP` between them — see the
/// gap allowed between consecutive keywords below) match `keywords`, up to and including the
/// terminating top-level `;` (or end of input, if the statement is the last one and unterminated).
/// Quote-aware (single-quoted strings, double-quoted identifiers) and line-comment-aware (a real
/// `pg_dump`'s `-- Name: x; Type: SEQUENCE; ...` header routinely contains literal `;` and keyword
/// text inside the comment itself — both must be skipped over untouched, never mistaken for a
/// statement's own content) so neither a string/identifier nor a comment can be mistaken for real
/// statement text. Same shape as `sql-dialect-clickhouse`'s `strip_create_dictionary_statements`,
/// generalized to an arbitrary leading-keyword sequence so `CREATE SEQUENCE` and `ALTER SEQUENCE`
/// share one implementation.
fn strip_statements_starting_with(sql: &str, keywords: &[&str]) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while i < chars.len() {
        let c = chars[i];

        if in_single_quote {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '\'' {
                in_single_quote = false;
            }
            i += 1;
            continue;
        }
        if in_double_quote {
            out.push(c);
            if c == '"' {
                in_double_quote = false;
            }
            i += 1;
            continue;
        }
        if c == '\'' {
            in_single_quote = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '"' {
            in_double_quote = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            // Line comment: copy verbatim through the next newline (or end of input) — its
            // content (which may contain `;` or keyword text, e.g. a `pg_dump` header naming
            // the very statement kind being stripped) must never be scanned as real SQL.
            while i < chars.len() && chars[i] != '\n' {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }

        if matches_leading_keywords(&chars, i, keywords) {
            let terminator = scan_to_terminator(&chars, i, &[]);
            let end = if chars.get(terminator) == Some(&';') {
                terminator + 1
            } else {
                terminator
            };
            i = end;
            continue;
        }

        out.push(c);
        i += 1;
    }

    out
}

/// True if `keywords` occur one after another starting at `chars[start]`, each at a word
/// boundary, with only whitespace (and, between the first and second keyword only, one optional
/// extra identifier — e.g. `TEMPORARY`/`TEMP` in `CREATE TEMPORARY SEQUENCE`) separating them.
fn matches_leading_keywords(chars: &[char], start: usize, keywords: &[&str]) -> bool {
    let Some(&first) = keywords.first() else {
        return true;
    };
    if !is_word_boundary_match(chars, start, first) {
        return false;
    }
    let mut pos = start + first.chars().count();

    for (idx, &kw) in keywords.iter().enumerate().skip(1) {
        let mut j = pos;
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if is_word_boundary_match(chars, j, kw) {
            pos = j + kw.chars().count();
            continue;
        }
        // Only the gap right after the very first keyword may contain one extra word
        // (`CREATE [TEMPORARY|TEMP] SEQUENCE`).
        if idx == 1 {
            let mut k = j;
            while k < chars.len() && (chars[k].is_ascii_alphanumeric() || chars[k] == '_') {
                k += 1;
            }
            if k > j {
                while k < chars.len() && chars[k].is_whitespace() {
                    k += 1;
                }
                if is_word_boundary_match(chars, k, kw) {
                    pos = k + kw.chars().count();
                    continue;
                }
            }
        }
        return false;
    }
    true
}

/// Scans forward from `start`, tracking single- and double-quoted strings and `--` line comments
/// (a `;` or keyword-like text inside either must never end the scan early), and returns the
/// index of the first top-level `;` found, or `chars.len()` if none (end of input). `terminators`
/// is unused today (kept for API symmetry with `sql-dialect-clickhouse`'s version, which also
/// terminates on keywords) — `CREATE SEQUENCE`/`ALTER SEQUENCE` statements are always
/// `;`-terminated in real `pg_dump` output, never dollar-quoted bodies.
fn scan_to_terminator(chars: &[char], start: usize, _terminators: &[&str]) -> usize {
    let mut i = start;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while i < chars.len() {
        let c = chars[i];
        if in_single_quote {
            if c == '\\' && i + 1 < chars.len() {
                i += 2;
                continue;
            }
            if c == '\'' {
                in_single_quote = false;
            }
            i += 1;
            continue;
        }
        if in_double_quote {
            if c == '"' {
                in_double_quote = false;
            }
            i += 1;
            continue;
        }
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        match c {
            '\'' => in_single_quote = true,
            '"' => in_double_quote = true,
            ';' => return i,
            _ => {}
        }
        i += 1;
    }

    chars.len()
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn matches_word_at(chars: &[char], start: usize, word: &str) -> bool {
    let wchars: Vec<char> = word.chars().collect();
    let end = start + wchars.len();
    end <= chars.len() && chars[start..end] == wchars[..]
}

/// True if `word` occurs at `start` and is a real word (not a substring of a longer identifier)
/// — the char before `start`, if any, and the char after the word, if any, are both non-identifier
/// characters.
fn is_word_boundary_match(chars: &[char], start: usize, word: &str) -> bool {
    if !matches_word_at(chars, start, word) {
        return false;
    }
    let prev_is_ident = start > 0 && is_ident_char(chars[start - 1]);
    let after = start + word.chars().count();
    let next_is_ident = after < chars.len() && is_ident_char(chars[after]);
    !prev_is_ident && !next_is_ident
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

    /// The entire, unmodified real `structure.sql` from Plausible Analytics
    /// (`analytics/priv/repo/structure.sql`) — the file that motivated RFC 0059. Before
    /// preprocessing this fails whole-file (`sql parser error: Expected: end of statement, found:
    /// INCREMENT at Line: 116, Column: 5`, `SqlAnalyzerPass` parses a whole file in one
    /// `Parser::parse_sql` call and discards every table in it on any single statement's
    /// failure); after preprocessing every real `CREATE TABLE` (including the one `CREATE
    /// UNLOGGED TABLE`) must parse.
    const REAL_ANALYTICS_POSTGRES_STRUCTURE_SQL: &str =
        include_str!("../tests/fixtures/analytics-structure.sql");

    #[test]
    fn postgres_dialect_parses_the_real_analytics_structure_sql_after_preprocessing() {
        let preprocessed = PostgresDialectParser.preprocess(REAL_ANALYTICS_POSTGRES_STRUCTURE_SQL);

        for leftover in ["INCREMENT", "UNLOGGED", "NOT VALID"] {
            assert!(
                !preprocessed.contains(leftover),
                "expected {leftover:?} to be fully stripped, still present in:\n{preprocessed}"
            );
        }

        let dialect = PostgresDialectParser.sqlparser_dialect();
        let result = Parser::parse_sql(&*dialect, &preprocessed);
        assert!(
            result.is_ok(),
            "expected the real, preprocessed analytics/ Postgres structure.sql to parse, got: {result:?}"
        );

        let statements = result.unwrap();
        let create_table_count = statements
            .iter()
            .filter(|s| matches!(s, sqlparser::ast::Statement::CreateTable(_)))
            .count();
        // 41 ordinary CREATE TABLE + 1 CREATE UNLOGGED TABLE (oban_peers) — every real
        // application table in the dump, CREATE SEQUENCE/ALTER SEQUENCE excluded (never
        // modeled in the KIR, same reasoning RFC 0058 applied to CREATE DICTIONARY).
        assert_eq!(
            create_table_count, 42,
            "expected 42 real CREATE TABLE statements, got {create_table_count}"
        );
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

    // ── RFC 0059: CREATE/ALTER SEQUENCE, UNLOGGED, NOT VALID ───────────────────────────────

    #[test]
    fn strip_statements_starting_with_removes_a_create_sequence_statement() {
        let sql = "\
CREATE TABLE t (id INT);
CREATE SEQUENCE public.t_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;
CREATE TABLE u (id INT);";
        let out = strip_statements_starting_with(sql, &["CREATE", "SEQUENCE"]);
        assert_eq!(out, "CREATE TABLE t (id INT);\n\nCREATE TABLE u (id INT);");
    }

    #[test]
    fn strip_statements_starting_with_removes_an_alter_sequence_statement() {
        let sql = "CREATE TABLE t (id INT); ALTER SEQUENCE public.t_id_seq OWNED BY public.t.id; CREATE TABLE u (id INT);";
        let out = strip_statements_starting_with(sql, &["ALTER", "SEQUENCE"]);
        assert_eq!(out, "CREATE TABLE t (id INT);  CREATE TABLE u (id INT);");
    }

    #[test]
    fn strip_statements_starting_with_is_comment_aware() {
        // A real pg_dump header naming the very statement kind being stripped, with a literal
        // `;` inside the comment text itself — neither must confuse the scan.
        let sql = "\
--
-- Name: t_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.t_id_seq START WITH 1;
CREATE TABLE u (id INT);";
        let out = strip_statements_starting_with(sql, &["CREATE", "SEQUENCE"]);
        assert!(
            out.contains("CREATE TABLE u"),
            "the real statement after the stripped one must survive:\n{out}"
        );
        assert!(
            !out.contains("public.t_id_seq"),
            "the CREATE SEQUENCE statement must be fully removed:\n{out}"
        );
    }

    #[test]
    fn strip_statements_starting_with_leaves_unrelated_sql_untouched() {
        let sql = "CREATE TABLE t (id INT);";
        assert_eq!(
            strip_statements_starting_with(sql, &["CREATE", "SEQUENCE"]),
            sql
        );
    }

    #[test]
    fn strip_unlogged_before_table_removes_the_keyword() {
        let sql = "CREATE UNLOGGED TABLE public.oban_peers (name text NOT NULL);";
        assert_eq!(
            strip_unlogged_before_table(sql),
            "CREATE TABLE public.oban_peers (name text NOT NULL);"
        );
    }

    #[test]
    fn strip_unlogged_before_table_leaves_ordinary_create_table_untouched() {
        let sql = "CREATE TABLE t (id INT);";
        assert_eq!(strip_unlogged_before_table(sql), sql);
    }

    #[test]
    fn strip_not_valid_clause_removes_it_after_a_check_constraint() {
        let sql = "ALTER TABLE ONLY public.events\n    ADD CONSTRAINT check_x CHECK ((x IS NOT NULL)) NOT VALID;";
        let out = strip_not_valid_clause(sql);
        assert_eq!(
            out,
            "ALTER TABLE ONLY public.events\n    ADD CONSTRAINT check_x CHECK ((x IS NOT NULL));"
        );
    }

    #[test]
    fn strip_not_valid_clause_does_not_touch_an_unrelated_is_not_null() {
        // "IS NOT NULL" must survive untouched — only a standalone "NOT VALID" is stripped.
        let sql = "CHECK ((x IS NOT NULL));";
        assert_eq!(strip_not_valid_clause(sql), sql);
    }

    #[test]
    fn postgres_dialect_parses_create_sequence_and_alter_sequence_after_preprocessing() {
        let sql = "\
CREATE TABLE public.api_keys (id integer NOT NULL);
CREATE SEQUENCE public.api_keys_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;
ALTER SEQUENCE public.api_keys_id_seq OWNED BY public.api_keys.id;";
        let preprocessed = PostgresDialectParser.preprocess(sql);
        let dialect = PostgresDialectParser.sqlparser_dialect();
        let result = Parser::parse_sql(&*dialect, &preprocessed);
        assert!(
            result.is_ok(),
            "expected the preprocessed real-world DDL to parse, got: {result:?}"
        );
    }

    #[test]
    fn postgres_dialect_parses_unlogged_table_after_preprocessing() {
        let sql =
            "CREATE UNLOGGED TABLE public.oban_peers (name text NOT NULL, node text NOT NULL);";
        let preprocessed = PostgresDialectParser.preprocess(sql);
        let dialect = PostgresDialectParser.sqlparser_dialect();
        let result = Parser::parse_sql(&*dialect, &preprocessed);
        assert!(
            result.is_ok(),
            "expected the preprocessed real-world DDL to parse, got: {result:?}"
        );
        let statements = result.unwrap();
        assert!(matches!(
            statements.as_slice(),
            [sqlparser::ast::Statement::CreateTable(_)]
        ));
    }

    #[test]
    fn postgres_dialect_parses_not_valid_check_constraint_after_preprocessing() {
        let sql =
            "ALTER TABLE ONLY public.events\n    ADD CONSTRAINT check_x CHECK ((1 = 1)) NOT VALID;";
        let preprocessed = PostgresDialectParser.preprocess(sql);
        let dialect = PostgresDialectParser.sqlparser_dialect();
        let result = Parser::parse_sql(&*dialect, &preprocessed);
        assert!(
            result.is_ok(),
            "expected the preprocessed real-world DDL to parse, got: {result:?}"
        );
    }
}
