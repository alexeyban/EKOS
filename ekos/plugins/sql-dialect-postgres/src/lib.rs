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
//!
//! **Hand-written schemas (RFC 0146):** everything above was tuned against `pg_dump` output, which
//! emits a deliberately narrow, mechanical subset of PostgreSQL. Hand-maintained schemas do not.
//! Measured on LedgerSMB (a Perl/PostgreSQL ERP, 260 `.sql` files): with `dialect = "postgres"`
//! correctly configured, its core schema `sql/Pg-database.sql` recovered **0 of 158 tables**,
//! because the file's first `COMMENT ON TABLE ... IS $$...$$` failed the whole-file parse and took
//! every table with it. RFC 0146 adds the transforms below for constructs `sqlparser 0.53` has no
//! grammar for but real schemas use constantly — `COMMENT ON`, `INHERITS`, `SECURITY DEFINER`,
//! `RETURNS SETOF`, `:=` named arguments, `DO` blocks, `CREATE RULE`, psql meta-commands and
//! `COPY ... FROM stdin` payloads. After them the same file parses whole and yields 158 tables and
//! 214 foreign keys.
//!
//! Two hazards that cost real debugging time and are guarded by tests here:
//!
//! * **`\.` is not a psql meta-command** — it terminates a `COPY ... FROM stdin` data block.
//!   Stripping it as if it were one makes `sqlparser` consume the whole rest of the file as COPY
//!   payload and return **`Ok`**: the parse looks successful while every later statement silently
//!   vanishes. In the RFC's first measurement pass this turned 158 tables into 63 with no error
//!   anywhere, which is why [`preprocess_postgres_ddl`]'s fixture test asserts a statement *count*
//!   and not just that parsing succeeded.
//! * **A statement's leading keyword must be read past its comment header.** Deciding what a
//!   statement is from its first characters misses every statement carrying a `--` banner, which in
//!   a hand-written schema is most of them. See [`leading_keyword`].

use ekos_sql_dialect_sdk::SqlDialectParser;
use ekos_sql_dialect_sdk::lex::{self, skip_non_code};
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

/// Runs every preprocessing pass, in order. Ordering is load-bearing in three places:
///
/// 1. **`COPY ... FROM stdin` blocks go first** (RFC 0146 P9). Their payload rows are not SQL at
///    all — a row may contain an unbalanced `'` or `$` that would desynchronise every scanner
///    downstream — so they are removed line-wise before anything tries to read the text as SQL.
/// 2. **psql meta-command lines go second** (P4), after `COPY` blocks have already consumed the
///    `\.` terminators that belong to them. See this module's header for why stripping `\.`
///    blind is catastrophic and silent.
/// 3. **Whole-statement removals precede clause-level edits.** Clause edits (`UNLOGGED`,
///    `NOT VALID`, `INHERITS`, `SECURITY`, `SETOF`, `:=`) only need to consider the statements
///    that actually survive.
fn preprocess_postgres_ddl(sql: &str) -> String {
    // Line-oriented passes (RFC 0146) — must run before any statement-level scanning.
    let sql = strip_copy_from_stdin_blocks(sql);
    let sql = strip_psql_meta_command_lines(&sql);

    // Whole-statement removals. RFC 0059's two first, then RFC 0146's.
    let sql = strip_statements_starting_with(&sql, &["CREATE", "SEQUENCE"]);
    let sql = strip_statements_starting_with(&sql, &["ALTER", "SEQUENCE"]);
    let sql = strip_unparseable_statements(&sql);

    // Clause-level edits, on whatever statements remain.
    let sql = strip_unlogged_before_table(&sql);
    let sql = strip_not_valid_clause(&sql);
    let sql = strip_inherits_clause(&sql);
    let sql = strip_security_clause(&sql);
    let sql = strip_setof_after_returns(&sql);
    rewrite_named_argument_assignment(&sql)
}

// ── RFC 0146: line-oriented passes ──────────────────────────────────────────────────────────

/// Removes each `COPY ... FROM stdin;` statement together with the inline data block that follows
/// it, up to and including the lone `\.` terminator (RFC 0146 P9).
///
/// The header and the payload must go as one unit. `sqlparser` has no grammar for the payload
/// rows, and they are not SQL in any sense — tab-separated values that routinely contain `'`, `$`
/// and `;`, any of which would desynchronise the quote-tracking scanners the later passes rely on.
/// Seed data is not a KIR fact (only `Table`/`Column`/FK are), so nothing already captured is lost.
///
/// If a `COPY ... FROM stdin` is never terminated by `\.` the rest of the input is dropped, which
/// mirrors what Postgres itself would do with such a file.
fn strip_copy_from_stdin_blocks(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut skipping = false;

    for line in sql.split_inclusive('\n') {
        let trimmed = line.trim();
        if skipping {
            if trimmed == "\\." {
                skipping = false;
            }
            push_blank_line_for(line, &mut out);
            continue;
        }
        if is_copy_from_stdin_header(trimmed) {
            skipping = true;
            push_blank_line_for(line, &mut out);
            continue;
        }
        out.push_str(line);
    }

    out
}

/// True for a `COPY <table> [(cols)] FROM stdin [WITH ...];` header — the inline-data form.
/// `COPY ... FROM '/path/file'` (a server-side file read) has no payload block and is left alone.
fn is_copy_from_stdin_header(trimmed_line: &str) -> bool {
    let upper = trimmed_line.to_uppercase();
    upper.starts_with("COPY ") && upper.contains(" FROM STDIN")
}

/// Removes psql client meta-command lines — `\echo`, `\set`, `\copy`, `\i`, `\connect` — which are
/// `psql`'s own scripting syntax and not part of any SQL grammar (RFC 0146 P4).
///
/// **`\.` is deliberately excluded.** It is not a meta-command: it terminates a `COPY ... FROM
/// stdin` payload, and [`strip_copy_from_stdin_blocks`] (which runs first) owns it. Removing it
/// here instead makes `sqlparser` swallow the remainder of the file as COPY payload and return
/// `Ok` with every later statement silently gone — see this module's header.
fn strip_psql_meta_command_lines(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());

    for line in sql.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with('\\') && line.trim() != "\\." {
            push_blank_line_for(line, &mut out);
            continue;
        }
        out.push_str(line);
    }

    out
}

/// Pushes a replacement for a removed line that preserves the original line count, so `sqlparser`
/// error messages and any `SourceLocation` derived later still point at the right line of the
/// user's actual file.
fn push_blank_line_for(removed: &str, out: &mut String) {
    if removed.ends_with('\n') {
        out.push('\n');
    }
}

// ── RFC 0146: whole-statement removal ───────────────────────────────────────────────────────

/// Drops whole statements `sqlparser 0.53` cannot parse and EKOS models no facts from
/// (RFC 0146 P2/P3/P5/P10):
///
/// | Statement | Why it is dropped rather than repaired |
/// |---|---|
/// | `COMMENT ON ...` | Only a fixed object-type set is known (`FUNCTION`/`VIEW`/`TYPE`/`INDEX`/… are not), and dollar-quoted bodies are rejected outright even for `TABLE`/`COLUMN`. |
/// | `DO $$ ... $$` | An anonymous PL/pgSQL block; there is no PL/pgSQL grammar. |
/// | `CREATE [OR REPLACE] RULE ...` | No grammar at all; rules are not a KIR fact. |
/// | `ALTER TABLE ... NO INHERIT ...` | Only the inheritance link is altered, and inheritance is not modeled. |
///
/// `COMMENT ON TABLE`/`COLUMN` carry real author-written schema documentation. Dropping them loses
/// nothing **today** — `sql_analyzer` does not model `COMMENT ON` at all — but RFC 0146 Phase 2
/// will extract their text into `description` properties *before* this pass removes them.
///
/// Statement-boundary aware (so a `DO` inside `CREATE RULE ... DO INSTEAD`, or the word `COMMENT`
/// inside a function body, is never mistaken for a statement start) and comment-header aware via
/// [`leading_keyword`].
fn strip_unparseable_statements(sql: &str) -> String {
    edit_statements(sql, |statement| {
        let keyword = leading_keyword(statement);
        let drop = keyword.starts_with("COMMENT ON")
            || keyword == "DO"
            || keyword.starts_with("DO ")
            || keyword.starts_with("CREATE RULE")
            || keyword.starts_with("CREATE OR REPLACE RULE")
            || (keyword.starts_with("ALTER TABLE") && contains_no_inherit(statement));
        if drop {
            None
        } else {
            Some(statement.to_string())
        }
    })
}

/// True if the statement carries a top-level `NO INHERIT` clause (outside strings/comments).
fn contains_no_inherit(statement: &str) -> bool {
    let mut found = false;
    // The rewritten copy is discarded; `scan_code` is used here only for its quote-, comment- and
    // dollar-quote-aware traversal, so `NO INHERIT` inside a string or body cannot match.
    let _ = scan_code(statement, |chars, i, _out| {
        if is_word_boundary_match_ci(chars, i, "NO") {
            let mut j = i + 2;
            let ws_start = j;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j > ws_start && is_word_boundary_match_ci(chars, j, "INHERIT") {
                found = true;
            }
        }
        None
    });
    found
}

// ── RFC 0146: clause-level edits ────────────────────────────────────────────────────────────

/// Removes a `CREATE TABLE ... INHERITS (parent[, ...])` clause, keeping the table (RFC 0146 P1).
///
/// `INHERITS` is the only construct measured on LedgerSMB that breaks a `CREATE TABLE` statement
/// *itself* rather than by failing the file around it — 24 of the 26 tables that fail to parse
/// individually. Postgres table inheritance is not represented in EKOS's KIR (no
/// `RelationshipKind` models it), so removing the clause loses nothing already captured, while
/// stripping the whole statement would discard a real user table with real columns — the same
/// reasoning as [`strip_unlogged_before_table`].
fn strip_inherits_clause(sql: &str) -> String {
    scan_code(sql, |chars, i, out| {
        if !is_word_boundary_match_ci(chars, i, "INHERITS") {
            return None;
        }
        let mut j = i + "INHERITS".chars().count();
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if chars.get(j) != Some(&'(') {
            return None;
        }
        let close = match matching_paren(chars, j) {
            Some(c) => c,
            None => return None,
        };
        while matches!(out.chars().last(), Some(' ') | Some('\t')) {
            out.pop();
        }
        Some(close + 1)
    })
}

/// Removes a `SECURITY DEFINER` / `SECURITY INVOKER` clause from `CREATE FUNCTION` (RFC 0146 P6).
///
/// Real Postgres grammar that `sqlparser 0.53` has no case for: it reports `Expected: end of
/// statement, found: SECURITY`. The clause selects the privilege context the function executes
/// under and says nothing about the schema; nothing in the KIR represents it. Keeping the rest of
/// the `CREATE FUNCTION` intact matters because `sql_transform_analyzer` dispatches on
/// `Statement::CreateFunction` to build the Transformation IR.
fn strip_security_clause(sql: &str) -> String {
    scan_code(sql, |chars, i, out| {
        if !is_word_boundary_match_ci(chars, i, "SECURITY") {
            return None;
        }
        let mut j = i + "SECURITY".chars().count();
        let ws_start = j;
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if j == ws_start {
            return None;
        }
        for kind in ["DEFINER", "INVOKER"] {
            if is_word_boundary_match_ci(chars, j, kind) {
                while matches!(out.chars().last(), Some(' ') | Some('\t')) {
                    out.pop();
                }
                return Some(j + kind.chars().count());
            }
        }
        None
    })
}

/// Rewrites `RETURNS SETOF <type>` to `RETURNS <type>` (RFC 0146 P7).
///
/// `sqlparser 0.53` has no `SETOF` grammar and reports `Expected: end of statement, found: <type>`.
/// Dropping just the keyword preserves the return type itself; only the set-ness is lost, and
/// nothing in the KIR distinguishes a set-returning function from a scalar one.
fn strip_setof_after_returns(sql: &str) -> String {
    scan_code(sql, |chars, i, out| {
        if !is_word_boundary_match_ci(chars, i, "RETURNS") {
            return None;
        }
        let mut j = i + "RETURNS".chars().count();
        let ws_start = j;
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if j == ws_start || !is_word_boundary_match_ci(chars, j, "SETOF") {
            return None;
        }
        // Copy the source's own `RETURNS` rather than a literal, so a lowercase `returns` in the
        // user's file stays lowercase. The whitespace that followed `SETOF` is still ahead of the
        // scanner and gets copied through, so no separator is emitted here.
        out.extend(&chars[i..i + "RETURNS".chars().count()]);
        Some(j + "SETOF".chars().count())
    })
}

/// Rewrites Postgres named call arguments `f(x := 1)` to `f(x => 1)` (RFC 0146 P8).
///
/// Postgres accepts both spellings; `sqlparser 0.53` knows only `=>` and reports
/// `Expected: ), found: :=`. [`scan_code`] keeps this out of dollar-quoted bodies, which is
/// essential: `v := expr;` is also PL/pgSQL's assignment operator, and rewriting it there would
/// corrupt function-body text that `sql_transform_analyzer` reads back into the Transformation IR.
fn rewrite_named_argument_assignment(sql: &str) -> String {
    scan_code(sql, |chars, i, out| {
        if chars[i] == ':' && chars.get(i + 1) == Some(&'=') {
            out.push_str("=>");
            return Some(i + 2);
        }
        None
    })
}

// ── RFC 0146: shared scanning primitives ────────────────────────────────────────────────────

/// Index of the `)` matching the `(` at `open`, or `None` if unbalanced. Depth-aware and blind to
/// parentheses inside string literals, quoted identifiers, dollar-quoted blocks and comments.
fn matching_paren(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < chars.len() {
        if let Some(next) = skip_non_code(chars, i) {
            i = next;
            continue;
        }
        match chars[i] {
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

/// Copies `sql`, giving `on_code` a chance to consume input at every position that is real SQL
/// code — never inside a string literal, quoted identifier, dollar-quoted block or comment, whose
/// contents are copied through verbatim.
///
/// `on_code(chars, i, out)` returns `Some(next_index)` when it has handled the input at `i`
/// (writing any replacement to `out` itself), or `None` to let the character be copied normally.
fn scan_code<F>(sql: &str, mut on_code: F) -> String
where
    F: FnMut(&[char], usize, &mut String) -> Option<usize>,
{
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;

    while i < chars.len() {
        if let Some(next) = skip_non_code(&chars, i) {
            out.extend(&chars[i..next]);
            i = next;
            continue;
        }
        if let Some(next) = on_code(&chars, i, &mut out) {
            i = next;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }

    out
}

/// Splits `sql` into top-level statements, applies `edit` to each, and reassembles the result.
///
/// `edit` returns `Some(replacement)` to keep a statement (possibly rewritten) or `None` to drop
/// it. A dropped statement is replaced by as many newlines as it contained, so the line numbers of
/// everything after it — and therefore any `sqlparser` error message read against the user's real
/// file — stay correct.
///
/// Boundaries are found by [`skip_non_code`], so a `;` inside a dollar-quoted PL/pgSQL body or a
/// comment never ends a statement. This is the guarantee `sql.split(';')` cannot make.
fn edit_statements<F>(sql: &str, mut edit: F) -> String
where
    F: FnMut(&str) -> Option<String>,
{
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut start = 0;
    let mut i = 0;

    let flush = |from: usize, to: usize, out: &mut String, edit: &mut F| {
        if from >= to {
            return;
        }
        let statement: String = chars[from..to].iter().collect();
        match edit(&statement) {
            Some(kept) => out.push_str(&kept),
            None => out.extend(statement.chars().filter(|c| *c == '\n')),
        }
    };

    while i < chars.len() {
        if let Some(next) = skip_non_code(&chars, i) {
            i = next;
            continue;
        }
        if chars[i] == ';' {
            flush(start, i + 1, &mut out, &mut edit);
            start = i + 1;
        }
        i += 1;
    }
    flush(start, chars.len(), &mut out, &mut edit);

    out
}

/// A statement's leading keywords, uppercased, with leading whitespace, `--` line comments and
/// `/* */` block comments skipped.
///
/// Reading the first characters of a statement directly would misclassify every statement carrying
/// a comment banner — which in a hand-written schema is most of them. In the RFC 0146 measurements
/// a single `COMMENT ON ... IS $$...$$` sitting behind three `--` lines was enough to keep failing
/// the whole-file parse after every other transform was already in place.
///
/// Returns at most the first few words, which is all any caller needs to classify a statement.
fn leading_keyword(statement: &str) -> String {
    let chars: Vec<char> = statement.chars().collect();
    let mut i = 0;

    loop {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let is_comment = (chars.get(i) == Some(&'-') && chars.get(i + 1) == Some(&'-'))
            || (chars.get(i) == Some(&'/') && chars.get(i + 1) == Some(&'*'));
        if !is_comment {
            break;
        }
        match skip_non_code(&chars, i) {
            Some(next) if next > i => i = next,
            _ => break,
        }
    }

    let rest: String = chars[i.min(chars.len())..].iter().collect();
    rest.split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
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
    lex::is_ident_char(c)
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
    is_at_word_boundary(chars, start, word)
}

/// Case-insensitive [`is_word_boundary_match`], for RFC 0146's passes.
///
/// The RFC 0059 passes above are case-sensitive because `pg_dump` emits uppercase keywords
/// exclusively. Hand-written schemas do not: the LedgerSMB measurements found both `INHERITS`
/// (15 statements) and `inherits` (10), `SECURITY DEFINER` (25) and `security definer` (12). A
/// case-sensitive matcher silently handles some of a project's tables and not others, which is
/// exactly the kind of half-working recovery RFC 0146 exists to eliminate.
fn is_word_boundary_match_ci(chars: &[char], start: usize, word: &str) -> bool {
    let wchars: Vec<char> = word.chars().collect();
    let end = start + wchars.len();
    if end > chars.len() {
        return false;
    }
    let matches = chars[start..end]
        .iter()
        .zip(wchars.iter())
        .all(|(a, b)| a.eq_ignore_ascii_case(b));
    matches && is_at_word_boundary(chars, start, word)
}

/// Shared boundary check: neither the character before `start` nor the one after the word is an
/// identifier character.
fn is_at_word_boundary(chars: &[char], start: usize, word: &str) -> bool {
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

    // ── RFC 0146: hand-written schema constructs ───────────────────────────────────────────

    /// Parses `sql` after preprocessing, asserting success, and returns the statements.
    fn parse_preprocessed(sql: &str) -> Vec<sqlparser::ast::Statement> {
        let preprocessed = PostgresDialectParser.preprocess(sql);
        let dialect = PostgresDialectParser.sqlparser_dialect();
        match Parser::parse_sql(&*dialect, &preprocessed) {
            Ok(statements) => statements,
            Err(e) => {
                panic!("expected preprocessed SQL to parse, got {e}\n--- input ---\n{preprocessed}")
            }
        }
    }

    fn create_table_count(statements: &[sqlparser::ast::Statement]) -> usize {
        statements
            .iter()
            .filter(|s| matches!(s, sqlparser::ast::Statement::CreateTable(_)))
            .count()
    }

    // P1 — INHERITS

    #[test]
    fn strip_inherits_clause_keeps_the_table() {
        let sql =
            "CREATE TABLE account_translation (PRIMARY KEY (trans_id)) INHERITS (translation);";
        assert_eq!(
            strip_inherits_clause(sql),
            "CREATE TABLE account_translation (PRIMARY KEY (trans_id));"
        );
    }

    #[test]
    fn strip_inherits_clause_handles_multiple_parents() {
        let sql = "CREATE TABLE t (id INT) INHERITS (a, b);";
        assert_eq!(strip_inherits_clause(sql), "CREATE TABLE t (id INT);");
    }

    #[test]
    fn inherits_inside_a_string_literal_is_not_touched() {
        let sql = "INSERT INTO notes (body) VALUES ('INHERITS (parent)');";
        assert_eq!(strip_inherits_clause(sql), sql);
    }

    #[test]
    fn postgres_dialect_parses_inherits_table_after_preprocessing() {
        let statements =
            parse_preprocessed("CREATE TABLE country_tax_form (id INT) INHERITS (tax_form);");
        assert_eq!(create_table_count(&statements), 1);
    }

    // P2 — ALTER TABLE ... NO INHERIT

    #[test]
    fn alter_table_no_inherit_is_dropped_and_neighbours_survive() {
        let sql = "CREATE TABLE a (id INT);\nalter table asset_note no inherit note;\nCREATE TABLE b (id INT);";
        let statements = parse_preprocessed(sql);
        assert_eq!(create_table_count(&statements), 2);
    }

    #[test]
    fn an_ordinary_alter_table_is_not_dropped() {
        let sql = "ALTER TABLE a ADD COLUMN x INT;";
        let statements = parse_preprocessed(sql);
        assert_eq!(statements.len(), 1, "expected the ALTER TABLE to survive");
    }

    // P3 — COMMENT ON

    #[test]
    fn comment_on_with_a_dollar_quoted_body_is_dropped() {
        let sql = "\
CREATE TABLE lsmb_module (id int not null unique);
COMMENT ON TABLE lsmb_module IS
$$ This stores categories functionality into modules.  Addons may add rows here; the
id should be hardcoded. $$;
CREATE TABLE language (code text primary key);";
        let statements = parse_preprocessed(sql);
        assert_eq!(
            create_table_count(&statements),
            2,
            "both tables must survive a COMMENT ON between them"
        );
    }

    #[test]
    fn comment_on_function_is_dropped() {
        let sql = "COMMENT ON FUNCTION menu_generate() IS $$ Returns the menu tree. $$;\nCREATE TABLE t (id INT);";
        assert_eq!(create_table_count(&parse_preprocessed(sql)), 1);
    }

    #[test]
    fn comment_on_is_dropped_even_behind_a_comment_banner() {
        // The exact shape that kept failing the whole-file parse in the RFC 0146 measurements
        // after every other transform was already in place.
        let sql = "\
CREATE TABLE a (id INT);
-- Moving this comment to SQL comments because it is about this code rather than
-- the database structure as API. --CT
-- This could probably be done better.
COMMENT ON TABLE a IS $$ Hardwired classifications for orders and quotations. $$;
CREATE TABLE b (id INT);";
        assert_eq!(create_table_count(&parse_preprocessed(sql)), 2);
    }

    #[test]
    fn a_semicolon_inside_a_dollar_quoted_comment_body_does_not_split_the_statement() {
        // If `;` inside `$$ ... $$` ended the statement, the tail would be left behind as
        // garbage and the following CREATE TABLE would be lost.
        let sql = "COMMENT ON TABLE a IS $$ first; second; third $$;\nCREATE TABLE b (id INT);";
        let preprocessed = PostgresDialectParser.preprocess(sql);
        assert!(
            !preprocessed.contains("second"),
            "the whole dollar-quoted body must be removed, got:\n{preprocessed}"
        );
        assert_eq!(create_table_count(&parse_preprocessed(sql)), 1);
    }

    // P4 / P9 — psql meta-commands and COPY ... FROM stdin

    #[test]
    fn psql_meta_command_lines_are_removed() {
        let sql = "\\echo 'loading'\n\\set foo 1\nCREATE TABLE t (id INT);";
        assert_eq!(create_table_count(&parse_preprocessed(sql)), 1);
    }

    /// The bug that turned 158 tables into 63 silently: `\.` is the COPY payload terminator, not a
    /// meta-command. Stripped as one, `sqlparser` eats the rest of the file as COPY data and still
    /// returns `Ok` — so this asserts the statement *count*, which is the only thing that catches
    /// it. See this crate's module header.
    #[test]
    fn copy_from_stdin_payload_does_not_swallow_the_rest_of_the_file() {
        let sql = "\
CREATE TABLE defaults (setting_key text primary key, value text);
COPY defaults FROM stdin WITH DELIMITER '|';
timeout|90 minutes
version|1.14.0-dev
curr|USD
\\.
CREATE TABLE account (id int primary key);
CREATE TABLE entity (id int primary key);";
        let statements = parse_preprocessed(sql);
        assert_eq!(
            create_table_count(&statements),
            3,
            "every table after the COPY payload must survive; got statements: {statements:?}"
        );
    }

    #[test]
    fn copy_from_a_file_path_is_not_treated_as_an_inline_payload() {
        // `COPY ... FROM '<path>'` has no payload block, so nothing after it may be swallowed.
        let sql =
            "\\copy blacklisted_funcs FROM 'sql/modules/BLACKLIST';\nCREATE TABLE t (id INT);";
        assert_eq!(create_table_count(&parse_preprocessed(sql)), 1);
    }

    // P5 / P10 — DO blocks and CREATE RULE

    #[test]
    fn do_block_is_dropped_and_neighbours_survive() {
        let sql = "\
CREATE TABLE a (id INT);
DO $$
DECLARE f record;
BEGIN
  FOR f IN SELECT 1 LOOP
    RAISE NOTICE 'x';
  END LOOP;
END;
$$;
CREATE TABLE b (id INT);";
        assert_eq!(create_table_count(&parse_preprocessed(sql)), 2);
    }

    #[test]
    fn create_rule_is_dropped_and_neighbours_survive() {
        let sql = "\
CREATE TABLE a (id INT);
CREATE RULE file_sec_insert AS ON INSERT TO file_secondary_attachment
  WHERE source_class = 1 DO INSTEAD INSERT INTO file_tx_to_order(id) VALUES (1);
CREATE TABLE b (id INT);";
        assert_eq!(create_table_count(&parse_preprocessed(sql)), 2);
    }

    #[test]
    fn the_word_do_inside_another_statement_is_not_a_statement_start() {
        // `DO INSTEAD` lives inside CREATE RULE; `DO` must only match at a statement boundary,
        // or statement-level stripping would cut arbitrary statements in half.
        let sql = "CREATE TABLE a (id INT);\nINSERT INTO log (msg) VALUES ('nothing to do here');";
        let preprocessed = PostgresDialectParser.preprocess(sql);
        assert!(
            preprocessed.contains("nothing to do here"),
            "a `do` inside a literal must survive:\n{preprocessed}"
        );
    }

    // P6 — SECURITY DEFINER / INVOKER

    #[test]
    fn strip_security_clause_removes_definer_and_invoker() {
        assert_eq!(
            strip_security_clause(
                "CREATE FUNCTION f() RETURNS INT AS $$ 1 $$ LANGUAGE sql SECURITY DEFINER;"
            ),
            "CREATE FUNCTION f() RETURNS INT AS $$ 1 $$ LANGUAGE sql;"
        );
        assert_eq!(
            strip_security_clause(
                "CREATE FUNCTION f() RETURNS INT AS $$ 1 $$ LANGUAGE sql SECURITY INVOKER;"
            ),
            "CREATE FUNCTION f() RETURNS INT AS $$ 1 $$ LANGUAGE sql;"
        );
    }

    /// Hand-written schemas mix keyword case; `pg_dump` never does. LedgerSMB writes both
    /// `SECURITY DEFINER` (25 statements) and `security definer` (12).
    #[test]
    fn clause_strips_are_case_insensitive() {
        assert_eq!(
            strip_security_clause(
                "CREATE FUNCTION f() RETURNS INT AS $$ 1 $$ language plpgsql security definer;"
            ),
            "CREATE FUNCTION f() RETURNS INT AS $$ 1 $$ language plpgsql;"
        );
        assert_eq!(
            strip_inherits_clause(
                "create table account_translation (id int) inherits (translation);"
            ),
            "create table account_translation (id int);"
        );
        assert_eq!(
            strip_setof_after_returns("create function f() returns setof account as $$ x $$;"),
            "create function f() returns account as $$ x $$;"
        );
    }

    #[test]
    fn postgres_dialect_parses_security_definer_function_after_preprocessing() {
        let sql = "\
CREATE OR REPLACE FUNCTION eca_bu_trigger() RETURNS TRIGGER AS $$
BEGIN
  RETURN NEW;
END;
$$ LANGUAGE plpgsql SECURITY DEFINER;";
        let statements = parse_preprocessed(sql);
        assert_eq!(statements.len(), 1);
    }

    // P7 — RETURNS SETOF

    #[test]
    fn strip_setof_after_returns_keeps_the_return_type() {
        assert_eq!(
            strip_setof_after_returns("CREATE FUNCTION f() RETURNS SETOF account AS $$ x $$;"),
            "CREATE FUNCTION f() RETURNS account AS $$ x $$;"
        );
    }

    #[test]
    fn postgres_dialect_parses_returns_setof_after_preprocessing() {
        let sql = "CREATE OR REPLACE FUNCTION chart_list_all() RETURNS SETOF account AS\n$$ SELECT * FROM account ORDER BY accno; $$ LANGUAGE SQL;";
        assert_eq!(parse_preprocessed(sql).len(), 1);
    }

    // P8 — named argument assignment

    #[test]
    fn named_argument_assignment_is_rewritten_to_arrow() {
        assert_eq!(
            rewrite_named_argument_assignment("SELECT f(table_name_in := 'acc_trans');"),
            "SELECT f(table_name_in => 'acc_trans');"
        );
    }

    /// `:=` is also PL/pgSQL's assignment operator. Rewriting it inside a dollar-quoted body would
    /// corrupt function text that `sql_transform_analyzer` reads back into the Transformation IR.
    #[test]
    fn named_argument_rewrite_does_not_touch_a_plpgsql_body() {
        let sql = "CREATE FUNCTION f() RETURNS INT AS $$ DECLARE v INT; BEGIN v := 1; RETURN v; END; $$ LANGUAGE plpgsql;";
        assert_eq!(rewrite_named_argument_assignment(sql), sql);
    }

    #[test]
    fn postgres_dialect_parses_named_arguments_after_preprocessing() {
        let sql = "SELECT migrate_to_identity(table_name_in := 'acc_trans', column_name_in := 'entry_id');";
        assert_eq!(parse_preprocessed(sql).len(), 1);
    }

    // ── shared scanning primitives ─────────────────────────────────────────────────────────

    #[test]
    fn leading_keyword_skips_comment_banners() {
        assert_eq!(
            leading_keyword("  CREATE TABLE t (id INT)"),
            "CREATE TABLE T (ID"
        );
        assert_eq!(
            leading_keyword("--\n-- Name: t; Type: TABLE\n--\nCOMMENT ON TABLE a IS 'x'"),
            "COMMENT ON TABLE A"
        );
        assert_eq!(
            leading_keyword("/* block */ DO $$ BEGIN END; $$"),
            "DO $$ BEGIN END;"
        );
    }

    // `dollar_quote_tag_end` and `skip_non_code` moved to `ekos_sql_dialect_sdk::lex` so
    // `ekos-recovery`'s COMMENT ON extractor (RFC 0146 Phase 2) reads the same text these passes
    // strip. Their unit tests moved with them; what stays here is the behaviour that depends on
    // them holding, exercised through `preprocess` itself.

    #[test]
    fn edit_statements_preserves_line_numbers_of_later_statements() {
        let sql = "CREATE TABLE a (id INT);\nDO $$\nBEGIN\nEND;\n$$;\nCREATE TABLE b (id INT);";
        let out = PostgresDialectParser.preprocess(sql);
        assert_eq!(
            sql.matches('\n').count(),
            out.matches('\n').count(),
            "dropped statements must be replaced by their own newlines, got:\n{out}"
        );
    }

    #[test]
    fn edit_statements_does_not_split_on_a_semicolon_in_a_dollar_quoted_body() {
        let sql = "CREATE FUNCTION f() RETURNS INT AS $body$ BEGIN RETURN 1; END; $body$ LANGUAGE plpgsql;";
        assert_eq!(parse_preprocessed(sql).len(), 1);
    }

    // ── the whole thing, on a fixture built from real LedgerSMB shapes ─────────────────────

    /// Every construct RFC 0146 measured on LedgerSMB's `sql/Pg-database.sql`, in one file, in the
    /// shapes they actually appear in. Hand-authored rather than excerpted so no GPL source is
    /// vendored into this repo.
    const HAND_WRITTEN_SCHEMA_SQL: &str = include_str!("../tests/fixtures/hand-written-schema.sql");

    #[test]
    fn postgres_dialect_parses_the_hand_written_schema_fixture_after_preprocessing() {
        let statements = parse_preprocessed(HAND_WRITTEN_SCHEMA_SQL);

        // A count, not just `is_ok()`: the `\.` bug this fixture guards against produces a
        // *successful* parse that silently drops everything after the COPY payload.
        assert_eq!(
            create_table_count(&statements),
            9,
            "expected all 9 tables, got {} — statements: {statements:#?}",
            create_table_count(&statements)
        );
    }

    #[test]
    fn the_hand_written_schema_fixture_keeps_every_table_name() {
        let statements = parse_preprocessed(HAND_WRITTEN_SCHEMA_SQL);
        let rendered = format!("{statements:?}");
        for table in [
            "lsmb_module",
            "language",
            "account",
            "translation",
            "account_translation",
            "defaults",
            "entity",
            "asset_note",
            "payroll_wage",
        ] {
            assert!(
                rendered.contains(table),
                "table {table} must survive preprocessing"
            );
        }
    }
}
