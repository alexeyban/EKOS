# RFC 0058 — ClickHouse Dialect: Preprocess `INDEX`/`PARTITION BY`/`SAMPLE BY`/`SETTINGS`/`CREATE DICTIONARY`

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-20

---

## Motivation

RFC 0057 fixed `CODEC(...)`, the first `sqlparser`/`ClickHouseDialect` gap found while running
`ekos recover` against a real repo (`analytics/`, Plausible Analytics). Live re-verification after
that fix (its own Acceptance Criteria, checked honestly rather than assumed) found the same file
immediately hits a second wall — reported to the user rather than silently fixed. The user then
asked to close it. Investigating fully, before writing any code, turned up more than the three
named gaps:

- **`INDEX <name> <expr> TYPE <type> GRANULARITY <n>`** (table-level secondary index, inside the
  column list — real file, line 49). `sqlparser`'s only `INDEX`-as-table-constraint grammar
  (`parser/mod.rs:6863`) is gated to `dialect_of!(self is GenericDialect | MySqlDialect)` —
  `ClickHouseDialect` excluded — and even if it weren't, that grammar parses MySQL's `INDEX name
  (col1, col2, ...)` shape, which doesn't match ClickHouse's `TYPE ... GRANULARITY ...` form at
  all. Two independent reasons this can never parse as-is, not one.
- **`PARTITION BY <expr>`** (real file, lines 52 and 310). Confirmed in RFC 0057's own testing
  section: `CREATE TABLE`'s `partition_by` field is only parsed for `dialect_of!(self is
  BigQueryDialect | PostgreSqlDialect | GenericDialect)` (`parser/mod.rs:6236`) — ClickHouse
  excluded, despite `PARTITION BY` being arguably ClickHouse's single most characteristic
  `MergeTree` clause.
- **`SAMPLE BY <expr>`** (real file, lines 55 and 313) — not named by the user, found while
  reading the real file's full table-options tail to design the fix. `Keyword::SAMPLE` does not
  exist anywhere in `sqlparser`'s keyword table at all (confirmed by grep) — there is no gate to
  even check, `SAMPLE` simply isn't a keyword the tokenizer recognizes as anything but a plain
  identifier.
- **`SETTINGS <k>=<v>, <k2>=<v2>, ...`** (real file, present on nearly every table). No `CREATE
  TABLE` handling anywhere in the crate — the only `Keyword::SETTINGS` reference in the whole
  crate (`parser/mod.rs:9280`) is for an unrelated `SELECT ... SETTINGS` clause, not `CREATE
  TABLE`.
- **`CREATE DICTIONARY ...`** (real file, two occurrences) — not named by the user, found the same
  way. A zero-hit grep for `DICTIONARY` anywhere in `sqlparser` confirms this is not a gated
  option on an existing statement type, like the four gaps above; it's an entirely different
  top-level statement `sqlparser` has no grammar for at all.

**Why the last two are in scope even though the user only named three:** `SqlAnalyzerPass::run`
(`crates/recovery/src/sql_analyzer.rs:163`, `parse_ddl_structural`) calls `Parser::parse_sql` once
for the *entire file* and discards every table into an empty graph if *any* statement anywhere in
the file fails to parse — confirmed by reading the function directly, not assumed. `structure.sql`
contains two `CREATE DICTIONARY` statements and `SAMPLE BY` on both of its `VersionedCollapsing`/
`MergeTree` event tables. Fixing only `INDEX`/`PARTITION BY`/`SETTINGS` and stopping would still
leave the whole file unparseable — the acceptance criterion the user actually cares about
("real `Table` KIR objects get compiled") would still not be met. Closing the named gaps without
closing these two immediately-adjacent ones, found in the same investigation, on the same file,
would be technically responsive to the literal request and practically useless.

## Scope

Extend `ClickHouseDialectParser::preprocess` with four more transforms, chained after RFC 0057's
`strip_codec_clauses`:

1. `strip_index_clauses` — removes `INDEX ... GRANULARITY <n>` table-level index definitions from
   inside the column list, including one adjacent comma so the list stays well-formed.
2. `strip_keyword_expr_clause` — a small reusable primitive (not three copies of the same logic)
   that removes `<keyword> [<keyword2>] <expr>`, terminated by the next occurrence (outside
   parens/quotes) of any keyword in a caller-supplied terminator list, a top-level `;`, or end of
   input. Applied three times: `PARTITION BY` (terminators: `PRIMARY`, `ORDER`, `SAMPLE`,
   `SETTINGS`, `COMMENT`), `SAMPLE BY` (terminators: `SETTINGS`, `COMMENT`), `SETTINGS`
   (terminators: `COMMENT`).
3. `strip_create_dictionary_statements` — removes whole `CREATE DICTIONARY ... ;` statements.

## Non-goals

- **Not modeling `INDEX`/`PARTITION BY`/`SAMPLE BY`/`SETTINGS`/dictionaries in the KIR.** Same
  reasoning as RFC 0057's `CODEC` decision: `ClickHouseAnalyzerPass`'s (RFC 0056 Stage 1)
  `properties["columns"]` shape never captured any of these, even from live `system.columns`
  introspection, so textually discarding them during file-based recovery loses no information any
  existing EKOS pass already captures.
- **Not a general ClickHouse-DDL-completeness guarantee.** This closes every gap the real
  `analytics/priv/ingest_repo/structure.sql` file actually hits, verified by getting that specific
  file to fully parse (see Testing) — not a claim that every possible ClickHouse DDL construct now
  parses. A future real file hitting a new gap gets its own RFC, the same discipline RFC 0057
  already stated and this RFC is itself an instance of (found five minutes after RFC 0057 shipped,
  on the same file, and given its own RFC rather than silently folded backward into 0057's already-
  closed Acceptance Criteria).
- **Not fixing the upstream `sqlparser` crate.** Same alternative-considered-and-rejected reasoning
  as RFC 0057: preprocessing, not forking/patching, keeps zero new dependency risk. Filing the
  real fixes upstream is a separate, non-blocking action.

## Design

All four new functions live in `plugins/sql-dialect-clickhouse/src/lib.rs`, chained in
`preprocess`:

```rust
fn preprocess_clickhouse_ddl(sql: &str) -> String {
    let sql = strip_codec_clauses(sql);                                   // RFC 0057
    let sql = strip_index_clauses(&sql);                                  // RFC 0058
    let sql = strip_keyword_expr_clause(&sql, "PARTITION", Some("BY"),
        &["PRIMARY", "ORDER", "SAMPLE", "SETTINGS", "COMMENT"]);
    let sql = strip_keyword_expr_clause(&sql, "SAMPLE", Some("BY"),
        &["SETTINGS", "COMMENT"]);
    let sql = strip_keyword_expr_clause(&sql, "SETTINGS", None, &["COMMENT"]);
    strip_create_dictionary_statements(&sql)
}
```

Each transform reuses the same quote/backtick-aware, word-boundary-matching scanning style RFC
0057 already established (`is_ident_char`, `matches_word_at`) — no new dependency, hand-written
scanners throughout.

- **`strip_index_clauses`**: matches `INDEX` at a word boundary outside quotes/backticks, then
  scans forward tracking paren depth (the `TYPE` sub-expression can itself carry parens, e.g.
  `TYPE bloom_filter(0.01)`) until finding `GRANULARITY` at depth 0, then consumes the following
  whitespace and digit run as the granularity value. If the clause is well-formed (a `GRANULARITY
  <digits>` was actually found), removes the whole span plus one adjacent comma — the trailing
  comma if the clause is followed by `,`, else a preceding comma already emitted to `out` (trimmed
  the same way RFC 0057 trims a dangling space before `CODEC`) — so the enclosing column list
  never ends up with a doubled or dangling comma.
- **`strip_keyword_expr_clause`**: after matching the keyword(s) at a word boundary, scans forward
  tracking paren depth and single-quoted strings; at depth 0, checks at each position whether the
  upcoming word matches a terminator (also at a word boundary) or is a top-level `;`; stops there
  (before the terminator, so the terminator clause itself is left completely untouched for a later
  pass — or for `sqlparser` itself, in `PRIMARY`/`ORDER`'s case — to parse) or at end of input.
  Malformed input (e.g. `PARTITION` not followed by `BY`) is left untouched, same "only remove
  what's certain" posture as RFC 0057.
- **`strip_create_dictionary_statements`**: matches `CREATE` then whitespace then `DICTIONARY` at
  a word boundary, then scans forward the same depth/quote-aware way to the next top-level `;`,
  removing the entire statement including that semicolon.

## Alternatives Considered

- **Writing `PARTITION BY`/`SAMPLE BY`/`SETTINGS` as three separate hand-copied functions**
  (matching `strip_index_clauses`'s standalone style) — rejected once the three turned out to be
  the exact same shape (`keyword [keyword2] expr-until-terminator-keyword-or-statement-end`) with
  only the keyword/terminator-list arguments differing; a single parameterized
  `strip_keyword_expr_clause` avoids three near-identical scanners that would drift out of sync on
  the next bugfix.
- **Silently dropping `CREATE DICTIONARY` statements without calling it out as a real information
  loss** — rejected; the RFC states plainly that dictionaries are not modeled at all today, rather
  than letting the fact that "the file parses now" imply more coverage than it delivers.
- **Stopping at the three gaps the user literally named and reporting `SAMPLE BY`/`CREATE
  DICTIONARY` as yet another "found but not fixed"** — rejected this time: the user's request was
  explicit ("close the gaps too"), and stopping short of an actually-parseable file when the
  remaining two gaps are immediately adjacent and already understood would not deliver what was
  asked. RFC 0057's narrower stopping point was the right call when the fix's true size was still
  unknown; here it is known, and finishing it is the more honest response to "close it."

## Testing

- Unit tests per function: `strip_index_clauses` (simple index, index with a parameterized `TYPE`
  like `bloom_filter(0.01)`, index as the last column-list entry vs. a middle entry, no
  false-strip of an unrelated identifier containing "INDEX"); `strip_keyword_expr_clause`
  (`PARTITION BY` stopping at `PRIMARY`/`ORDER`/`SETTINGS`/`;`, `SAMPLE BY` stopping at `SETTINGS`,
  bare `SETTINGS` stopping at `COMMENT`/`;`, malformed `PARTITION` with no `BY` left untouched);
  `strip_create_dictionary_statements` (single statement removed, two statements in one file both
  removed, a `CREATE TABLE` statement elsewhere in the same file left untouched).
- **The real regression test**: the *entire, unmodified* `analytics/priv/ingest_repo/structure.sql`
  file, embedded as a fixture, preprocessed and parsed with `Parser::parse_sql` end to end,
  asserting `Ok` and a non-zero statement count — the strongest test available, since it's the
  literal file that motivated both this RFC and RFC 0057.
- Full workspace gate: `cargo build --workspace && cargo test --workspace && cargo clippy
  --workspace -- -D warnings && cargo fmt --check`.
- Live verification: rerun `ekos recover` against the real `analytics/` repo, confirm zero SQL
  parse warnings and that `ekos query find` surfaces real `ObjectKind::Table` objects for
  `events_v2`, `sessions_v2`, and every `imported_*`/support table, each with populated
  `properties["columns"]`.

## Acceptance Criteria

- [x] `strip_index_clauses`, `strip_keyword_expr_clause`, `strip_create_dictionary_statements`
      implemented and unit-tested (16 new tests, 24 total in the crate).
- [x] The full real `analytics/priv/ingest_repo/structure.sql` file parses end to end after
      preprocessing — embedded verbatim as a test fixture
      (`plugins/sql-dialect-clickhouse/tests/fixtures/analytics-structure.sql`), asserted to parse
      into exactly 15 `Statement::CreateTable`s (every real table; both `CREATE DICTIONARY`
      statements correctly stripped).
- [x] Full workspace `cargo build/test/clippy/fmt` clean.
- [x] **Live, fully met this time.** Rebuilt `target/release/ekos` and reran the full pipeline
      against the real `analytics/` repo. `sql-analyzer` reported
      `objects=15 relationships=0` with **zero parse warnings** — up from `falling back to empty
      graph` before RFC 0057. `ekos query find "sessions_v2"`/`"events_v2"` surface real
      `plausible_events_db.sessions_v2` / `plausible_events_db.events_v2` `Table` objects;
      `ekos query object` on `sessions_v2` shows all 43 real columns with correct types (including
      `LowCardinality(FixedString(2))`, `Array(STRING)` for the `entry_meta.key`/`entry_meta.value`
      pair, and every `ALIAS` column), each with 100%-confidence Evidence citing
      `priv/ingest_repo/structure.sql`. This closes the gap RFC 0057 found and reported rather than
      silently expanded into.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0058-clickhouse-table-options-preprocessing.md` | This RFC |
| `ekos/plugins/sql-dialect-clickhouse/src/lib.rs` | `strip_index_clauses`, `strip_keyword_expr_clause`, `strip_create_dictionary_statements`, `preprocess_clickhouse_ddl` orchestrator, new tests including the full real-file regression |
