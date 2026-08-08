# RFC 0039 — Phase 1: Close SQL/Pentaho Gaps (RFC 0038 Phase 1)

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-08

---

## Motivation

RFC 0038's Phase 1 scoped "close existing SQL/Pentaho gaps" at a high level. Investigating the
actual code before implementing surfaced two real, more serious gaps than the roadmap assumed:

1. **RFC 0031 (pluggable SQL dialects) was never fully completed.** Its own Acceptance Criteria
   still show an unchecked box: "`SqlAnalyzerPass` and `SqlTransformAnalyzerPass` both take a
   resolved `SqlDialectParser` instead of ... `SqlTransformAnalyzerPass` owning its own private
   `dialect_for`." Confirmed in the current source: `SqlTransformAnalyzerPass::new` still takes a
   bare `&str` dialect name and calls its own private `dialect_for(name)` match
   (`sql_transform_analyzer.rs:170-180`), completely independent of `sql_dialect_registry.rs`'s
   `build_dialect_registry()`. Adding a new dialect to the registry (e.g. Snowflake) makes
   `SqlAnalyzerPass` (DDL) use it correctly, but `SqlTransformAnalyzerPass` (Transformation IR)
   silently keeps falling through to `GenericDialect` — the two passes can disagree about which
   dialect parsed the same file.
2. **MSSQL stored procedures containing `IF`/`WHILE` control flow fail to parse *at all* today,
   not just "become `Unmapped`" as the code's own doc comments claim.** Verified directly:
   `sqlparser` 0.53's `Statement` enum has no `IF`/`WHILE` grammar for any dialect. A minimal real
   MSSQL procedure body (`IF EXISTS (...) BEGIN SELECT ... END ELSE BEGIN SELECT ... END`) fails
   `Parser::parse_sql` outright ("Expected: an SQL statement, found: IF"). Since this happens
   during the *whole-file* parse (procedure bodies are parsed as nested statement lists inline,
   not separately), the current fallback (`statement_repair`'s missing-`;` retry) doesn't help —
   it's not a missing-separator problem — and `parse_sql_to_transform_graphs` returns `Vec::new()`
   for the **entire file**, silently dropping every other statement in it too (independent
   `SELECT`s, `CREATE VIEW`s, anything else), not just the procedure. `procedure_body_to_graph`'s
   per-statement `Unmapped` handling (line 536-544) is real and correct — it's just unreachable
   for any procedure containing `IF`/`WHILE`, because the file never gets that far.

This RFC's scope, revised from RFC 0038's original Phase 1 description accordingly:

## Scope

1. Finish RFC 0031: `SqlTransformAnalyzerPass` uses the same resolved `SqlDialectParser`
   (`Box<dyn Dialect + Send + Sync>`) `SqlAnalyzerPass` already gets from the registry, instead of
   its own private `dialect_for`. The dialect *name* string stays (still needed for
   `SqlTransformStats.dialect` and `TransformOrigin.source_kind` display/tagging, per the existing
   doc comment's stated reason) — only the actual `sqlparser::Dialect` selection is unified.
2. Add `snowflake` and `databricks` as real, independently-registered `SqlDialectParser` plugins
   (`plugins/sql-dialect-snowflake`, `plugins/sql-dialect-databricks`), wrapping `sqlparser`'s
   already-available `SnowflakeDialect`/`DatabricksDialect` — same shape as the existing
   `mysql`/`postgres` plugins. Once (1) is done, both automatically apply to *both* passes. **Also
   add `plugins/sql-dialect-mssql`** — found while implementing (1): the registry never had an
   `"mssql"`/`"tsql"`/`"synapse"` entry at all, only `sql_transform_analyzer.rs`'s old private
   `dialect_for` recognized those names, so unifying both passes on the registry without this
   addition would have silently broken existing MSSQL workspace configs.
3. Fix the whole-file-drop bug: when full-file structured parsing fails, fall back to a
   per-top-level-statement text-splitting retry (same style `function_to_graph` already uses for
   Postgres function bodies) so independent statements in the same file survive even when one
   `CREATE PROCEDURE` in it uses unparseable control flow. Documented limitation: naive `;`-
   splitting doesn't respect nested `BEGIN...END` blocks, so a failing procedure's own internal
   fragments may come out as multiple partial/duplicate `Unmapped` nodes rather than one clean
   node — an honest approximation, not a silent wrong answer, consistent with this project's
   "accept incomplete coverage, never guess" posture.
4. `pentaho_analyzer.rs`'s still-unverified `DatabaseJoin` shape: no real sample found this pass
   either — left as the documented approximation it already is, not blocking.

## What already exists and is reused

- `SqlDialectParser` trait, `sql_dialect_registry.rs`, `plugins/sql-dialect-mysql`/`-postgres` —
  the exact pattern items 1-2 extend, not replace.
- `function_to_graph`'s existing per-fragment split-and-retry heuristic — item 3 generalizes this
  same approach one level up, to whole-file recovery.
- `sqlparser::dialect::SnowflakeDialect`/`DatabricksDialect` — already present in the pinned
  `sqlparser = "0.53"` dependency, zero new dependency.

## Design

**`sql_transform_analyzer.rs`**: `SqlTransformAnalyzerPass::new` gains a `dialect: Box<dyn Dialect
+ Send + Sync>` parameter (or accepts it via the caller passing `dialect_parser.sqlparser_dialect()`
directly, mirroring how `recover.rs` already calls `dialect_parser.preprocess(&sql)`).
`parse_sql_to_transform_graphs` takes `&dyn Dialect` instead of a name string for parsing;
`source_kind_for` stays name-keyed (pure display tag, gains a `"snowflake"` arm) since it doesn't
duplicate parsing behavior. `dialect_for`'s hardcoded match is deleted once the registry is the
only source of truth.

**New dialect crates**: `SnowflakeDialectParser`/`DatabricksDialectParser`, `name()` returns
`"snowflake"`/`"databricks"`, `sqlparser_dialect()` wraps the respective unit struct, no
preprocessing needed (same as Postgres — verified no dialect-specific text convention like MySQL's
`DELIMITER` applies to either). Registered in `build_dialect_registry()`.

**Whole-file fallback**: in `parse_sql_to_transform_graphs`, after the existing `statement_repair`
retry also fails, split `sql` on top-level `;` (reusing/extending the existing splitting logic
already proven in `function_to_graph`), attempt `Parser::parse_sql` on each fragment
independently, dispatch successfully-parsed fragments through the same `match stmt { ... }` the
whole-file path already uses, and represent any fragment that still fails to parse as one
`Unmapped` node with `reason: "statement-level parse failure (likely control flow): {error}"`.

## Alternatives Considered

- **A real BEGIN/END-aware statement splitter** (correctly bracket-matching nested blocks before
  splitting) — rejected for this phase; meaningfully more implementation risk for a benefit
  (perfectly clean single-node procedure failures vs. several partial/duplicate `Unmapped` nodes)
  that doesn't change the honest bottom line (procedure control flow still isn't modeled either
  way). Revisit only if the imperfect splitting is found to actively mislead a real user, not
  preemptively.
- **`pg_query`/a real T-SQL grammar for full procedural fidelity** — already rejected in RFC 0031's
  own Alternatives Considered, same reasoning still holds (new dependency class, asymmetric
  dialect coverage, no concrete need yet).

## Testing

- Unit tests for `SnowflakeDialectParser`/`DatabricksDialectParser`, mirroring
  `PostgresDialectParser`'s existing test style.
- Registry test extended to assert `snowflake`/`databricks` are present alongside
  `generic`/`mysql`/`postgres`.
- A test proving `SqlTransformAnalyzerPass` actually uses the registry's dialect object, not a
  hardcoded fallback, for a dialect only the registry knows about.
- A regression test using the exact real MSSQL `IF`/`BEGIN...END` shape verified against real
  `sqlparser` behavior this pass: a file with one such procedure *and* one independent, otherwise-
  parseable `CREATE VIEW` must recover the view instead of losing the whole file.

## Acceptance Criteria

- [x] RFC 0031's previously-unchecked acceptance criterion (`SqlTransformAnalyzerPass` uses the
      resolved dialect, not a private `dialect_for`) is now true and verified by test.
      `parse_sql_to_transform_graphs` takes `&dyn Dialect` from the caller; `dialect_for` is
      deleted entirely.
- [x] `snowflake` and `databricks` dialects are independently registered, unit-tested, and used by
      both `SqlAnalyzerPass` and `SqlTransformAnalyzerPass` once configured. **Scope grew by one
      real finding**: unifying both passes on the registry would have silently broken existing
      `dialect = "mssql"`/`"tsql"`/`"synapse"` workspace configs, since the registry never had an
      `"mssql"` entry at all (MSSQL was only ever reachable via the old private `dialect_for`).
      Added `plugins/sql-dialect-mssql` to close that gap before unifying — not originally
      scoped, found while implementing.
- [x] A file containing an unparseable-control-flow procedure alongside independent parseable
      statements recovers the independent statements — regression test
      (`independent_statement_survives_when_another_procedure_in_the_same_file_has_unparseable_control_flow`)
      passes, and confirmed live via a real CLI `ekos recover` run: before this fix such a file
      returned zero Transformation IR nodes; after, `statements=2 nodes=4 mapped=3
      coverage_pct=75.0` (real terminal output, this session).
- [x] `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`
      all pass (145 new/changed tests across 5 new plugin crates + `ekos-recovery`, zero failures).
- [x] Real-data smoke test: a real `ekos init && ekos build && ekos recover` run against a fresh
      scratch workspace with the exact real MSSQL `IF`/`BEGIN...END` shape verified against
      `sqlparser` 0.53 directly — real log output confirms the fallback engaged
      ("falling back to per-statement recovery") and real nodes were recovered.

## Files Changed

| File | Change |
|---|---|
| `ekos/crates/recovery/src/sql_transform_analyzer.rs` | Take resolved `Dialect` object instead of private `dialect_for`; whole-file fallback (`parse_sql_statement_by_statement`) for statement-level parse failures; `dispatch_one_statement` factored out so both paths share one statement→graph mapping |
| `ekos/crates/recovery/src/sql_dialect_registry.rs` | Register `snowflake`, `databricks`, `mssql`/`tsql`/`synapse`, `postgresql` alias |
| `ekos/plugins/sql-dialect-snowflake/` | new crate |
| `ekos/plugins/sql-dialect-databricks/` | new crate |
| `ekos/plugins/sql-dialect-mssql/` | new crate (found necessary while implementing, not originally scoped) |
| `ekos/crates/cli/src/commands/recover.rs` | Pass resolved dialect object to `SqlTransformAnalyzerPass::new` |
| `ekos/Cargo.toml`, `ekos/crates/recovery/Cargo.toml` | new workspace members/dependencies |
