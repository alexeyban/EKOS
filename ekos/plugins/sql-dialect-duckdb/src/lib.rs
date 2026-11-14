//! DuckDB `SqlDialectParser` (RFC 0259 D1).
//!
//! Parses with `sqlparser`'s `DuckDbDialect`, which reads what dbt-duckdb models and DuckDB's own
//! documentation examples write — `GROUP BY ALL`, `ORDER BY ALL`, `SELECT * EXCLUDE (…)` /
//! `REPLACE (…)`, `QUALIFY`, list and struct literals, `::` casts, lambdas, `COLUMNS(…)`,
//! `POSITIONAL`/`ASOF`/`ANTI`/`SEMI` joins, `PIVOT (…)` after a table, `INSERT OR REPLACE`,
//! `CREATE SECRET`, `INSTALL`/`LOAD`, `COPY … TO`, `date_trunc`, `strftime`. Measured before this
//! crate was written on 116 statements (RFC 0259's implementation log): 89 parse unaided, 98 after
//! the rewrites below. Three constructs did not parse and matter for recovery, and [`DuckDbDialectParser::preprocess`] blanks them out:
//!
//! * a sample clause after a table — `USING SAMPLE 10%`, `USING SAMPLE reservoir(50 ROWS)
//!   REPEATABLE (100)`, `TABLESAMPLE 10%`, `TABLESAMPLE reservoir(20%)`. The table stays, so
//!   lineage still sees what the query reads; a sample changes which rows, not which columns.
//! * the materialization hint of a common table expression, `AS MATERIALIZED (` / `AS NOT
//!   MATERIALIZED (`: the hint word(s) only, the CTE stays. It is a plan hint, not meaning.
//! * the option list of `ATTACH '…' AS name (TYPE iceberg, ENDPOINT …, SECRET …)`: `sqlparser`
//!   reads only `TYPE` and `READ_ONLY` there. The attached name stays, so a catalog-qualified name
//!   later in the file still reads as a name.
//!
//! What still does not parse, and is reported per statement by the SQL analyzer's fallback (RFC
//! 0176): `FROM`-first queries (`FROM t SELECT x`, bare `FROM t`), the `PIVOT`/`UNPIVOT`/
//! `SUMMARIZE` statements, positional references (`#1`), list comprehensions, field access on a
//! struct literal, the `GLOB` operator, `LIMIT n%`, `**`, `E'…'` strings, Python-style `lambda x:`,
//! `SET GLOBAL`/`SET SESSION`, `CREATE TYPE … AS ENUM`, `EXPORT DATABASE` and `CHECKPOINT`. None
//! of them appears in what the lakehouse generates; reordering a `FROM`-first query would move
//! text between lines, which the line-preserving rule forbids.
//!
//! Blanking replaces every character except line breaks with a space, so line numbers in
//! diagnostics and evidence stay the file's own (the same rule RFC 0176 uses).

use ekos_sql_dialect_sdk::lex::{CodeToken, blank, closing_paren, code_tokens, statement_spans};
use ekos_sql_dialect_sdk::{DialectFacts, RowLimit, SqlDialectParser};
use sqlparser::dialect::{Dialect, DuckDbDialect};

pub struct DuckDbDialectParser;

impl SqlDialectParser for DuckDbDialectParser {
    fn name(&self) -> &str {
        "duckdb"
    }

    fn sqlparser_dialect(&self) -> Box<dyn Dialect + Send + Sync> {
        Box::new(DuckDbDialect {})
    }

    /// RFC 0242, measured on DuckDB 1.5.5: `7 / 2` is `3.5` (integer division is `//`); NULL
    /// sorts last ascending; `LIKE` is case-sensitive and `ILIKE` exists; `LIMIT` and `FETCH
    /// FIRST` both work; backticks are not identifiers.
    fn facts(&self) -> DialectFacts {
        DialectFacts {
            has_ilike: true,
            row_limit: RowLimit::LimitOrFetch,
            identifier_quote: Some('"'),
            ..DialectFacts::GENERIC
        }
    }

    fn preprocess(&self, sql: &str) -> String {
        let mut chars: Vec<char> = sql.chars().collect();
        for (start, end) in statement_spans(&chars) {
            let toks = code_tokens(&chars[..end], start);
            blank_attach_options(&mut chars, &toks);
            blank_clauses(&mut chars, &toks, end);
        }
        chars.into_iter().collect()
    }

    /// RFC 0243: how to write correct SQL for this engine.
    fn generation_notes(&self) -> &'static [&'static str] {
        &[
            "Limit rows with LIMIT n (FETCH FIRST n ROWS ONLY also works).",
            "/ always divides exactly (7 / 2 = 3.5); // is integer division.",
            "Concatenate with || (a NULL operand makes the result NULL) or concat(), which skips NULLs.",
            "= and LIKE are case-sensitive; use ILIKE, or lower(col) = lower('…').",
            "Dates: DATE '2024-01-31', date_trunc('month', ts), year(ts), date_diff('day', a, b), strftime(ts, '%Y-%m'), ts::DATE, INTERVAL 7 DAY.",
            "Quote names with double quotes; backticks are not accepted.",
            "NULL sorts last in ascending and in descending order: write NULLS FIRST when you need it.",
            "GROUP BY ALL groups by every non-aggregate column; count(*) FILTER (WHERE …) counts conditionally; QUALIFY filters on a window function.",
            "Read only the tables in the schema: table functions that read files or the network (read_csv, read_parquet, a quoted path in FROM) are refused.",
        ]
    }
}

/// `ATTACH '…' [AS name] (options)`: the option list, which `sqlparser` reads only for `TYPE` and
/// `READ_ONLY`.
fn blank_attach_options(chars: &mut [char], toks: &[CodeToken]) {
    if !toks.first().is_some_and(|t| t.is("ATTACH")) {
        return;
    }
    if let Some(open) = toks.iter().position(|t| t.symbol == '(') {
        let close = closing_paren(toks, open);
        if let Some(c) = toks.get(close) {
            blank(chars, toks[open].start, c.end);
        }
    }
}

/// Words that end a sample clause at parenthesis depth 0.
const CLAUSE_END: &[&str] = &[
    "WHERE",
    "JOIN",
    "LEFT",
    "RIGHT",
    "INNER",
    "FULL",
    "CROSS",
    "POSITIONAL",
    "ASOF",
    "ANTI",
    "SEMI",
    "NATURAL",
    "ON",
    "GROUP",
    "ORDER",
    "LIMIT",
    "OFFSET",
    "FETCH",
    "HAVING",
    "QUALIFY",
    "WINDOW",
    "UNION",
    "EXCEPT",
    "INTERSECT",
];

fn blank_clauses(chars: &mut [char], toks: &[CodeToken], stmt_end: usize) {
    let mut i = 0;
    while i < toks.len() {
        if let Some(from) = sample_start(toks, i) {
            let j = sample_end(toks, from);
            let end = toks.get(j).map_or(stmt_end, |u| u.start);
            blank(chars, toks[i].start, end);
            i = j;
        } else if let Some(n) = materialized_hint(toks, i) {
            blank(chars, toks[i + 1].start, toks[i + n].end);
            i += n + 1;
        } else {
            i += 1;
        }
    }
}

/// `USING SAMPLE` or `TABLESAMPLE` at token `i`: the index of the clause's first argument token.
fn sample_start(toks: &[CodeToken], i: usize) -> Option<usize> {
    if toks[i].is("TABLESAMPLE") {
        return Some(i + 1);
    }
    (toks[i].is("USING") && toks.get(i + 1).is_some_and(|n| n.is("SAMPLE"))).then_some(i + 2)
}

/// The token that ends a sample clause starting at `from`: a top-level `,` `;` or unmatched `)`,
/// or a clause keyword. `REPEATABLE (seed)` belongs to the clause.
fn sample_end(toks: &[CodeToken], from: usize) -> usize {
    let mut depth = 0i32;
    for (j, u) in toks.iter().enumerate().skip(from) {
        match u.symbol {
            '(' => depth += 1,
            ')' if depth == 0 => return j,
            ')' => depth -= 1,
            ',' | ';' if depth == 0 => return j,
            _ => {}
        }
        let clause_end = u.word.as_deref().is_some_and(|w| CLAUSE_END.contains(&w));
        if depth == 0 && clause_end {
            return j;
        }
    }
    toks.len()
}

/// `AS [NOT] MATERIALIZED (` at token `i`: how many hint words follow `AS`.
fn materialized_hint(toks: &[CodeToken], i: usize) -> Option<usize> {
    if !toks[i].is("AS") {
        return None;
    }
    let n = if toks.get(i + 1).is_some_and(|t| t.is("NOT")) {
        2
    } else {
        1
    };
    let hint = toks.get(i + n).is_some_and(|t| t.is("MATERIALIZED"))
        && toks.get(i + n + 1).is_some_and(|t| t.symbol == '(');
    hint.then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::parser::Parser;

    fn parses(sql: &str) -> bool {
        let p = DuckDbDialectParser;
        Parser::parse_sql(&*p.sqlparser_dialect(), &p.preprocess(sql)).is_ok()
    }

    #[test]
    fn name_is_duckdb() {
        assert_eq!(DuckDbDialectParser.name(), "duckdb");
    }

    /// DuckDB-only syntax `DuckDbDialect` reads unaided, as measured.
    #[test]
    fn duckdb_statements_parse() {
        for sql in [
            "SELECT customer_id, count(*) FROM orders GROUP BY ALL ORDER BY ALL",
            "SELECT * EXCLUDE (password, ssn) FROM users",
            "SELECT * REPLACE (lower(email) AS email) FROM users",
            "SELECT * FROM orders QUALIFY row_number() OVER (PARTITION BY id ORDER BY ts DESC) = 1",
            "SELECT [1, 2, 3] AS l, {'a': 1, 'b': 'x'} AS s, l[1], s.a, s['b'] FROM t",
            "SELECT amount::DECIMAL(18,2), created_at::DATE, x::INTEGER[] FROM t",
            "SELECT date_trunc('month', ts) AS m, strftime(ts, '%Y-%m') AS ym FROM t",
            "SELECT list_transform([1,2,3], x -> x + 1), struct_pack(a := 1)",
            "SELECT count(*) FILTER (WHERE x > 1), min(COLUMNS(*)) FROM t",
            "SELECT * FROM t1 ASOF JOIN t2 ON t1.id = t2.id AND t1.ts >= t2.ts",
            "SELECT * FROM t1 POSITIONAL JOIN t2",
            "SELECT * FROM t1 ANTI JOIN t2 USING (id)",
            "CREATE OR REPLACE TABLE marts.gold.f AS SELECT * FROM lh.silver.c_invoice",
            "CREATE TABLE t (id BIGINT PRIMARY KEY, tags VARCHAR[], m MAP(VARCHAR, INTEGER), \
             s STRUCT(a INT, b VARCHAR), h HUGEINT, u UUID, ts TIMESTAMPTZ)",
            "SELECT ts + INTERVAL 1 DAY, x ILIKE '%y%', x // 2 FROM t",
            "INSERT OR REPLACE INTO t SELECT * FROM u",
            "CREATE SECRET s3 (TYPE s3, KEY_ID 'k', SECRET 's', URL_STYLE 'path', USE_SSL false)",
            "INSTALL iceberg; LOAD httpfs; SET s3_endpoint = 'localhost:9000'",
            "COPY (SELECT 1) TO 'x.parquet' (FORMAT parquet)",
            "SELECT a, b, sum(c) FROM t GROUP BY ALL HAVING sum(c) > 0 \
             QUALIFY rank() OVER (ORDER BY sum(c)) < 5",
        ] {
            assert!(parses(sql), "{sql}");
        }
    }

    /// What dbt-duckdb writes around a model (RFC 0259 L2), and a gold model as the lakehouse
    /// drafts it.
    #[test]
    fn dbt_duckdb_materializations_parse() {
        for sql in [
            "create table \"marts\".\"gold\".\"fct_invoice__dbt_tmp\" as (\n  \
             with src as (select * from lh.silver.c_invoice where not isreversal)\n  \
             select docstatus, date_trunc('month', dateinvoiced)::date as month,\n    \
             count(*) as n, sum(grandtotal)::decimal(18,2) as total\n  from src group by all\n)",
            "alter table \"marts\".\"gold\".\"fct_invoice__dbt_tmp\" rename to \"fct_invoice\"",
            "drop table if exists \"marts\".\"gold\".\"fct_invoice__dbt_backup\" cascade",
            "create view \"marts\".\"gold\".\"v__dbt_tmp\" as (select 1 as x)",
            "ATTACH 'lakehouse' AS lh (TYPE iceberg, ENDPOINT 'http://polaris:8181/api/catalog', \
             SECRET iceberg_secret, ACCESS_DELEGATION_MODE 'none')",
        ] {
            assert!(parses(sql), "{sql}");
        }
    }

    #[test]
    fn sample_clauses_are_removed_and_the_table_kept() {
        for sql in [
            "SELECT x FROM t USING SAMPLE 10%",
            "SELECT x FROM t USING SAMPLE 100 ROWS WHERE x > 1",
            "SELECT * FROM t USING SAMPLE 10% (bernoulli) JOIN u ON t.id = u.id",
            "SELECT * FROM t USING SAMPLE reservoir(50 ROWS) REPEATABLE (100) ORDER BY 1",
            "SELECT x FROM t TABLESAMPLE 10%",
            "SELECT * FROM t TABLESAMPLE reservoir(20%), u",
            "SELECT * FROM (SELECT * FROM t USING SAMPLE 5) s",
        ] {
            assert!(parses(sql), "{sql}");
            let out = DuckDbDialectParser.preprocess(sql);
            assert!(!out.contains("SAMPLE"), "{out}");
            assert!(out.contains(" t "), "{out}");
        }
        let out = DuckDbDialectParser
            .preprocess("SELECT * FROM t USING SAMPLE 10% JOIN u USING (id) WHERE t.x > 0");
        assert!(out.contains("JOIN u USING (id) WHERE t.x > 0"), "{out}");
    }

    #[test]
    fn cte_materialization_hints_are_removed() {
        for sql in [
            "WITH a AS MATERIALIZED (SELECT 1) SELECT * FROM a",
            "WITH a AS NOT MATERIALIZED (SELECT 1), b AS (SELECT 2) SELECT * FROM a, b",
        ] {
            assert!(parses(sql), "{sql}");
            assert!(!DuckDbDialectParser.preprocess(sql).contains("MATERIALIZED"));
        }
        // A materialized view is not a hint.
        let view = "CREATE MATERIALIZED VIEW v AS SELECT 1";
        assert_eq!(DuckDbDialectParser.preprocess(view), view);
    }

    #[test]
    fn attach_keeps_its_name() {
        let out = DuckDbDialectParser
            .preprocess("ATTACH 'lakehouse' AS lh (TYPE iceberg, ENDPOINT 'e');\nSELECT 1;");
        assert!(out.starts_with("ATTACH 'lakehouse' AS lh "), "{out}");
        assert!(!out.contains("ENDPOINT"), "{out}");
        assert!(out.ends_with("\nSELECT 1;"), "{out}");
    }

    #[test]
    fn line_numbers_and_length_are_kept() {
        let sql = "SELECT *\nFROM t USING SAMPLE\n  10%\nWHERE x = 1;\nWITH a AS\nMATERIALIZED (SELECT 1) SELECT * FROM a;\n";
        let out = DuckDbDialectParser.preprocess(sql);
        assert_eq!(out.chars().count(), sql.chars().count());
        assert_eq!(out.lines().count(), sql.lines().count());
        assert!(out.lines().nth(3).unwrap().contains("WHERE x = 1"));
        assert!(parses(sql), "{out}");
    }

    #[test]
    fn keywords_inside_literals_and_comments_are_left_alone() {
        for sql in [
            "SELECT 'USING SAMPLE 10%' FROM t",
            "SELECT 1 -- TABLESAMPLE 10%\nFROM t",
            "SELECT \"TABLESAMPLE\" FROM t",
            "SELECT $$AS MATERIALIZED ($$",
        ] {
            assert_eq!(DuckDbDialectParser.preprocess(sql), sql);
        }
    }

    /// Not read, and not rewritten: these stay failures the analyzer reports per statement.
    #[test]
    fn what_is_not_read_is_left_as_written() {
        for sql in [
            "FROM orders SELECT id",
            "PIVOT t ON y USING sum(x)",
            "SUMMARIZE t",
        ] {
            assert_eq!(DuckDbDialectParser.preprocess(sql), sql);
            assert!(!parses(sql), "{sql} now parses: update the crate docs");
        }
    }
}
