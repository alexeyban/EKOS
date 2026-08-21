# Devlog 32 — RFC 0031: Pluggable SQL Dialect Parsers

**Date:** 2026-08-05
**PRs:** none yet — uncommitted, pending review
**Branch:** main (working tree)

---

## Summary

Follow-up to devlog_31's identity-resolution bug fix: this session addressed the underlying
SQL-dialect gap that GitHub issue #3 was filed against. SQL recovery had two independent,
inconsistent dialect stories — `SqlAnalyzerPass` had no dialect concept at all (always
`GenericDialect`), and `SqlTransformAnalyzerPass` had its own private dialect list that didn't
even include MySQL. Wrote RFC 0031, then built a compile-time-pluggable dialect architecture:
a new minimal `SqlDialectParser` trait, two new dialect crates (MySQL, PostgreSQL) following the
existing `Observer`-plugin pattern exactly, a config-driven registry, and a new `[recover.sql]`
`ekos.toml` section supporting per-path dialect rules. Verified against the real file that
GitHub issue #3 was filed about — zero LLM/Anthropic API calls used anywhere in this session's
verification, per an explicit request mid-session to conserve API credits.

---

## RFC 0031 — Pluggable SQL Dialect Parsers

### Problem / motivation

- `SqlAnalyzerPass` (`sql_analyzer.rs:153`, pre-fix) hardcoded `GenericDialect {}` with no
  dialect parameter on the pass at all.
- `SqlTransformAnalyzerPass` had its own private `dialect_for(name: &str)` supporting
  `postgres`/`mssql`/`databricks` — but `recover.rs:92` always passed the literal `"generic"`,
  and `"mysql"` wasn't even a supported name.
- Confirmed failing on a real public repo (`etl_adventureworks_sales_purchases_datamart`,
  analyzed in devlog_31): MySQL `#`-comments and multi-statement scripts both fail under
  `GenericDialect`.

### What was built

| Component | Location |
|---|---|
| `SqlDialectParser` trait | new crate `ekos-sql-dialect-sdk` |
| MySQL dialect parser | new crate `ekos-plugin-sql-dialect-mysql` |
| PostgreSQL dialect parser | new crate `ekos-plugin-sql-dialect-postgres` |
| Dialect registry + ANSI baseline | `ekos-recovery::sql_dialect_registry` |
| `[recover.sql]` config schema | `ekos-compiler-core::config` |
| Wiring | `crates/cli/src/commands/recover.rs` |

### Implementation details worth remembering

- **The trait is deliberately tiny** (`name()`, `sqlparser_dialect()`, `preprocess()` with a
  default no-op) — mirrors `Observer`'s two-method surface exactly. It does *not* duplicate
  IR-building logic; `SqlAnalyzerPass`/`SqlTransformAnalyzerPass` kept all their existing,
  working DDL/transform-graph code. A dialect crate's only job is "which `sqlparser` dialect"
  and "any text preprocessing before parsing."
- **`sqlparser_dialect()` must return `Box<dyn Dialect + Send + Sync>`, not bare `Box<dyn
  Dialect>`.** `sqlparser::dialect::Dialect` has no `Send`/`Sync` supertrait bound, and trait
  objects don't get auto-traits unless the trait object type says so explicitly. Since
  `CompilerPass: Send + Sync` and `SqlAnalyzerPass` now stores a `Box<dyn Dialect>` field, the
  unbounded version doesn't compile as a struct field. Found this by hitting the compiler error
  directly rather than reasoning it out in advance — worth remembering as a checklist item for
  any future trait wrapping a third-party non-Send/Sync-bounded trait.
- **`MySqlDialect` already handles `#`-comments — confirmed by reading `sqlparser` 0.53's own
  tokenizer source** (`'#' if dialect_of!(self is SnowflakeDialect | BigQueryDialect |
  MySqlDialect)`), not assumed. The GitHub issue #3 fix for the comment half is genuinely just
  "select the right dialect," no new parsing logic needed. The `DELIMITER $$...$$` stripping
  (a `mysql` CLI client convention, not real SQL grammar — no dialect understands it) is the one
  piece of real preprocessing the MySQL crate adds.
- **Path-glob dialect rules, not a single global dialect**, because the AdventureWorks fixture
  from devlog_31 mixes dialects by folder in one workspace (`DB Scripts/Destination MySQL/` vs.
  `.../Source MSSQL/`) — a real, previously-encountered case, not a hypothetical one. Used the
  `glob` crate (new dependency) rather than hand-rolling pattern matching.
- **`SqlTransformAnalyzerPass` kept its existing `dialect_name: String` API** rather than being
  changed to take `&dyn SqlDialectParser` like `SqlAnalyzerPass` did — the name string is also
  used for stats/display (`SqlTransformStats.dialect`) and `TransformOrigin` source-kind
  tagging, so threading a trait object through would've meant either duplicating the name
  separately or leaking dialect-parser internals into stats output. `recover.rs` resolves once,
  preprocesses once via the `SqlDialectParser`, and passes the resolved name + preprocessed text
  to both passes — no double-preprocessing, both passes see identical normalized SQL.
- **Explicitly scoped out** (stated in the RFC's Non-goals, not silently skipped): true dynamic
  plugin loading (confirmed zero `dlopen`/`libloading`/`cdylib` anywhere in this codebase before
  this session — the "plugin" pattern here has always meant "compile-time crate," and this RFC
  keeps it that way rather than inventing new `unsafe` surface); and deep procedural-body
  parsing (`IF`/`LOOP`/cursors) for MySQL or Postgres — `sqlparser` 0.53 only captures
  `CREATE PROCEDURE`/`CREATE FUNCTION` *headers* for every dialect, treating bodies as opaque
  `Expr`/string blobs. Postgres has a real option (`pg_query`, bindings to the actual
  `libpg_query` C library) but it's a new C-dependency class; MySQL has no mature equivalent at
  all. Decision: skip both for v1, control flow stays `Unmapped` for every dialect — same
  fidelity MSSQL procedures already got before this RFC.

### Decisions

- **Compile-time crate + config-selected, not dynamic loading** — confirmed with the user
  directly rather than assumed. Matches every existing plugin in this codebase; true `.so`
  loading would need a stable ABI/serialization boundary and new `unsafe`, unjustified by
  anything specific to SQL dialects.
- **Header + simple embedded statements only for procedures/functions, no `pg_query`** —
  confirmed with the user. Keeps both dialects at equal, honest fidelity rather than Postgres
  getting deep PL/pgSQL parsing while MySQL stays shallow with no equivalent tool available.

---

## Regression testing without spending LLM credits

Mid-session, the user asked to avoid using Anthropic API credits for the rest of the task. This
mattered because the obvious verification path — re-running `ekos recover` against the real
`etl_adventureworks_sales_purchases_datamart` clone from devlog_31 — calls the LLM once per SQL
file for `SqlAnalyzerPass`'s enrichment step, regardless of whether the dialect fix itself needs
it. Instead:

- Copied the exact real file that failed (`DB Scripts/Destination MySQL/
  create.eae_data_management_mmjja.sql`, verbatim) into `tests/fixtures/mysql_hash_comments.sql`
  as a permanent, checked-in regression fixture — avoids depending on an external clone even
  existing in CI, which the original manual verification approach would have.
- Added two unit tests directly against `parse_ddl_structural` (no `CompilerPass`, no LLM, no
  network): one proving `GenericDialect` still fails on the real file (0 tables — matching
  devlog_31's documented finding), one proving `MySqlDialect` recovers real tables
  (`fact_sales`, `dim_date`) from the same file. Both pure, local, deterministic — zero API cost,
  and a stronger regression test than a manual CLI run would have been (committed to the repo,
  runs in every future `cargo test`).

## Knowledge Captured

- **`sqlparser`'s `Dialect` trait has no `Send`/`Sync` bound** — any new trait wrapping
  `Box<dyn Dialect>` for storage in a `Send + Sync` struct (as every `CompilerPass` must be)
  needs `Box<dyn Dialect + Send + Sync>` explicitly. Non-obvious until the compiler says so;
  now documented in `ekos-sql-dialect-sdk`'s trait doc comment for the next person who copies
  this pattern for a different third-party trait.
- **This codebase's "plugin" concept has never meant runtime dynamic loading** — verified by
  grepping for `libloading`/`cdylib`/`dlopen` (zero hits) before designing anything, not
  assumed. Worth remembering before any future "make X pluggable" request: the existing,
  precedented answer is a new workspace crate + compile-time registration, not `.so` files.
- **Real bugs found by testing against real, uncontrolled data don't get fixed by adding one
  match arm.** The tempting one-line fix here (`"mysql" => Box::new(MySqlDialect {})` in the
  existing `dialect_for`) would have left `SqlAnalyzerPass` with zero dialect awareness forever
  and no config surface for anyone to actually select MySQL in the first place — the real fix
  needed unifying two independently-drifted code paths, not patching the one that happened to
  already have a `match`.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0031-pluggable-sql-dialects.md` | New RFC |
| `ekos/crates/sql-dialect-sdk/` | New crate — `SqlDialectParser` trait |
| `ekos/plugins/sql-dialect-mysql/` | New crate — MySQL dialect + DELIMITER stripping |
| `ekos/plugins/sql-dialect-postgres/` | New crate — PostgreSQL dialect |
| `ekos/crates/recovery/src/sql_dialect_registry.rs` | New — registry + `GenericDialectParser` + path-glob resolution |
| `ekos/crates/recovery/src/sql_analyzer.rs` | `SqlAnalyzerPass`/`parse_ddl_structural` now take a resolved dialect; 2 new regression tests against a real fixture |
| `ekos/crates/recovery/src/sql_transform_analyzer.rs` | Added `"mysql"` to `dialect_for`/`source_kind_for`; 2 new tests |
| `ekos/crates/cli/src/commands/recover.rs` | Resolves dialect per file via registry + config, unifies both passes on it |
| `ekos/crates/compiler-core/src/config.rs` | New `[recover.sql]` section (`default-dialect`, `dialect-rules`) |
| `ekos/Cargo.toml` | New workspace members/dependencies; added `glob` crate |
| `tests/fixtures/mysql_hash_comments.sql` | New — real MySQL DDL fixture (from the public AdventureWorks ETL repo) for regression testing without an external clone |
| `devlog_32.md` | This file |
