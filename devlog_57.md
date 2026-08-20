# Devlog 57 — RFC 0057: EKOS pointed at a stranger, and the CODEC gap it found

**Date:** 2026-08-20
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

A user asked EKOS to research `/home/legion/PycharmProjects/analytics` (Plausible Analytics, a
real, unmodified open-source Elixir/Phoenix app) and document its ClickHouse component — the
first time this session's EKOS binary had ever been pointed at that repo. The compiler pipeline
(`init → build → recover → resolve → compile → commit`) compiled 3,074 objects/3,977
relationships from git history (500 commits, 124 contributors), CI/CD, dependencies, and docs
cleanly. It found its own gap on the one file that mattered most for the ask: RFC 0031's
`"clickhouse"` SQL dialect (RFC 0056) failed to parse `priv/ingest_repo/structure.sql` at all,
falling back to an empty graph. The documentation got written anyway — direct source reads filled
the gap the ledger couldn't — and published as an Artifact. Two follow-on deliverables came out of
that session: a presentation deck documenting the cold run itself (gap named plainly, real
transcripts throughout), and RFC 0057, which found and fixed the specific root cause: `sqlparser`'s
`ClickHouseDialect` has never supported ClickHouse's own `CODEC(...)` column clause. Live
re-verification after the fix found a second, unrelated wall the same file hits next
(`INDEX ... TYPE ... GRANULARITY`) — reported to the user rather than silently chased further.

---

## RFC 0057 — ClickHouse Dialect: Preprocess `CODEC(...)` Before Parsing

### Problem / motivation

`ekos recover` against `analytics/` (routed via `[[recover.sql.dialect-rules]]` to the
`"clickhouse"` dialect for `priv/ingest_repo/**`) failed with:

```
sql parser error: Expected: ',' or ')' after column definition, found: CODEC at Line: 7, Column: 23
```

Line 7 is `` `timestamp` DateTime CODEC(Delta(4), LZ4), `` — completely ordinary ClickHouse DDL.
Two explanations were checked, in order, before writing any code:

1. **Stale binary.** `target/release/ekos` on disk predated RFC 0056's dialect registration —
   confirmed live: rerunning `ekos recover` against the identical file with that binary produced
   an *additional* `unknown dialect "clickhouse"` warning that a fresh `cargo build --release -p
   ekos` against current `main` made disappear. A real, separate bug, fixed in under 30 seconds —
   but the CODEC error itself didn't move.
2. **`sqlparser` itself.** Reading `sqlparser-0.53.0/src/parser/mod.rs:6410`'s
   `parse_optional_column_option` directly: it's a long `if`/`else if` chain matching specific
   `Keyword` variants, and ClickHouse-specific column options genuinely exist there —
   `MATERIALIZED`/`ALIAS`/`EPHEMERAL` are all real, gated branches. `CODEC` is not one of them,
   confirmed by a zero-hit grep across the entire vendored crate, and cross-checked against the
   current published `ColumnOption` API docs (23 variants, still no `Codec`/`Compression` among
   them). This is a real, still-open upstream gap in `apache/datafusion-sqlparser-rs`, not
   something a version bump would have already fixed.

### What was built

| Component | Location |
|---|---|
| RFC | `ekos/docs/rfcs/0057-clickhouse-codec-preprocessing.md` |
| `strip_codec_clauses` + `preprocess` override | `ekos/plugins/sql-dialect-clickhouse/src/lib.rs` |
| 6 new unit tests + 1 real-fixture regression test | same file |

`ClickHouseDialectParser::preprocess` — previously a no-op, with a moduledoc explicitly (and, as
of this session, incorrectly) claiming "No preprocessing is needed" — now strips well-formed
`CODEC(...)` clauses before the SQL reaches `sqlparser`. The scanner is quote-aware (tracks
single-quoted strings, with `ClickHouseDialect`'s backslash-escape convention, and backtick-quoted
identifiers, so the literal word `CODEC` inside either is never mistaken for the clause) and
balanced-paren-aware (handles nested forms like `CODEC(ZSTD(3))` and multi-arg
`CODEC(Delta(4), LZ4)`, both present in the real fixture file). Both `SqlAnalyzerPass` (which
calls `dialect_parser.preprocess` internally) and `SqlTransformAnalyzerPass` (preprocessed
explicitly in `recover.rs`) pick this up with zero caller-side changes.

### Implementation details worth remembering

**The fix reused RFC 0031's existing `preprocess` hook rather than touching `sqlparser` itself.**
`MySqlDialectParser::strip_delimiter_directives` already established the pattern — dialect
grammar `sqlparser` doesn't support gets normalized away textually before parsing, not patched
into the parser. Forking/vendoring `sqlparser` was explicitly considered and rejected: RFC 0056
chose the pinned crates.io `sqlparser = "0.53"` specifically to avoid new dependency risk, and a
local fork would both reverse that decision and drift silently from upstream on every version
bump. A hand-written scanner (no `regex` dependency, same as the MySQL precedent) was a few more
lines than a regex would have been and was already proven correct in this exact codebase for a
structurally similar nested-delimiter problem.

**Live re-verification (this project's established discipline — RFC 0054/0055/0056 each found a
real bug invisible to `cargo test` alone) found a second wall on the exact same file.** After
rebuilding and rerunning `ekos recover` against `analytics/`, the CODEC warning was gone —
progress moved from line 7 to line 49:

```
sql parser error: Expected: ',' or ')' after column definition, found: timestamp at Line: 49, Column: 28
```

Line 49 is `INDEX minmax_timestamp timestamp TYPE minmax GRANULARITY 1` — a table-level secondary
index definition inside the column list, a completely separate `sqlparser` gap. Checking further:
`PARTITION BY` is *also* unsupported for `ClickHouseDialect` specifically — `CREATE TABLE`'s
`partition_by` field is only parsed for `dialect_of!(self is BigQueryDialect | PostgreSqlDialect |
GenericDialect)` (`parser/mod.rs:6236`), ClickHouse conspicuously absent even though `PARTITION
BY` is arguably ClickHouse's single most characteristic MergeTree clause. `SETTINGS` has no
`CREATE TABLE` handling anywhere in the crate at all. **`sqlparser`'s `ClickHouseDialect` support
for `CREATE TABLE` turned out to be much narrower than "missing one clause"** — real ClickHouse
DDL almost always carries `PARTITION BY`/`SETTINGS`, and often `INDEX`, so a real schema file
still won't fully recover into `Table`/`Column` KIR objects after this RFC alone.

**This was reported to the user rather than silently chased.** Each of `INDEX`/`PARTITION
BY`/`SETTINGS` is architecturally the same *class* of fix (a preprocessing strip, since none of
them have any parse path for `ClickHouseDialect` to extend), but scoping all of them into "fix
CODEC" would have quietly turned a single-clause bug fix the user asked about into an unscoped,
multi-clause ClickHouse-DDL-compatibility project. RFC 0057's own Non-goals name this explicitly:
further gaps get their own RFC when a real file hits them and the user chooses to pursue it — the
same just-in-time discipline this project applies everywhere else, not abandoned just because the
next gap was found five minutes after the first one.

### Decisions (alternatives considered, why this choice)

- **Forking/patching `sqlparser` locally via a `[patch]` override** — rejected, see above.
- **Filing the real fix upstream in `apache/datafusion-sqlparser-rs` instead of a local
  workaround** — correct long-term fix, but this codebase doesn't control that review timeline; a
  real user's request doesn't wait on it. Worth doing as a separate, non-blocking action.
- **Regex-based stripping (`regex` crate)** — rejected on the same "no new dependency for one
  clause" grounds already established for the MySQL `DELIMITER` case.
- **Silently expanding the fix to also strip `INDEX`/`PARTITION BY`/`SETTINGS`, since they were
  found in the same session** — rejected. The user asked specifically about `CODEC`; the honest
  move once scope was found to be larger than expected was to report it and let the user decide
  whether to keep going, not to unilaterally balloon a small, well-tested fix into a much larger
  unscoped one.

---

## Knowledge Captured

- **A parse error's line/column moving after a fix is a stronger live-verification signal than
  "the specific warning text is gone."** Rerunning against the exact same real file and watching
  the failure point advance (line 7 → line 49) is what actually proved the CODEC fix worked,
  independent of and before discovering the next gap — a cleaner signal than trusting the unit
  tests alone would have given.
- **A vendored parser crate's dialect-specific `dialect_of!` gates are worth grepping before
  assuming "ClickHouse support" means "full ClickHouse support."** `sqlparser`'s `ClickHouseDialect`
  gets real, deliberate treatment for `MATERIALIZED`/`ALIAS`/`EPHEMERAL`/`PRIMARY KEY`-before-`ORDER
  BY` — but `PARTITION BY` (arguably more central to ClickHouse than any of those) is gated to a
  dialect list that excludes it, and `CODEC`/`SETTINGS`/table-level `INDEX` have no path at all.
  "This dialect exists and is tested" is not the same claim as "this dialect covers what a real
  schema file for that system actually contains" — the two only get confirmed apart by running a
  real file through it, the same lesson RFC 0054/0055/0056 each drew from a different angle.
- **Stale release binaries produce warnings that look like config bugs.** `unknown dialect
  "clickhouse"` read, at first glance, like a typo in `ekos.toml`'s `[[recover.sql.dialect-rules]]`
  — it was actually a `target/release/ekos` binary that predated the dialect's registration in
  `main`. Worth checking build recency before debugging config when a *just-shipped* RFC's feature
  appears to be unavailable.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0057-clickhouse-codec-preprocessing.md` | New RFC |
| `ekos/plugins/sql-dialect-clickhouse/src/lib.rs` | `strip_codec_clauses`, `preprocess` override, corrected moduledoc, 6 new tests + 1 real-fixture regression test (11 total) |
| `docs/presentations/analytics-clickhouse-cold-run.html` | New deck: EKOS pointed at `analytics/` cold, gap found and named, real transcripts throughout |
| `docs/presentations/examples/analytics-clickhouse/*.txt` | 5 real, unedited terminal transcripts backing the deck's "open the real transcript" links |
| `docs/presentations.html`, `docs/index.html` | New deck listing entries |
| `README.md` | Second ClickHouse deck link, noting the CODEC gap and RFC 0057 |
| `TODO.md` | RFC 0057 entry |
| (untracked, in `analytics/`, not this repo) `ekos.toml`, `.ekos/` | The compiled ledger and dialect-rule config used for the research session — left for the user to decide whether to keep/gitignore |

## Still open (tracked, not silently dropped)

- **`INDEX ... TYPE ... GRANULARITY`, `PARTITION BY`, and `SETTINGS` are all still unsupported by
  `sqlparser`'s `ClickHouseDialect` for `CREATE TABLE`.** Each would need the same class of
  preprocessing-strip fix as this RFC's `CODEC` handling. Not started — offered to the user as
  follow-on work, not assumed.
- **An upstream PR to `apache/datafusion-sqlparser-rs` adding real `CODEC`/`PARTITION BY` support**
  would obsolete this RFC's workaround entirely for any consumer of that crate, not just EKOS.
  Worth filing separately; not this session's scope.
