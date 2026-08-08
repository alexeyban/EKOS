# Devlog 39 — RFC 0038/0039 Phase 1: finished RFC 0031, closed a real whole-file-drop bug

**Date:** 2026-08-08
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Started RFC 0038's Phase 1 ("close existing SQL/Pentaho gaps") and found the real scope was
larger than the roadmap assumed. RFC 0031 (pluggable SQL dialects, marked Accepted) had an
unchecked acceptance criterion that was never actually implemented: `SqlTransformAnalyzerPass`
still had its own private `dialect_for` match, completely independent of the shared dialect
registry — adding a dialect to the registry silently didn't apply to Transformation IR recovery.
Worse, verifying real `sqlparser` 0.53 behavior directly (not assuming from the code's own doc
comments) found that MSSQL stored procedures using `IF`/`WHILE` control flow fail to parse
*entirely* — not "become one `Unmapped` node" as documented, but silently drop the whole
containing file's transform graphs, including unrelated statements. Both fixed this session, plus
`snowflake` and `databricks` shipped as real, independently-registered SQL dialect plugins (RFC
0038's originally-scoped Phase 1 deliverable) — and a third, previously-unscoped plugin
(`sql-dialect-mssql`) that turned out to be a prerequisite for the first fix, not an addition.

---

## What was found and fixed

### RFC 0031 was never fully completed

RFC 0031's own Acceptance Criteria (still visible as unchecked boxes in the RFC file) included:
"`SqlAnalyzerPass` and `SqlTransformAnalyzerPass` both take a resolved `SqlDialectParser` instead
of ... `SqlTransformAnalyzerPass` owning its own private `dialect_for`." Confirmed in the current
source: `SqlTransformAnalyzerPass::new` still took a bare `&str` dialect name and called its own
private `dialect_for(name)` match, entirely independent of `sql_dialect_registry.rs`'s
`build_dialect_registry()`. Fixed: `SqlTransformAnalyzerPass` now takes the resolved
`Box<dyn Dialect + Send + Sync>` directly from the caller (`recover.rs`, via
`dialect_parser.sqlparser_dialect()` — the same object `SqlAnalyzerPass` already uses), and the
private `dialect_for` is deleted entirely.

### A real whole-file-drop bug, worse than documented

Verified directly against real `sqlparser` 0.53: `Statement`'s grammar has zero support for `IF`/
`WHILE` in any dialect. A minimal real MSSQL procedure (`IF EXISTS (...) BEGIN SELECT ... END`)
fails `Parser::parse_sql` outright. Since procedure bodies parse inline as part of the whole-file
statement list (not separately), this failure happens during the *entire file's* parse — meaning
before this fix, a file with one control-flow-using procedure and five independent, perfectly
parseable `CREATE VIEW`s lost all six, with only a `tracing::warn!` and `return Vec::new()`. The
existing per-statement `Unmapped` handling inside `procedure_body_to_graph` was real and correct
— it was just unreachable, because the file never got that far.

Fixed with a whole-file fallback: when full-file structured parsing plus the existing
missing-`;` repair retry both fail, split on top-level `;` and retry each fragment independently
(the same style `function_to_graph` already used for Postgres function bodies, generalized one
level up). Statement dispatch was factored into a shared `dispatch_one_statement` function so the
happy path and the fallback path can't silently drift apart. Honest, stated limitation: the
`;`-split doesn't track nested `BEGIN...END`, so a failing procedure's own internal fragments can
produce several partial/duplicate `Unmapped` nodes instead of one clean one — an approximation,
not a silently wrong answer, and the procedure's control flow was never going to be modeled
either way.

Verified live, not just by unit test: a real `ekos recover` run against a fresh scratch workspace
with this exact shape logged `"falling back to per-statement recovery"` and produced real stats
— `statements=2 nodes=4 mapped=3 coverage_pct=75.0` — where the pre-fix behavior would have been
zero nodes for the whole file.

### A gap found while fixing the first gap: MSSQL was never in the registry at all

Unifying both passes on the shared dialect registry would have silently broken every existing
`dialect = "mssql"`/`"tsql"`/`"synapse"` workspace config: the registry (`sql_dialect_registry.rs`)
never had an `"mssql"` entry — MSSQL was only ever reachable through the old private `dialect_for`.
Added `plugins/sql-dialect-mssql` (a new `MsSqlDialectParser`, registered under all three real
aliases the old code recognized) before doing the unification, so no regression. Also preserved
the `"postgresql"` alias the same way, for the same reason.

### `snowflake` and `databricks` dialect plugins (RFC 0038's originally-scoped deliverable)

Both wrap `sqlparser`'s already-available `SnowflakeDialect`/`DatabricksDialect` — zero new
dependency. Real distinguishing behavior verified directly, not assumed: `SnowflakeDialect`
accepts a trailing comma in a `SELECT` projection list (`SELECT a, b, FROM t`), which
`GenericDialect` rejects outright; `DatabricksDialect` accepts backtick-delimited identifiers.
One inaccurate assumption caught while writing the Snowflake test: Snowflake's real `$$`-quoted
procedure/function body syntax is *not* supported by `sqlparser` 0.53's `CREATE PROCEDURE`/
`CREATE FUNCTION` grammar at all (that grammar only accepts MSSQL's `BEGIN...END` shape and
Postgres's single-string-literal shape respectively) — documented as a real, disclosed gap in the
plugin's own doc comment rather than left as an untested assumption.

---

## Knowledge Captured

- **A hardcoded fallback path duplicating a "pluggable" registry's job is a real, recurring
  failure mode in this codebase** — this is the second time this session a formerly-hardcoded
  match (`pentaho_analyzer.rs`'s join-key extraction, RFC 0036/0037's Phase 2 findings) turned out
  to silently diverge from what the surrounding architecture claimed was configurable. Worth
  checking for the same pattern anywhere else a "registry" or "pluggable X" abstraction coexists
  with an older, not-fully-migrated code path.
- **`sqlparser` 0.53's procedural-body grammar is dialect-specific in ways that aren't obvious
  from the `Dialect` trait alone** — MSSQL gets `CREATE PROCEDURE ... AS BEGIN ... END` parsed
  into real `Vec<Statement>`; Postgres gets a single opaque string-literal body; Snowflake's real
  `$$`-quoted body shape isn't parseable as either `CreateProcedure` or `CreateFunction` at all.
  Any future dialect plugin should verify its real procedural-body shape directly against
  `sqlparser`, not assume the `CreateProcedure`/`CreateFunction` statement variants are
  dialect-agnostic.
- **Real bugs keep coming from verifying documented behavior directly, not from reading the
  documentation.** The module's own doc comment claimed control flow "becomes `Unmapped`" — true
  in the narrow case that had a test, false in general. A two-line manual `sqlparser` check (not
  even a full test) found the real gap in under a minute.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0039-sql-pentaho-phase1-gaps.md` | new — full RFC, all acceptance criteria closed |
| `ekos/crates/recovery/src/sql_transform_analyzer.rs` | Unified dialect resolution; whole-file fallback; `dispatch_one_statement` factored out; regression test |
| `ekos/crates/recovery/src/sql_dialect_registry.rs` | `snowflake`, `databricks`, `mssql`/`tsql`/`synapse`, `postgresql` registered |
| `ekos/plugins/sql-dialect-snowflake/`, `ekos/plugins/sql-dialect-databricks/`, `ekos/plugins/sql-dialect-mssql/` | 3 new crates |
| `ekos/crates/cli/src/commands/recover.rs` | Pass resolved dialect object into `SqlTransformAnalyzerPass::new` |
