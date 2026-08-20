//! ClickHouse `SqlDialectParser` (RFC 0031 pattern, added for RFC 0056).
//!
//! Wraps `sqlparser::dialect::ClickHouseDialect`, already available in the pinned
//! `sqlparser = "0.53"` (confirmed present at `sqlparser-0.53.0/src/dialect/clickhouse.rs`) — no
//! new dependency.
//!
//! **Preprocessing (RFC 0057, RFC 0058):** `sqlparser`'s `ClickHouseDialect` recognizes
//! `MATERIALIZED`/`ALIAS`/`EPHEMERAL` as ClickHouse-specific column options
//! (`parser/mod.rs:6431-6448`) and `PRIMARY KEY`/`ORDER BY`/`ENGINE` as `CREATE TABLE` options —
//! but real ClickHouse DDL uses several more clauses this crate has no parse path for at all:
//!
//! - `CODEC(...)` (RFC 0057) — per-column compression codec, used on most columns of most
//!   `MergeTree` tables.
//! - `INDEX <name> <expr> TYPE <type> GRANULARITY <n>` (RFC 0058) — table-level secondary index
//!   inside the column list. `sqlparser`'s only `INDEX`-as-table-constraint grammar is gated to
//!   `GenericDialect | MySqlDialect` and parses MySQL's `INDEX name (col, ...)` shape regardless —
//!   neither the gate nor the grammar shape matches ClickHouse's form.
//! - `PARTITION BY <expr>` (RFC 0058) — `CREATE TABLE`'s `partition_by` field is only parsed for
//!   `BigQueryDialect | PostgreSqlDialect | GenericDialect`, ClickHouse excluded, despite
//!   `PARTITION BY` being arguably ClickHouse's single most characteristic `MergeTree` clause.
//! - `SAMPLE BY <expr>` (RFC 0058) — `Keyword::SAMPLE` doesn't exist anywhere in `sqlparser`'s
//!   keyword table at all.
//! - `SETTINGS <k>=<v>, ...` (RFC 0058) — no `CREATE TABLE` handling anywhere in the crate.
//! - `CREATE DICTIONARY ...` (RFC 0058) — an entirely different, ClickHouse-only statement type
//!   `sqlparser` has no grammar for at all (not a gated option on an existing statement, a
//!   missing statement kind).
//!
//! Confirmed live against a real, unmodified ClickHouse schema (Plausible Analytics'
//! `priv/ingest_repo/structure.sql`, embedded as a test fixture below): every one of the above
//! caused a whole-file parse failure (`falling back to empty graph`), not a partial one —
//! `SqlAnalyzerPass` parses an entire file in one `Parser::parse_sql` call and discards every
//! table in it if *any* statement anywhere fails. `preprocess` strips each of these, well-formed
//! occurrences only, before the SQL reaches `sqlparser`, the same slot `MySqlDialectParser`
//! already uses to strip `DELIMITER` directives. This is a repair of the file-based DDL path
//! only — `ekos-plugin-clickhouse`'s live `system.columns` introspection (RFC 0056 Stage 1) never
//! modeled any of these either, so no information already captured elsewhere is lost.
//!
//! Used in two places: RFC 0031's existing file-based `SqlAnalyzerPass`/`SqlTransformAnalyzerPass`
//! registry (so `.sql` files authored against ClickHouse parse correctly), and, since RFC 0056,
//! as the SELECT-only validation gate in front of Stage 2's live query execution — an
//! LLM-generated query is parsed with this dialect and rejected unless it is a single
//! `Statement::Query`. Live queries are always plain `SELECT`s, never `CREATE TABLE`/`CREATE
//! DICTIONARY`, so none of this preprocessing changes that gate's behavior.

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

    fn preprocess(&self, sql: &str) -> String {
        preprocess_clickhouse_ddl(sql)
    }
}

/// Runs every clause-stripping pass, in order. `CODEC` first since it operates purely inside
/// column definitions and doesn't interact with any of the others; `INDEX` next since it's also
/// inside the column list; then the three table-options-tail clauses (`PARTITION BY`/`SAMPLE
/// BY`/`SETTINGS`), each independent of the others since they match on keywords, not on what
/// prior passes removed; `CREATE DICTIONARY` last since it operates at the whole-statement level.
fn preprocess_clickhouse_ddl(sql: &str) -> String {
    let sql = strip_codec_clauses(sql);
    let sql = strip_index_clauses(&sql);
    let sql = strip_keyword_expr_clause(
        &sql,
        "PARTITION",
        Some("BY"),
        &["PRIMARY", "ORDER", "SAMPLE", "SETTINGS", "COMMENT"],
    );
    let sql = strip_keyword_expr_clause(&sql, "SAMPLE", Some("BY"), &["SETTINGS", "COMMENT"]);
    let sql = strip_keyword_expr_clause(&sql, "SETTINGS", None, &["COMMENT"]);
    strip_create_dictionary_statements(&sql)
}

/// Removes well-formed `CODEC(...)` clauses from ClickHouse DDL — see the module doc for why
/// `sqlparser` needs this. Quote-aware (single-quoted string literals, with `ClickHouseDialect`'s
/// backslash-escape convention, and backtick-quoted identifiers) so the literal word `CODEC`
/// inside either is never mistaken for the clause. Only removes a clause it can fully match a
/// balanced closing paren for; a malformed `CODEC` (no following `(`, or an unmatched paren) is
/// left untouched and fails downstream exactly as it does today.
fn strip_codec_clauses(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_backtick = false;

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

        if in_backtick {
            out.push(c);
            if c == '`' {
                in_backtick = false;
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

        if c == '`' {
            in_backtick = true;
            out.push(c);
            i += 1;
            continue;
        }

        if is_word_boundary_match(&chars, i, "CODEC") {
            let after = i + "CODEC".chars().count();
            let mut j = after;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if chars.get(j) == Some(&'(')
                && let Some(close) = matching_paren(&chars, j)
            {
                // Drop the single space/tab that separated the data type (or a prior
                // option) from CODEC — otherwise it dangles before the next `,`/`)`.
                while matches!(out.chars().last(), Some(' ') | Some('\t')) {
                    out.pop();
                }
                i = close + 1;
                continue;
            }
        }

        out.push(c);
        i += 1;
    }

    out
}

/// Strips ClickHouse's table-level `INDEX <name> <expr> TYPE <type_expr> GRANULARITY <n>`
/// secondary-index definitions from inside a `CREATE TABLE` column list (RFC 0058). Also removes
/// one adjacent comma (preferring a following one, else trimming a preceding one already
/// emitted) so the enclosing column list stays well-formed. Only removes a clause where a
/// well-formed `GRANULARITY <digits>` was actually found; a malformed one is left untouched.
fn strip_index_clauses(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_backtick = false;

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
        if in_backtick {
            out.push(c);
            if c == '`' {
                in_backtick = false;
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
        if c == '`' {
            in_backtick = true;
            out.push(c);
            i += 1;
            continue;
        }

        if is_word_boundary_match(&chars, i, "INDEX")
            && let Some(end) = find_index_clause_end(&chars, i)
        {
            let mut k = end;
            while k < chars.len() && chars[k].is_whitespace() {
                k += 1;
            }
            while matches!(
                out.chars().last(),
                Some(' ') | Some('\t') | Some('\n') | Some('\r')
            ) {
                out.pop();
            }
            if chars.get(k) == Some(&',') {
                i = k + 1;
                continue;
            }
            if out.ends_with(',') {
                out.pop();
            }
            i = end;
            continue;
        }

        out.push(c);
        i += 1;
    }

    out
}

/// Finds the end index (exclusive) of an `INDEX ... GRANULARITY <n>` clause starting at
/// `index_start` (the `I` of `INDEX`) — the position right after the granularity value's digits.
/// Tracks paren depth so a parameterized `TYPE` (e.g. `TYPE bloom_filter(0.01)`) doesn't confuse
/// the scan. Returns `None` (leaving the clause untouched) if a top-level `,`/`)` — the end of
/// the enclosing column list — is reached before a well-formed `GRANULARITY <digits>` is found.
fn find_index_clause_end(chars: &[char], index_start: usize) -> Option<usize> {
    let mut i = index_start;
    let mut depth = 0i32;
    let mut in_string = false;

    while i < chars.len() {
        let c = chars[i];
        if in_string {
            if c == '\\' && i + 1 < chars.len() {
                i += 2;
                continue;
            }
            if c == '\'' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '\'' {
            in_string = true;
            i += 1;
            continue;
        }
        if depth == 0 {
            if is_word_boundary_match(chars, i, "GRANULARITY") {
                let mut j = i + "GRANULARITY".chars().count();
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                let digits_start = j;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
                return if j > digits_start { Some(j) } else { None };
            }
            if c == ',' || c == ')' {
                return None;
            }
        }
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        i += 1;
    }

    None
}

/// Strips `<keyword> [<keyword2>] <expr>` clauses `sqlparser`'s `ClickHouseDialect` has no parse
/// path for at all — `PARTITION BY`, `SAMPLE BY`, bare `SETTINGS` (RFC 0058). `<expr>` runs
/// until, at paren-depth 0 outside quotes, the next occurrence of any word in `terminators`, a
/// top-level `;`, or end of input; that terminator is left completely untouched for `sqlparser`
/// (or a later pass in the same chain) to parse. A malformed clause (`keyword` not followed by
/// `keyword2`, when required) is left untouched.
fn strip_keyword_expr_clause(
    sql: &str,
    keyword: &str,
    keyword2: Option<&str>,
    terminators: &[&str],
) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_backtick = false;

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
        if in_backtick {
            out.push(c);
            if c == '`' {
                in_backtick = false;
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
        if c == '`' {
            in_backtick = true;
            out.push(c);
            i += 1;
            continue;
        }

        if is_word_boundary_match(&chars, i, keyword) {
            let after_keyword = i + keyword.chars().count();
            let (matched, clause_body_start) = match keyword2 {
                Some(kw2) => {
                    let mut k = after_keyword;
                    while k < chars.len() && chars[k].is_whitespace() {
                        k += 1;
                    }
                    if is_word_boundary_match(&chars, k, kw2) {
                        (true, k + kw2.chars().count())
                    } else {
                        (false, after_keyword)
                    }
                }
                None => (true, after_keyword),
            };

            if matched {
                let end = scan_to_terminator(&chars, clause_body_start, terminators);
                // Only safe to trim the whitespace right before `end` when `end` is a `;` or
                // end of input — trimming before a terminator *keyword* would glue it directly
                // onto whatever precedes it (`MergeTreePRIMARY`).
                if chars.get(end) == Some(&';') || end == chars.len() {
                    while matches!(
                        out.chars().last(),
                        Some(' ') | Some('\t') | Some('\n') | Some('\r')
                    ) {
                        out.pop();
                    }
                }
                i = end;
                continue;
            }
        }

        out.push(c);
        i += 1;
    }

    out
}

/// Scans forward from `start`, tracking paren depth and single-quoted strings, and returns the
/// index of the first position (at paren depth 0, outside a string) where either a top-level `;`
/// is found or the upcoming word matches one of `terminators` at a word boundary. Returns
/// `chars.len()` (end of input) if neither is ever found.
fn scan_to_terminator(chars: &[char], start: usize, terminators: &[&str]) -> usize {
    let mut i = start;
    let mut depth = 0i32;
    let mut in_string = false;

    while i < chars.len() {
        let c = chars[i];
        if in_string {
            if c == '\\' && i + 1 < chars.len() {
                i += 2;
                continue;
            }
            if c == '\'' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '\'' {
            in_string = true;
            i += 1;
            continue;
        }
        if depth == 0 {
            if c == ';' {
                return i;
            }
            if terminators
                .iter()
                .any(|t| is_word_boundary_match(chars, i, t))
            {
                return i;
            }
        }
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        i += 1;
    }

    chars.len()
}

/// Strips whole `CREATE DICTIONARY ... ;` statements (RFC 0058) — a ClickHouse-only statement
/// type `sqlparser` has no grammar for at all (confirmed: zero `DICTIONARY` hits anywhere in the
/// crate, not merely a gated option). Dropped entirely rather than partially parsed: EKOS's KIR
/// has never modeled dictionaries (RFC 0056 Stage 1 only ever emits `ObjectKind::Table`), so this
/// loses no information any existing pass already captured, and unblocks every other statement in
/// the same file — `SqlAnalyzerPass` parses a whole file in one `Parser::parse_sql` call and
/// discards everything on any single statement's failure.
fn strip_create_dictionary_statements(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_backtick = false;

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
        if in_backtick {
            out.push(c);
            if c == '`' {
                in_backtick = false;
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
        if c == '`' {
            in_backtick = true;
            out.push(c);
            i += 1;
            continue;
        }

        if is_word_boundary_match(&chars, i, "CREATE") {
            let mut j = i + "CREATE".chars().count();
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if is_word_boundary_match(&chars, j, "DICTIONARY") {
                let terminator = scan_to_terminator(&chars, j, &[]);
                let end = if chars.get(terminator) == Some(&';') {
                    terminator + 1
                } else {
                    terminator
                };
                i = end;
                continue;
            }
        }

        out.push(c);
        i += 1;
    }

    out
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

/// Finds the index of the `)` matching the `(` at `open_idx`, respecting nested parens and
/// single-quoted strings inside the codec expression itself.
fn matching_paren(chars: &[char], open_idx: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = open_idx;
    let mut in_string = false;

    while i < chars.len() {
        let c = chars[i];
        if in_string {
            if c == '\\' && i + 1 < chars.len() {
                i += 2;
                continue;
            }
            if c == '\'' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match c {
            '\'' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }

    None
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
    fn preprocess_is_identity_when_no_codec_clause_present() {
        let sql = "CREATE TABLE t (id UInt64) ENGINE = MergeTree ORDER BY id;";
        assert_eq!(ClickHouseDialectParser.preprocess(sql), sql);
    }

    #[test]
    fn preprocess_strips_a_simple_codec_clause() {
        let sql = "CREATE TABLE t (`hostname` String CODEC(ZSTD(3)), id UInt64);";
        let out = ClickHouseDialectParser.preprocess(sql);
        assert_eq!(out, "CREATE TABLE t (`hostname` String, id UInt64);");
    }

    #[test]
    fn preprocess_strips_a_nested_codec_clause() {
        // ZSTD(3) nests a paren inside CODEC(...) — the balanced-paren scan must not stop
        // at the first `)` it sees.
        let sql = "`hostname` String CODEC(ZSTD(3))";
        assert_eq!(ClickHouseDialectParser.preprocess(sql), "`hostname` String");
    }

    #[test]
    fn preprocess_strips_a_multi_arg_codec_clause() {
        let sql = "`timestamp` DateTime CODEC(Delta(4), LZ4)";
        assert_eq!(
            ClickHouseDialectParser.preprocess(sql),
            "`timestamp` DateTime"
        );
    }

    #[test]
    fn preprocess_does_not_strip_codec_inside_a_string_literal() {
        let sql = "DEFAULT 'this mentions CODEC(ZSTD) in a string'";
        assert_eq!(ClickHouseDialectParser.preprocess(sql), sql);
    }

    #[test]
    fn preprocess_does_not_strip_codec_inside_a_backtick_identifier() {
        let sql = "`CODEC(fake)` String";
        assert_eq!(ClickHouseDialectParser.preprocess(sql), sql);
    }

    #[test]
    fn preprocess_leaves_a_malformed_codec_untouched() {
        // No opening paren after CODEC — nothing well-formed to remove.
        let sql = "`x` String CODEC ZSTD";
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

    /// Real excerpt from `analytics/priv/ingest_repo/structure.sql` (Plausible Analytics) —
    /// the exact shape that failed live before RFC 0057: `sql parser error: Expected: ',' or
    /// ')' after column definition, found: CODEC at Line: 7, Column: 23`.
    #[test]
    fn clickhouse_dialect_parses_real_codec_bearing_column_after_preprocessing() {
        let sql = "\
CREATE TABLE plausible_events_db.sessions_v2
(
    `session_id` UInt64,
    `sign` Int8,
    `site_id` UInt64,
    `hostname` String CODEC(ZSTD(3)),
    `timestamp` DateTime CODEC(Delta(4), LZ4),
    `start` DateTime CODEC(Delta(4), LZ4)
)
ENGINE = VersionedCollapsingMergeTree(sign, events)
ORDER BY (site_id, toDate(start), session_id);";

        let preprocessed = ClickHouseDialectParser.preprocess(sql);
        assert!(
            !preprocessed.contains("CODEC"),
            "CODEC clauses must be fully stripped:\n{preprocessed}"
        );

        let dialect = ClickHouseDialectParser.sqlparser_dialect();
        let result = Parser::parse_sql(&*dialect, &preprocessed);
        assert!(
            result.is_ok(),
            "expected the preprocessed real-world DDL to parse, got: {result:?}"
        );
    }

    // ── RFC 0058: INDEX / PARTITION BY / SAMPLE BY / SETTINGS / CREATE DICTIONARY ──────────

    #[test]
    fn strip_index_clauses_removes_a_middle_entry_and_keeps_one_comma() {
        let sql =
            "(\n    `a` UInt64,\n    INDEX idx a TYPE minmax GRANULARITY 1,\n    `b` UInt64\n)";
        let out = strip_index_clauses(sql);
        assert_eq!(out, "(\n    `a` UInt64,\n    `b` UInt64\n)");
    }

    #[test]
    fn strip_index_clauses_removes_a_last_entry_and_drops_preceding_comma() {
        let sql = "(\n    `a` UInt64,\n    INDEX idx a TYPE minmax GRANULARITY 1\n)";
        let out = strip_index_clauses(sql);
        assert_eq!(out, "(\n    `a` UInt64\n)");
    }

    #[test]
    fn strip_index_clauses_handles_a_parameterized_type() {
        let sql = "INDEX idx a TYPE bloom_filter(0.01) GRANULARITY 4)";
        let out = strip_index_clauses(sql);
        assert_eq!(out, ")");
    }

    #[test]
    fn strip_index_clauses_leaves_malformed_index_untouched() {
        // No GRANULARITY at all — nothing well-formed to remove.
        let sql = "INDEX idx a TYPE minmax)";
        assert_eq!(strip_index_clauses(sql), sql);
    }

    #[test]
    fn strip_keyword_expr_clause_removes_partition_by_stopping_at_primary() {
        let sql = "ENGINE = MergeTree PARTITION BY toYYYYMM(start) PRIMARY KEY id";
        let out = strip_keyword_expr_clause(
            sql,
            "PARTITION",
            Some("BY"),
            &["PRIMARY", "ORDER", "SAMPLE", "SETTINGS", "COMMENT"],
        );
        assert_eq!(out, "ENGINE = MergeTree PRIMARY KEY id");
    }

    #[test]
    fn strip_keyword_expr_clause_removes_partition_by_stopping_at_settings() {
        let sql = "PARTITION BY toYYYYMM(ts) SETTINGS index_granularity = 8192;";
        let out = strip_keyword_expr_clause(
            sql,
            "PARTITION",
            Some("BY"),
            &["PRIMARY", "ORDER", "SAMPLE", "SETTINGS", "COMMENT"],
        );
        assert_eq!(out, "SETTINGS index_granularity = 8192;");
    }

    #[test]
    fn strip_keyword_expr_clause_removes_sample_by() {
        let sql = "ORDER BY id SAMPLE BY user_id SETTINGS x = 1;";
        let out = strip_keyword_expr_clause(sql, "SAMPLE", Some("BY"), &["SETTINGS", "COMMENT"]);
        assert_eq!(out, "ORDER BY id SETTINGS x = 1;");
    }

    #[test]
    fn strip_keyword_expr_clause_removes_bare_settings() {
        let sql =
            "ORDER BY id SETTINGS index_granularity = 8192, replicated_deduplication_window = 0;";
        let out = strip_keyword_expr_clause(sql, "SETTINGS", None, &["COMMENT"]);
        assert_eq!(out, "ORDER BY id;");
    }

    #[test]
    fn strip_keyword_expr_clause_stops_settings_before_table_comment() {
        let sql = "ORDER BY id SETTINGS index_granularity = 128 COMMENT '2024-07-09';";
        let out = strip_keyword_expr_clause(sql, "SETTINGS", None, &["COMMENT"]);
        assert_eq!(out, "ORDER BY id COMMENT '2024-07-09';");
    }

    #[test]
    fn strip_keyword_expr_clause_leaves_partition_without_by_untouched() {
        let sql = "PARTITION xyz PRIMARY KEY id";
        let out = strip_keyword_expr_clause(
            sql,
            "PARTITION",
            Some("BY"),
            &["PRIMARY", "ORDER", "SAMPLE", "SETTINGS", "COMMENT"],
        );
        assert_eq!(out, sql);
    }

    #[test]
    fn strip_create_dictionary_statements_removes_one_statement() {
        let sql = "\
CREATE TABLE t (id UInt64);
CREATE DICTIONARY d (id String) PRIMARY KEY id SOURCE(CLICKHOUSE(TABLE x));
CREATE TABLE u (id UInt64);";
        let out = strip_create_dictionary_statements(sql);
        assert_eq!(
            out,
            "\
CREATE TABLE t (id UInt64);

CREATE TABLE u (id UInt64);"
        );
    }

    #[test]
    fn strip_create_dictionary_statements_removes_two_statements() {
        let sql = "CREATE DICTIONARY a (x String); CREATE DICTIONARY b (y String);";
        // The single space between the two statements is left alone — harmless whitespace,
        // not part of either statement's own span.
        assert_eq!(strip_create_dictionary_statements(sql), " ");
    }

    /// The entire, unmodified real `structure.sql` from Plausible Analytics
    /// (`analytics/priv/ingest_repo/structure.sql`) — the file that motivated both RFC 0057 and
    /// RFC 0058. Every table, dictionary, and the trailing `schema_migrations` seed `INSERT` must
    /// parse after preprocessing.
    const REAL_ANALYTICS_STRUCTURE_SQL: &str =
        include_str!("../tests/fixtures/analytics-structure.sql");

    #[test]
    fn clickhouse_dialect_parses_the_real_analytics_structure_sql_after_preprocessing() {
        let preprocessed = ClickHouseDialectParser.preprocess(REAL_ANALYTICS_STRUCTURE_SQL);

        for leftover in ["CODEC(", " INDEX ", "PARTITION BY", "SAMPLE BY", "SETTINGS"] {
            assert!(
                !preprocessed.contains(leftover),
                "expected {leftover:?} to be fully stripped, still present in:\n{preprocessed}"
            );
        }

        let dialect = ClickHouseDialectParser.sqlparser_dialect();
        let result = Parser::parse_sql(&*dialect, &preprocessed);
        assert!(
            result.is_ok(),
            "expected the real, preprocessed analytics/ structure.sql to parse, got: {result:?}"
        );

        let statements = result.unwrap();
        // 4 CREATE TABLE (sessions_v2, location_data, ingest_counters, events_v2, +10
        // imported_* + schema_migrations) — CREATE DICTIONARY statements were stripped
        // entirely, so only CREATE TABLE / INSERT statements remain.
        let create_table_count = statements
            .iter()
            .filter(|s| matches!(s, sqlparser::ast::Statement::CreateTable(_)))
            .count();
        assert_eq!(
            create_table_count, 15,
            "expected 15 CREATE TABLE statements (all real tables, dictionaries excluded), got {create_table_count} in {statements:#?}"
        );
    }
}
