# RFC 0057 — ClickHouse Dialect: Preprocess `CODEC(...)` Before Parsing

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-20

---

## Motivation

Researching a real, unmodified open-source repo (`analytics/`, Plausible Analytics — an
Elixir/Phoenix app) for a user, `ekos.toml` was configured with RFC 0031 dialect rules routing
`priv/ingest_repo/**` (ClickHouse DDL) to the `"clickhouse"` dialect. `ekos recover` ran the file
through `SqlAnalyzerPass`/`SqlTransformAnalyzerPass`, both backed by
`ekos-plugin-sql-dialect-clickhouse`'s `ClickHouseDialectParser` (RFC 0056). It failed:

```
sqlparser failed on priv/ingest_repo/structure.sql: sql parser error: Expected: ',' or ')'
after column definition, found: CODEC at Line: 7, Column: 23; falling back to empty graph
```

Line 7 of that file is `` `timestamp` DateTime CODEC(Delta(4), LZ4), `` — a completely ordinary
ClickHouse column definition. `CODEC(...)` (per-column compression codec selection) is one of
ClickHouse's most common DDL clauses; real ClickHouse schemas use it on the majority of columns
in a MergeTree table (confirmed against this same file: both `events_v2` and `sessions_v2` carry
it on nearly every `String`/`DateTime` column). A dialect that can't parse it can't recover the
structural shape of most real ClickHouse tables at all — it fails whole-file, falling back to an
empty graph, not a partial one.

**This is not a config problem.** Before writing any code, the stale-binary explanation was ruled
out live: `target/release/ekos` initially predated RFC 0056's dialect registration (`unknown
dialect "clickhouse"` — a real, separate bug, fixed by `cargo build --release -p ekos` against
current `main`). Rebuilding and rerunning against the identical file reproduced the *exact same*
error, same line, same column — proving the fault is in dialect resolution or grammar, not this
session's setup.

**Root cause, confirmed by reading `sqlparser`'s own source** (the pinned
`sqlparser = "0.53"`, `~/.cargo/registry/src/.../sqlparser-0.53.0/src/parser/mod.rs:6410`,
`parse_optional_column_option`): this is one large `if`/`else if` chain matching specific
`Keyword` variants. ClickHouse-specific column options *do* exist here —
`dialect_of!(self is ClickHouseDialect | GenericDialect) && self.parse_keyword(Keyword::MATERIALIZED)`,
`...ALIAS`, `...EPHEMERAL` are all real branches (`mod.rs:6431-6448`) — so this isn't a case of
ClickHouse support being entirely absent. `CODEC` simply isn't one of the keywords this function
(or anywhere else in the crate — confirmed by a zero-hit grep for `CODEC` across the entire
vendored crate source) recognizes. `Keyword::CODEC` doesn't exist in `sqlparser`'s keyword table,
so `` `timestamp` DateTime CODEC(...) `` tokenizes `CODEC` as a plain identifier, none of
`parse_optional_column_option`'s branches match it, the column-option loop in `parse_column_def`
exits, and the outer column-list parser then chokes on an unexpected trailing token where it
expected `,` or `)` — exactly the reported error. Confirmed this gap is not version-specific to
0.53 either: `sqlparser`'s current `ColumnOption` enum (checked against the published API docs)
has 23 variants and no `Codec`/`Compression` variant among them — this is a real, still-open
upstream gap in `apache/datafusion-sqlparser-rs`, not something already fixed in a newer release
this project could simply upgrade into.

## Scope

Extend `ekos-plugin-sql-dialect-clickhouse`'s `ClickHouseDialectParser::preprocess` — currently a
no-op, with a moduledoc explicitly (and, as of this RFC, incorrectly) claiming "No preprocessing
is needed" — to strip `CODEC(...)` clauses from column definitions before the SQL reaches
`sqlparser`, the same architectural slot `MySqlDialectParser` already uses to strip `DELIMITER`
directives (RFC 0031's own precedent: dialect grammar `sqlparser` doesn't support gets normalized
away in `preprocess`, not worked around downstream). Both `SqlAnalyzerPass` (which calls
`dialect_parser.preprocess` internally, `sql_analyzer.rs:69`) and `SqlTransformAnalyzerPass`
(preprocessed explicitly in `recover.rs:141` before construction) pick this up for free — no
caller-side change needed.

## Non-goals

- **Not patching or forking `sqlparser`.** RFC 0056 explicitly chose the pinned crates.io
  `sqlparser = "0.53"` over any native/forked alternative to keep zero new dependency risk; a
  vendored fork would reverse that decision for one clause. An upstream PR to
  `apache/datafusion-sqlparser-rs` adding a real `ColumnOption::Codec` variant is the *correct*
  long-term fix and is worth filing separately, but this codebase doesn't control that review
  timeline and a real user's request doesn't wait on it.
- **Not modeling codec choice in the KIR.** `ClickHouseAnalyzerPass`'s (RFC 0056 Stage 1)
  `properties["columns"]` shape is `{name, data_type}` only — it doesn't capture codecs today
  even from live `system.columns` introspection. Stripping `CODEC(...)` textually loses no
  information this pass would otherwise have captured; it isn't a regression against Stage 1's
  live path, only a repair of the file-based DDL path Stage 1 never touches.
- **Not a general "make every real ClickHouse DDL file parse" guarantee.** Real ClickHouse schemas
  have other extension points `sqlparser` may not support (`TTL` expressions, `CODEC` on
  `ALTER TABLE ... MODIFY COLUMN`, exotic `ENGINE` parameter grammar). This RFC fixes the one gap
  a real file (`analytics/priv/ingest_repo/structure.sql`) actually hit; further gaps get their
  own RFC when a real file hits them, the same just-in-time discipline this project already
  applies everywhere else.

## Design

`strip_codec_clauses(sql: &str) -> String` in `plugins/sql-dialect-clickhouse/src/lib.rs`, in the
same style as `MySqlDialectParser::strip_delimiter_directives` (hand-written scanner, no new
dependency — `regex` is not a workspace dependency today and one clause doesn't justify adding
it):

- Scans byte-by-byte, tracking whether the cursor is inside a single-quoted string
  (`supports_string_literal_backslash_escape` is `true` for `ClickHouseDialect`, so a literal
  backslash-escaped quote must not be treated as the string's end) or a backtick-quoted identifier
  (ClickHouse's own quoting for column/table names, used throughout the real fixture file —
  `` `timestamp` ``, `` `hostname` ``) — `CODEC` must never be matched inside either, since a
  string or identifier could coincidentally contain those five letters.
- Outside a string/identifier, matches the literal word `CODEC` (case-sensitive — real ClickHouse
  DDL, including every example in `structure.sql`, always emits it uppercase; `sqlparser` itself
  has no case-insensitive fallback for unrecognized identifiers either, so this matches the
  dialect's own real-world convention rather than over-generalizing) at a word boundary (not a
  substring of a longer identifier).
- On a match, skips forward past optional whitespace to the following `(`, then counts balanced
  parentheses to find the matching `)` — required because real codec expressions nest, e.g.
  `CODEC(ZSTD(3))` and `CODEC(Delta(4), LZ4)`, both present in the real fixture file — and removes
  the whole `CODEC(...)` span, replacing it with nothing (not even a placeholder token; the
  column-option loop simply sees one fewer option, which is exactly correct — `CODEC` is optional
  everywhere it appears).
- If `CODEC` is matched but never followed by a `(` (malformed input), the word is left alone and
  parsing fails downstream exactly as it does today — this preprocessing step only removes
  well-formed clauses it's certain how to remove, never guesses.

`ClickHouseDialectParser::preprocess` calls this function; the crate's moduledoc comment claiming
"No preprocessing is needed" is corrected in the same change (a comment asserting something this
RFC just proved false must not survive it).

## Alternatives Considered

- **Forking/patching `sqlparser` locally via a `[patch]` crates.io override.** Rejected — reverses
  RFC 0056's explicit zero-new-dependency-risk decision, and a local patch drifts silently from
  upstream on every `sqlparser` version bump. Filing the real fix upstream is the correct channel;
  this RFC's preprocessing strip is the pragmatic bridge until (if ever) that lands.
- **Skipping the whole file/statement on a CODEC clause instead of stripping it.** Already the
  status quo (`falling back to empty graph`) — the entire motivation for this RFC is that it loses
  every table in a file, not just the columns using `CODEC`.
- **Regex-based stripping using the `regex` crate.** Rejected on the same "no new dependency for
  one clause" grounds `MySqlDialectParser` already established; a hand-written balanced-paren
  scanner is a few more lines and already proven correct in this exact codebase for a structurally
  similar problem (nested delimiters).

## Testing

- Unit tests in `plugins/sql-dialect-clickhouse/src/lib.rs`, same shape as
  `MySqlDialectParser`'s: `preprocess_is_identity_when_no_codec_clause_present`,
  `preprocess_strips_a_simple_codec_clause`, `preprocess_strips_a_nested_codec_clause` (
  `CODEC(ZSTD(3))`), `preprocess_strips_a_multi_arg_codec_clause` (`CODEC(Delta(4), LZ4)`),
  `preprocess_does_not_strip_codec_inside_a_string_literal`, `preprocess_does_not_strip_codec_inside_a_backtick_identifier`.
- Regression test using a real excerpt from `analytics/priv/ingest_repo/structure.sql` (the exact
  two-column snippet that failed live) as a fixture: `clickhouse_dialect_parses_real_codec_bearing_column_after_preprocessing`
  — preprocess, then `Parser::parse_sql`, asserting `Ok`.
- Full workspace gate: `cargo build --workspace && cargo test --workspace && cargo clippy
  --workspace -- -D warnings && cargo fmt --check`.
- Live verification (this project's established discipline — RFC 0054/0055/0056 each found a real
  bug invisible to `cargo test` alone): rerun `ekos recover` against the real `analytics/` repo
  after rebuilding, confirm the CODEC warning is gone and `ekos query find` surfaces real
  `ObjectKind::Table` objects for `events_v2`/`sessions_v2` with populated `properties["columns"]`.

## Acceptance Criteria

- [x] `ClickHouseDialectParser::preprocess` strips `CODEC(...)` clauses (including nested/
      multi-arg) without touching string literals or backtick identifiers containing the word.
- [x] Moduledoc no longer claims "No preprocessing is needed."
- [x] `cargo test -p ekos-plugin-sql-dialect-clickhouse` covers the cases in Testing (11 tests,
      all passing).
- [x] Full workspace `cargo build/test/clippy/fmt` clean.
- [x] **Live, partial** — rebuilt `target/release/ekos` and reran `ekos recover` against the real
      `analytics/` repo. The CODEC parse failure is gone: the reported error moved from line 7,
      column 23 (`CODEC`) to line 49, column 28 (`INDEX minmax_timestamp timestamp TYPE minmax
      GRANULARITY 1` — a table-level secondary-index definition inside the column list, a
      completely separate, unrelated `sqlparser` gap this RFC does not touch). **This criterion is
      not fully met**: `structure.sql` still doesn't produce `Table`/`Column` KIR objects, because
      `sqlparser`'s `ClickHouseDialect` support for `CREATE TABLE` is narrower than one clause —
      `INDEX ... TYPE ... GRANULARITY`, `PARTITION BY` (confirmed separately: gated to
      `BigQueryDialect | PostgreSqlDialect | GenericDialect`, ClickHouse excluded,
      `parser/mod.rs:6236`), and `SETTINGS` (no `CREATE TABLE` handling anywhere in the crate) are
      all still unsupported and would each need their own preprocessing pass, the same as this
      RFC's `CODEC` fix, to fully unblock this file. Scoped out of this RFC deliberately per its
      own Non-goals — reported to the user rather than silently expanded into a much larger,
      unscoped "make all real ClickHouse DDL parse" effort.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0057-clickhouse-codec-preprocessing.md` | This RFC |
| `ekos/plugins/sql-dialect-clickhouse/src/lib.rs` | `strip_codec_clauses`, `preprocess` override, corrected moduledoc, new tests |
