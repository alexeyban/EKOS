//! Trino `SqlDialectParser` (RFC 0190 M0).
//!
//! `sqlparser` 0.53 has no Trino dialect. Its `GenericDialect` parses the Trino that dbt-trino
//! models and the Lakehouse generators write — catalog-qualified names, `WITH (...)` table
//! properties, lambdas, `ROW` types, `MERGE`, `CALL` — except three constructs, measured against
//! every candidate base dialect (generic, ansi, hive, postgres, duckdb) before this crate was
//! written. None of them parses anywhere, so [`TrinoDialectParser::preprocess`] blanks them out:
//!
//! * `FOR TIMESTAMP AS OF <expr>` / `FOR VERSION AS OF <expr>` (Iceberg time travel) after a table
//!   reference. The table itself stays, so lineage still sees what the query reads.
//! * `ALTER TABLE <t> EXECUTE <procedure>(...)` (Iceberg maintenance: `optimize`,
//!   `expire_snapshots`, `remove_orphan_files`), the whole statement. It changes files, not
//!   schema or data, so nothing is lost for recovery.
//! * `TABLESAMPLE BERNOULLI|SYSTEM (<n>)`.
//!
//! Blanking replaces every character except line breaks with a space, so line numbers in
//! diagnostics and evidence stay the file's own (the same rule RFC 0176 uses).

use ekos_sql_dialect_sdk::SqlDialectParser;
use ekos_sql_dialect_sdk::lex::{is_ident_char, skip_non_code, statement_spans};
use sqlparser::dialect::{Dialect, GenericDialect};

pub struct TrinoDialectParser;

impl SqlDialectParser for TrinoDialectParser {
    fn name(&self) -> &str {
        "trino"
    }

    fn sqlparser_dialect(&self) -> Box<dyn Dialect + Send + Sync> {
        Box::new(GenericDialect {})
    }

    fn preprocess(&self, sql: &str) -> String {
        let mut chars: Vec<char> = sql.chars().collect();
        for (start, end) in statement_spans(&chars) {
            let toks = tokens(&chars[..end], start);
            if is_alter_execute(&toks) {
                blank(&mut chars, start, end);
                continue;
            }
            blank_clauses(&mut chars, &toks, end);
        }
        chars.into_iter().collect()
    }
}

/// One code token: a word (identifier or keyword, upper-cased in `word`) or a single symbol.
/// Strings, quoted identifiers and comments are skipped, never tokens, so a keyword inside a
/// literal can never trigger a rewrite.
#[derive(Debug)]
struct Tok {
    start: usize,
    end: usize,
    word: Option<String>,
    symbol: char,
}

fn tokens(chars: &[char], from: usize) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = from;
    while i < chars.len() {
        if let Some(next) = skip_non_code(chars, i) {
            // A quoted identifier is still a table or column reference; keep it as a word so
            // a clause that follows it is found.
            if chars[i] == '"' || chars[i] == '`' {
                out.push(Tok {
                    start: i,
                    end: next,
                    word: Some(String::new()),
                    symbol: ' ',
                });
            }
            i = next;
        } else if chars[i].is_whitespace() {
            i += 1;
        } else if is_ident_char(chars[i]) {
            let s = i;
            while i < chars.len() && is_ident_char(chars[i]) {
                i += 1;
            }
            out.push(Tok {
                start: s,
                end: i,
                word: Some(chars[s..i].iter().collect::<String>().to_uppercase()),
                symbol: ' ',
            });
        } else {
            out.push(Tok {
                start: i,
                end: i + 1,
                word: None,
                symbol: chars[i],
            });
            i += 1;
        }
    }
    out
}

fn is_word(t: &Tok, w: &str) -> bool {
    t.word.as_deref() == Some(w)
}

/// `ALTER TABLE <name> EXECUTE`: the name may be dotted and quoted.
fn is_alter_execute(toks: &[Tok]) -> bool {
    if toks.len() < 4 || !is_word(&toks[0], "ALTER") || !is_word(&toks[1], "TABLE") {
        return false;
    }
    let mut i = 2;
    while i < toks.len() && (toks[i].word.is_some() || toks[i].symbol == '.') {
        if is_word(&toks[i], "EXECUTE") && i > 2 {
            return true;
        }
        i += 1;
    }
    false
}

/// Words that end a time-travel expression at parenthesis depth 0.
const CLAUSE_END: &[&str] = &[
    "WHERE",
    "JOIN",
    "LEFT",
    "RIGHT",
    "INNER",
    "FULL",
    "CROSS",
    "ON",
    "USING",
    "GROUP",
    "ORDER",
    "LIMIT",
    "OFFSET",
    "FETCH",
    "HAVING",
    "WINDOW",
    "UNION",
    "EXCEPT",
    "INTERSECT",
    "AS",
    "TABLESAMPLE",
];

fn blank_clauses(chars: &mut [char], toks: &[Tok], stmt_end: usize) {
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        // FOR TIMESTAMP|VERSION AS OF <expr>
        if is_word(t, "FOR")
            && toks.get(i + 1).is_some_and(|n| {
                is_word(n, "TIMESTAMP") || is_word(n, "VERSION") || is_word(n, "SYSTEM_TIME")
            })
            && toks.get(i + 2).is_some_and(|n| is_word(n, "AS"))
            && toks.get(i + 3).is_some_and(|n| is_word(n, "OF"))
        {
            let mut j = i + 4;
            let mut depth = 0i32;
            // The expression's first token is never a clause end (`TIMESTAMP '...'`).
            let mut first = true;
            while j < toks.len() {
                let u = &toks[j];
                match u.symbol {
                    '(' => depth += 1,
                    ')' if depth == 0 => break,
                    ')' => depth -= 1,
                    ',' | ';' if depth == 0 => break,
                    _ => {}
                }
                if depth == 0
                    && !first
                    && u.word.as_deref().is_some_and(|w| CLAUSE_END.contains(&w))
                {
                    break;
                }
                first = false;
                j += 1;
            }
            let end = toks.get(j).map_or(stmt_end, |u| u.start);
            blank(chars, t.start, end);
            i = j;
            continue;
        }
        // TABLESAMPLE BERNOULLI|SYSTEM ( ... )
        if is_word(t, "TABLESAMPLE")
            && toks
                .get(i + 1)
                .is_some_and(|n| is_word(n, "BERNOULLI") || is_word(n, "SYSTEM"))
            && toks.get(i + 2).is_some_and(|n| n.symbol == '(')
        {
            let mut j = i + 2;
            let mut depth = 0i32;
            while j < toks.len() {
                match toks[j].symbol {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            let end = toks.get(j).map_or(stmt_end, |u| u.end);
            blank(chars, t.start, end);
            i = j + 1;
            continue;
        }
        i += 1;
    }
}

/// Spaces over `[start, end)`, keeping line breaks.
fn blank(chars: &mut [char], start: usize, end: usize) {
    for c in &mut chars[start..end] {
        if *c != '\n' && *c != '\r' {
            *c = ' ';
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::parser::Parser;

    fn parses(sql: &str) -> bool {
        let p = TrinoDialectParser;
        Parser::parse_sql(&*p.sqlparser_dialect(), &p.preprocess(sql)).is_ok()
    }

    #[test]
    fn name_is_trino() {
        assert_eq!(TrinoDialectParser.name(), "trino");
    }

    /// What the Lakehouse generators and dbt-trino models write, plus Trino's own syntax that
    /// `GenericDialect` was measured to handle.
    #[test]
    fn trino_statements_parse() {
        for sql in [
            "CREATE TABLE iceberg.bronze.orders (id BIGINT, ts TIMESTAMP(6) WITH TIME ZONE, \
             amount DECIMAL(12,2)) WITH (format = 'PARQUET', partitioning = ARRAY['day(ts)'])",
            "CREATE TABLE iceberg.bronze.t WITH (format = 'PARQUET') AS SELECT * FROM postgresql.public.t",
            "INSERT INTO iceberg.bronze.t SELECT *, current_timestamp AS _ekos_loaded_at \
             FROM postgresql.public.t WHERE updated_at > TIMESTAMP '2026-01-01 00:00:00'",
            "ALTER TABLE iceberg.bronze.t ADD COLUMN c BIGINT",
            "ALTER TABLE iceberg.bronze.t ALTER COLUMN c SET DATA TYPE BIGINT",
            "SELECT transform(arr, x -> x + 1), try_cast(a AS INTEGER), \
             CAST(ROW(1, 'a') AS ROW(x INTEGER, y VARCHAR)) FROM t",
            "SELECT to_hex(sha256(to_utf8(CAST(a AS VARCHAR)))) FROM t",
            "MERGE INTO iceberg.silver.t AS t USING iceberg.bronze.s AS s ON t.id = s.id \
             WHEN MATCHED THEN UPDATE SET v = s.v WHEN NOT MATCHED THEN INSERT (id, v) VALUES (s.id, s.v)",
            "SELECT id, row_number() OVER (PARTITION BY id ORDER BY _ekos_loaded_at DESC) AS rn FROM t",
            "SELECT * FROM UNNEST(ARRAY[1,2]) AS u(x)",
            "CALL iceberg.system.rollback_to_snapshot('bronze', 't', 1)",
            "SELECT x FROM t WHERE ts > current_timestamp - INTERVAL '1' DAY",
            "SELECT a['k'], arr[1] FROM t",
            "SELECT * FROM iceberg.bronze.\"t$snapshots\"",
        ] {
            assert!(parses(sql), "{sql}");
        }
    }

    #[test]
    fn time_travel_is_removed_and_the_table_kept() {
        for sql in [
            "SELECT * FROM iceberg.bronze.t FOR TIMESTAMP AS OF TIMESTAMP '2026-01-01 00:00:00 UTC'",
            "SELECT * FROM iceberg.bronze.t FOR VERSION AS OF 123456",
            "SELECT * FROM t FOR VERSION AS OF 1 JOIN u FOR TIMESTAMP AS OF current_timestamp - \
             INTERVAL '1' DAY ON t.id = u.id WHERE t.x > 0",
            "SELECT * FROM (SELECT * FROM t FOR VERSION AS OF 7) s",
            "SELECT * FROM t FOR TIMESTAMP AS OF CAST('2026-01-01' AS TIMESTAMP(6)), u",
        ] {
            assert!(parses(sql), "{sql}");
            let out = TrinoDialectParser.preprocess(sql);
            assert!(!out.contains("AS OF"), "{out}");
            assert!(out.contains(" t ") || out.contains(".t "), "{out}");
        }
        let out = TrinoDialectParser
            .preprocess("SELECT * FROM t FOR VERSION AS OF 1 JOIN u ON t.id = u.id WHERE t.x > 0");
        assert!(out.contains("JOIN u ON t.id = u.id WHERE t.x > 0"), "{out}");
    }

    #[test]
    fn maintenance_statements_are_blanked_whole() {
        let sql = "CREATE TABLE a (x INT);\nALTER TABLE iceberg.bronze.\"t\" EXECUTE optimize(\n  \
                   file_size_threshold => '128MB');\nALTER TABLE b EXECUTE expire_snapshots(retention_threshold => '7d');\nSELECT 1;";
        let out = TrinoDialectParser.preprocess(sql);
        assert!(!out.contains("EXECUTE"), "{out}");
        assert!(out.contains("CREATE TABLE a (x INT);") && out.contains("SELECT 1;"));
        assert!(parses(sql), "{out}");
        // Ordinary ALTER TABLE is untouched.
        let alter = "ALTER TABLE t ADD COLUMN execute_count BIGINT";
        assert_eq!(TrinoDialectParser.preprocess(alter), alter);
    }

    #[test]
    fn tablesample_is_removed() {
        let sql = "SELECT a FROM t TABLESAMPLE BERNOULLI (10) WHERE a > 1";
        assert!(parses(sql));
        assert!(TrinoDialectParser.preprocess(sql).contains("FROM t "));
    }

    #[test]
    fn line_numbers_and_length_are_kept() {
        let sql = "SELECT *\nFROM t FOR VERSION AS OF\n  42\nWHERE x = 1;\nALTER TABLE t EXECUTE\noptimize;\n";
        let out = TrinoDialectParser.preprocess(sql);
        assert_eq!(out.chars().count(), sql.chars().count());
        assert_eq!(out.lines().count(), sql.lines().count());
        assert!(out.lines().nth(3).unwrap().contains("WHERE x = 1"));
    }

    #[test]
    fn keywords_inside_literals_and_comments_are_left_alone() {
        for sql in [
            "SELECT 'FOR VERSION AS OF 1' FROM t",
            "SELECT 1 -- FOR VERSION AS OF 1\nFROM t",
            "SELECT \"TABLESAMPLE\" FROM t",
            "SELECT 'ALTER TABLE t EXECUTE optimize'",
        ] {
            assert_eq!(TrinoDialectParser.preprocess(sql), sql);
        }
    }
}
