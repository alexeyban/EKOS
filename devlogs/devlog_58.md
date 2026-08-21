# Devlog 58 — RFC 0058: closing the rest of the ClickHouse DDL gap, fully live-verified

**Date:** 2026-08-20
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Direct follow-on to devlog_57/RFC 0057. That RFC fixed `CODEC(...)` and, live-verifying honestly
rather than assuming success, found and reported a second wall on the same real file
(`analytics/priv/ingest_repo/structure.sql`) instead of silently chasing it. The user asked to
close it. Investigating fully before writing code turned up five gaps total, not three — `INDEX`,
`PARTITION BY`, `SETTINGS` as named, plus `SAMPLE BY` and `CREATE DICTIONARY` found in the same
pass, both necessary because `SqlAnalyzerPass` parses an entire file in one call and discards
everything on any single statement's failure. All five closed in one RFC (0058), and this time the
live-verification loop was run to actual completion: the real, unmodified `structure.sql` now
compiles into 15 real `Table` KIR objects with zero parse warnings, confirmed via `ekos query
object` showing all 43 real columns of `sessions_v2` with correct types and evidence.

---

## RFC 0058 — ClickHouse Dialect: Preprocess `INDEX`/`PARTITION BY`/`SAMPLE BY`/`SETTINGS`/`CREATE DICTIONARY`

### Problem / motivation

RFC 0057's own Acceptance Criteria, checked honestly after a live rebuild-and-rerun rather than
assumed from passing unit tests, recorded a real finding: the CODEC warning was gone, but the same
file's parse failure moved to line 49 — `INDEX minmax_timestamp timestamp TYPE minmax GRANULARITY
1`. That was reported to the user as open, not fixed. The user then asked to close it.

Investigating before writing any code (per the mandated workflow) read the real file in full
rather than guessing at the next error one line at a time, and found the true shape of the
problem was bigger than three clauses:

- **`INDEX ... TYPE ... GRANULARITY`** — `sqlparser`'s only `INDEX`-as-table-constraint grammar
  (`parser/mod.rs:6863`) is gated to `GenericDialect | MySqlDialect` *and* parses MySQL's
  `INDEX name (col, ...)` shape regardless of dialect — two independent reasons ClickHouse's
  `TYPE`/`GRANULARITY` form could never match it.
- **`PARTITION BY`** — confirmed in RFC 0057's own testing section already: gated to
  `BigQueryDialect | PostgreSqlDialect | GenericDialect`, ClickHouse excluded.
- **`SAMPLE BY`** — not named by the user. `Keyword::SAMPLE` doesn't exist anywhere in
  `sqlparser`'s keyword table at all; there's no gate to even check.
- **`SETTINGS`** — the only `Keyword::SETTINGS` reference in the whole crate is an unrelated
  `SELECT ... SETTINGS` clause, not `CREATE TABLE`.
- **`CREATE DICTIONARY`** — not named by the user either. A zero-hit grep for `DICTIONARY`
  anywhere in `sqlparser` confirmed this isn't a gated option on an existing statement type like
  the other four; it's an entirely different top-level statement the crate has no grammar for.

The last two were folded in without asking again, unlike RFC 0057's stopping point: this time the
user's request was explicit ("close the gaps too"), and `structure.sql` genuinely contains two
`CREATE DICTIONARY` statements and `SAMPLE BY` on both of its event tables — stopping at the three
named gaps would still have left the whole file unparseable, which is what "close it" actually
means in practice, not just fixing the three specific keywords by name.

### What was built

| Component | Location |
|---|---|
| RFC | `ekos/docs/rfcs/0058-clickhouse-table-options-preprocessing.md` |
| `strip_index_clauses` | `ekos/plugins/sql-dialect-clickhouse/src/lib.rs` |
| `strip_keyword_expr_clause` (reused for `PARTITION BY`/`SAMPLE BY`/`SETTINGS`) | same file |
| `strip_create_dictionary_statements` | same file |
| `preprocess_clickhouse_ddl` orchestrator, chaining all five passes | same file |
| Real-file regression fixture | `ekos/plugins/sql-dialect-clickhouse/tests/fixtures/analytics-structure.sql` |

`strip_index_clauses` removes `INDEX <name> <expr> TYPE <type_expr> GRANULARITY <n>` from inside
the column list, tracking paren depth (a parameterized `TYPE bloom_filter(0.01)` nests its own
parens) and cleaning up exactly one adjacent comma so the list stays well-formed regardless of
whether the index was the last entry or a middle one. `strip_keyword_expr_clause` is one
parameterized primitive — not three hand-copied scanners — for `<keyword> [<keyword2>] <expr>`,
terminated by the next occurrence (outside parens/quotes) of any word in a caller-supplied
terminator list, a top-level `;`, or end of input; applied to `PARTITION BY` (terminators:
`PRIMARY`/`ORDER`/`SAMPLE`/`SETTINGS`/`COMMENT`), `SAMPLE BY` (`SETTINGS`/`COMMENT`), and bare
`SETTINGS` (`COMMENT`). `strip_create_dictionary_statements` removes whole `CREATE DICTIONARY
... ;` statements — dictionaries were never modeled in the KIR even by RFC 0056 Stage 1's live
introspection, so this loses no information any existing EKOS pass already captured.

### Implementation details worth remembering

**Two rounds of the exact same bug class — a stripped clause gluing directly onto the next token
with no separating space — showed up across the CODEC (RFC 0057) and keyword-expr (RFC 0058)
strippers, and got fixed differently in each because the surrounding context differs.** For
`CODEC`, the fix trims a *preceding* space (the clause always sits right before a `,`/`)`, and the
space that separated it from the data type becomes dangling once the clause is gone). For
`PARTITION BY`/`SAMPLE BY`/`SETTINGS`, the naive port of that same trim actively broke things:
`ENGINE = MergeTree PARTITION BY toYYYYMM(start) PRIMARY KEY id` → stripping `PARTITION BY
toYYYYMM(start)` and then trimming trailing whitespace from the output produced
`ENGINE = MergeTreePRIMARY KEY id` — the space that was supposed to separate `MergeTree` from the
next real clause got eaten too, because unlike `CODEC`'s comma, the following token here (`PRIMARY`,
`ORDER`, `;`, ...) is content that must stay legible, not punctuation being cleaned up around.
Caught by the real-file regression test failing with exactly that glued string, not assumed fixed
because the smaller synthetic unit tests happened to pass first. The final rule: only safe to trim
trailing whitespace when what follows is `;` or end of input (nothing to glue onto); leave it
alone before a terminator *keyword*.

**Live re-verification this time reached actual completion, not just "a different error moved
further."** RFC 0057 stopped at "the error moved from line 7 to line 49" and reported the
remainder as open. This RFC's own live check went all the way: rebuild → `ekos recover` → `ekos
query find` → `ekos query object`, confirming not just "no warnings" but the actual compiled
content — 43 real columns on `sessions_v2`, correct nested types
(`LowCardinality(FixedString(2))`), correct `Array(STRING)` mapping for the `entry_meta.key`/
`entry_meta.value` Nested-style pair, every `ALIAS` column present, 100%-confidence Evidence
citing the real file. "The pipeline didn't warn" and "the compiled object is actually correct and
complete" are different claims, and only checking the first one is exactly the gap RFC 0056's own
live-verification note about `AsyncInsertRepo`/audit counts warned against repeating.

**The real fixture file earns its keep as a regression test far beyond what any hand-written
synthetic SQL string could catch.** Every glued-string bug in this RFC was found by the *real
file* test failing, not by the smaller synthetic unit tests (which, written by the same author
who wrote the bug, tend to only exercise the shapes already anticipated). Embedding the actual,
unmodified `structure.sql` as `include_str!` is now the second time in two RFCs this exact pattern
(`tests/fixtures/ecommerce.sql`, `northwind.sql`'s established precedent, now extended to a live
customer file) has paid for itself immediately.

### Decisions (alternatives considered, why this choice)

- **Three hand-copied strip functions for `PARTITION BY`/`SAMPLE BY`/`SETTINGS`** — rejected once
  it was clear they're the exact same shape with different keyword/terminator arguments; one
  parameterized `strip_keyword_expr_clause` avoids three scanners that would drift out of sync on
  the next bugfix (a real risk given how the whitespace-trim bug above required getting the exact
  same logic right in one place, not three).
- **Silently dropping `CREATE DICTIONARY` without naming it as real information loss** — rejected;
  the RFC states plainly that dictionaries were never modeled, rather than letting "the file
  parses now" imply more coverage than actually shipped.
- **Reporting `SAMPLE BY`/`CREATE DICTIONARY` as yet another "found but not fixed," matching RFC
  0057's own stopping posture** — rejected this time specifically because the user's request was
  explicit ("close the gaps too") and the two additional gaps were immediately adjacent, already
  understood, and necessary for the literal request to actually be satisfied at the live-testing
  level.

---

## Knowledge Captured

- **A stripped clause needs different whitespace-cleanup rules depending on what's structurally
  next to it — comma-adjacent (trim before) vs. keyword-adjacent (don't trim, or only trim right
  before `;`/EOF).** Porting one clause's whitespace-cleanup logic to a structurally different
  clause without re-deriving it from the actual surrounding grammar produces a real, silent bug —
  caught here only because a real-file regression test exercised the exact adjacency the synthetic
  unit tests didn't happen to cover.
- **"The warning is gone" is a weaker live-verification claim than "the compiled object is
  correct."** RFC 0057 stopped at the first; this RFC's own bar was the second — checking actual
  column counts, types, and evidence via `ekos query object`, not just an absence of `WARN` lines
  in the log.
- **A vendored parser's `CREATE TABLE` support for one dialect can be gapped in more independent
  ways than expected from a single failing test.** Five separate, independently-caused gaps
  (a missing keyword entirely, two different dialect-exclusion lists, one grammar-shape mismatch,
  and one entirely missing statement type) all block the exact same one real file. Fixing the
  first one found doesn't predict how many more remain — only reading the real file end to end,
  as this RFC did before writing any code, does.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0058-clickhouse-table-options-preprocessing.md` | New RFC, all Acceptance Criteria checked |
| `ekos/plugins/sql-dialect-clickhouse/src/lib.rs` | `strip_index_clauses`, `strip_keyword_expr_clause`, `strip_create_dictionary_statements`, `preprocess_clickhouse_ddl` orchestrator, 16 new tests (24 total) |
| `ekos/plugins/sql-dialect-clickhouse/tests/fixtures/analytics-structure.sql` | New: the real, unmodified `analytics/priv/ingest_repo/structure.sql`, embedded as a regression fixture |
| `TODO.md`, `README.md` | RFC 0058 entry; ClickHouse connector section and deck link updated from "gap found" to "gap closed, live-verified" |
| `ekos/docs/rfcs/0057-clickhouse-codec-preprocessing.md` | (from the prior session, left as its own honest historical record — not retroactively edited) |

## Still open (tracked, not silently dropped)

- **An upstream PR to `apache/datafusion-sqlparser-rs`** adding real `CODEC`/`INDEX`/`PARTITION
  BY`/`SAMPLE BY`/`SETTINGS`/`CREATE DICTIONARY` support would obsolete both RFC 0057's and this
  RFC's workarounds for any consumer of that crate, not just EKOS. Not filed this session.
- **`INDEX`/`PARTITION BY`/`SAMPLE BY`/`SETTINGS`/dictionaries are still not modeled in the KIR** —
  stripped for parseability, not captured as structured facts. A future RFC could add
  `properties["partition_by"]`/`properties["indexes"]` etc. to `ClickHouseAnalyzerPass`'s output
  shape if a real use case needs them; not assumed needed today.
