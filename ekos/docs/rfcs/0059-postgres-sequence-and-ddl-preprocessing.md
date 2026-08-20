# RFC 0059 — Postgres Dialect: Preprocess `CREATE`/`ALTER SEQUENCE`, `UNLOGGED`, `NOT VALID`

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-20

---

## Motivation

Devlog 60's whole-repo cold run against `analytics/` (Plausible Analytics) exercised the Postgres
dialect for the first time against its real, unmodified application schema dump
(`priv/repo/structure.sql`, 2,738 lines, `[[recover.sql.dialect-rules]] path-glob = "priv/repo/**"
dialect = "postgres"`) — every prior session against this repo only ever touched the ClickHouse
file. The whole file failed: `sql parser error: Expected: end of statement, found: INCREMENT at
Line: 116, Column: 5`. `SqlAnalyzerPass::run` (`crates/recovery/src/sql_analyzer.rs`,
`parse_ddl_structural`) calls `Parser::parse_sql` once for the entire file and discards every table
into an empty graph if any statement anywhere fails — the same "one bad statement loses the whole
file" shape RFC 0057/0058 fixed for ClickHouse, this time on the schema behind `sites`, `api_keys`,
and every other core application table. `sql-transform-analyzer` degraded the same way: 1 of 1,282
statements mapped (0.078% coverage).

Investigating before writing code (per the mandated workflow) found three independent, real gaps in
`sqlparser = "0.53"`'s `PostgreSqlDialect`, not one:

- **`CREATE SEQUENCE`/`ALTER SEQUENCE` clause ordering** (the named error, 34 + 34 real
  occurrences). `parse_create_sequence_options` (`parser/mod.rs:12699`) checks
  `INCREMENT`/`MINVALUE`/`MAXVALUE`/`START`/`CACHE`/`CYCLE` in that fixed order, **once each, with
  no loop**. Real `pg_dump` output emits `START WITH` *before* `INCREMENT BY`:
  ```sql
  CREATE SEQUENCE public.api_keys_id_seq
      START WITH 1
      INCREMENT BY 1
      NO MINVALUE
      NO MAXVALUE
      CACHE 1;
  ```
  The single-pass checker matches `START WITH 1` on its (later, in fixed-order) `START` check,
  finds no further match for anything after it in that order, and returns with `INCREMENT BY 1 ...`
  still unconsumed — which then fails the caller's own end-of-statement expectation. This is a real,
  still-open upstream ordering bug (confirmed: the grammar for both statement types exists, it's
  order-fragile, not missing) — not something a version bump or a config flag would fix.
- **`CREATE UNLOGGED TABLE`** (1 real occurrence, `public.oban_peers`). `parse_create`'s dispatcher
  (`parser/mod.rs:3847`) checks `TEMP`/`TEMPORARY` then goes straight to `if
  self.parse_keyword(Keyword::TABLE)` — `UNLOGGED` is a real, tokenizable `Keyword`
  (`keywords.rs:821`, used elsewhere for `SELECT ... INTO UNLOGGED TABLE`) but never consulted at
  this dispatch point, so the parser falls through every `else if` to `self.expected("an object
  type after CREATE", ...)`.
- **`NOT VALID`** (2 real occurrences, trailing `ADD CONSTRAINT ... CHECK (...) NOT VALID`).
  Zero-hit grep for `NOT VALID`/`NotValid` anywhere in the crate — no grammar for this real Postgres
  clause (deferred constraint validation) at all.

**Why all three are in scope, not just the one named error:** the acceptance criterion is "the real
file parses" (RFC 0058's same standard), and `SqlAnalyzerPass` discards the whole file on the first
unhandled statement regardless of which one it is. Fixing only the sequence-ordering bug and
stopping would still leave the file unparseable at `CREATE UNLOGGED TABLE`, then again at `NOT
VALID` — found by the same "keep re-running the scratch parse test after each fix" loop RFC 0057→
0058 used, not assumed from reading the file once.

## Scope

Extend `PostgresDialectParser::preprocess` (previously the identity function — no preprocessing had
ever been needed for Postgres before this file) with three transforms:

1. `strip_statements_starting_with(sql, &["CREATE", "SEQUENCE"])` and `strip_statements_starting_with(sql,
   &["ALTER", "SEQUENCE"])` — remove whole `CREATE SEQUENCE ... ;`/`ALTER SEQUENCE ... ;`
   statements, rather than trying to reorder every clause combination `pg_dump` might emit.
2. `strip_unlogged_before_table` — removes just the `UNLOGGED` keyword, keeping `CREATE TABLE ...`
   intact.
3. `strip_not_valid_clause` — removes just the trailing `NOT VALID` clause, keeping the rest of the
   `ALTER TABLE ... ADD CONSTRAINT ... CHECK (...)` statement intact.

## Non-goals

- **Not modeling sequences in the KIR.** Same reasoning as RFC 0058's `CREATE DICTIONARY` decision:
  `crates/recovery/src/sql_analyzer.rs` only ever emits `Table`/`Column` facts from DDL recovery,
  never sequences — textually discarding whole `CREATE`/`ALTER SEQUENCE` statements loses no
  information any existing EKOS pass already captures.
- **Not dropping the `UNLOGGED`/`NOT VALID` statements wholesale.** Unlike sequences, these attach
  to real, already-modeled facts — `CREATE UNLOGGED TABLE` is a real user table with real columns,
  and `ADD CONSTRAINT ... CHECK` names a real constraint on a real table (even though
  `sql_analyzer.rs` doesn't model `CHECK` constraints as KIR facts either, the surrounding
  `ALTER TABLE ONLY ...` statement's *other* effects, and any sibling statements in a longer
  multi-clause `ALTER TABLE`, must not be lost to a whole-statement strip when a one-keyword/
  one-clause strip fully solves it instead). Keyword/clause-level stripping was chosen specifically
  because it's strictly more information-preserving than the sequence fix's whole-statement
  approach.
- **Not a general Postgres-DDL-completeness guarantee.** This closes every gap the real
  `analytics/priv/repo/structure.sql` file actually hits (2,738 lines, 385 real statements after
  preprocessing) — not a claim that every possible Postgres DDL construct now parses. A future real
  file hitting a new gap gets its own RFC, the same discipline RFC 0057/0058 established.
- **Not fixing the upstream `sqlparser` crate.** Preprocessing, not forking/patching, keeps zero new
  dependency risk, matching every prior dialect-preprocessing RFC in this codebase. Filing the real
  fixes upstream (the `CREATE SEQUENCE` ordering bug in particular, since it's a real correctness
  gap independent of EKOS) is a separate, non-blocking action.

## Design

All three transforms live in `plugins/sql-dialect-postgres/src/lib.rs`, chained in `preprocess`:

```rust
fn preprocess_postgres_ddl(sql: &str) -> String {
    let sql = strip_statements_starting_with(sql, &["CREATE", "SEQUENCE"]);
    let sql = strip_statements_starting_with(&sql, &["ALTER", "SEQUENCE"]);
    let sql = strip_unlogged_before_table(&sql);
    strip_not_valid_clause(&sql)
}
```

Each transform reuses the same quote-aware, word-boundary-matching scanning style RFC 0057/0058
established for ClickHouse (`is_ident_char`, `matches_word_at`, `is_word_boundary_match`,
`scan_to_terminator`) — no new dependency, hand-written scanners throughout. One real difference
from the ClickHouse scanners: Postgres's `--` line comments needed explicit handling.
`pg_dump`-generated files put a `-- Name: x; Type: SEQUENCE; Schema: public; Owner: -` comment
header before nearly every statement — routinely containing both a literal `;` and the literal
keyword text being matched, inside the comment itself. An early version of this preprocessing that
wasn't comment-aware silently failed to strip anything on the real file (the comment's embedded `;`
confused a statement-boundary heuristic into never recognizing the real statement start) — found
immediately by the same scratch-test iteration loop, before any RFC text was finalized, and fixed by
having `strip_statements_starting_with`/`scan_to_terminator` copy `-- ...` comment text through to
`\n` verbatim without scanning it for `;` or keywords.

- **`strip_statements_starting_with(sql, keywords)`**: generalizes RFC 0058's
  `strip_create_dictionary_statements` to an arbitrary leading-keyword sequence (so `CREATE
  SEQUENCE` and `ALTER SEQUENCE` share one implementation instead of two near-identical copies).
  Matches `keywords` word-boundary-by-word-boundary from the current scan position (allowing one
  optional extra word between the first two keywords, e.g. `CREATE TEMPORARY SEQUENCE`), then scans
  to the next top-level `;` (or end of input) and removes the whole span.
- **`strip_unlogged_before_table`**: matches `UNLOGGED` immediately followed (across whitespace) by
  `TABLE`, drops just `UNLOGGED` and the surrounding whitespace, and re-inserts exactly one
  separating space so `CREATE`+`TABLE` don't glue together.
- **`strip_not_valid_clause`**: matches `NOT` immediately followed by `VALID`, drops both words and
  any trailing whitespace before them — leaving `IS NOT NULL` and every other unrelated `NOT`
  untouched, since the match requires `VALID` to follow directly.

## Alternatives Considered

- **Reordering `CREATE SEQUENCE`'s clauses into `sqlparser`'s expected fixed order** (`INCREMENT`,
  `MINVALUE`, `MAXVALUE`, `START`, `CACHE`, `CYCLE`) instead of stripping the whole statement —
  rejected: correctly handling every clause-order permutation real `pg_dump` output (or hand-written
  SQL) might emit is meaningfully more code than one whole-statement strip, for a statement kind
  that was never going to be modeled in the KIR regardless of whether it parses.
- **Dropping the whole `CREATE UNLOGGED TABLE`/`ALTER TABLE ... NOT VALID` statements**, matching
  the sequence fix's shape exactly for consistency — rejected: unlike sequences, these carry real,
  already-modeled information (a real table's real columns; a real `ALTER TABLE` statement that may
  carry other real effects alongside the one unsupported clause). A one-keyword/one-clause strip is
  no more code and is strictly more information-preserving.
- **Ignoring the comment-awareness bug and requiring dialect rules to route around `pg_dump`-style
  comments** — rejected: `pg_dump` output is the overwhelmingly common real-world shape a Postgres
  DDL file arrives in; a preprocessing pass that only works on hand-written, comment-free SQL would
  have shipped silently broken against the exact real file that motivated this RFC.

## Testing

- Unit tests per function: `strip_statements_starting_with` (removes a `CREATE SEQUENCE`, removes
  an `ALTER SEQUENCE`, is comment-aware against a real `pg_dump`-style header with an embedded `;`
  and the matched keyword inside the comment text, leaves unrelated SQL untouched);
  `strip_unlogged_before_table` (removes the keyword, leaves ordinary `CREATE TABLE` untouched);
  `strip_not_valid_clause` (removes the clause after a real `CHECK` constraint, does not touch an
  unrelated `IS NOT NULL`). Plus one parse-level test per transform confirming the preprocessed
  output actually parses with `PostgreSqlDialect`, not just that the text transform looks right.
- **The real regression test**: the *entire, unmodified* `analytics/priv/repo/structure.sql` file
  (2,738 lines), embedded as a fixture, preprocessed and parsed with `Parser::parse_sql` end to end,
  asserting `Ok`, zero leftover occurrences of `INCREMENT`/`UNLOGGED`/`NOT VALID` in the
  preprocessed text, and exactly 42 `Statement::CreateTable`s (41 ordinary + 1 `UNLOGGED`) — the
  literal file that motivated this RFC.
- Full workspace gate: `cargo build --workspace && cargo test --workspace && cargo clippy
  --workspace -- -D warnings && cargo fmt --check`.
- Live verification: rerun `ekos recover` against the real `analytics/` repo, confirm the Postgres
  file now compiles real `Table`/`Column` KIR objects instead of falling back to an empty graph, and
  that `sql-transform-analyzer`'s mapped-statement coverage rises materially above the pre-fix
  0.078%.

## Acceptance Criteria

- [x] `strip_statements_starting_with`, `strip_unlogged_before_table`, `strip_not_valid_clause`
      implemented and unit-tested (13 new tests, 15 total in the crate).
- [x] The full real `analytics/priv/repo/structure.sql` file parses end to end after preprocessing
      — embedded verbatim as a test fixture
      (`plugins/sql-dialect-postgres/tests/fixtures/analytics-structure.sql`), asserted to parse
      into exactly 42 `Statement::CreateTable`s.
- [x] Full workspace `cargo build/test/clippy/fmt` clean.
- [ ] Live: rebuild `target/release/ekos`, rerun the full pipeline against the real `analytics/`
      repo, confirm `sql-analyzer`/`sql-transform-analyzer` now recover real `Table`/`Column` facts
      from the Postgres schema instead of falling back to an empty graph — recorded in devlog_60.md
      once run.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0059-postgres-sequence-and-ddl-preprocessing.md` | This RFC |
| `ekos/plugins/sql-dialect-postgres/src/lib.rs` | `strip_statements_starting_with`, `strip_unlogged_before_table`, `strip_not_valid_clause`, `preprocess_postgres_ddl` orchestrator, new tests including the full real-file regression |
| `ekos/plugins/sql-dialect-postgres/tests/fixtures/analytics-structure.sql` | Real, unmodified `analytics/priv/repo/structure.sql`, vendored as a regression fixture |
