# RFC 0031 — Pluggable SQL Dialect Parsers

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-05

---

## Motivation

SQL recovery has two independent, inconsistent dialect stories today, both found while testing
EKOS cold against two real public GitHub ETL repos (devlog_31; the AdventureWorks fixture at
`joseph-higaki/etl_adventureworks_sales_purchases_datamart` mixes MySQL and MSSQL `.sql` files
in different folders of the same workspace):

- `SqlAnalyzerPass` (`crates/recovery/src/sql_analyzer.rs:153`) hardcodes `GenericDialect {}`
  directly when parsing `CREATE TABLE` DDL — there is no dialect parameter on this pass at all.
- `SqlTransformAnalyzerPass` has its own private `dialect_for(name: &str)` helper
  (`sql_transform_analyzer.rs:168-176`) supporting `postgres`/`mssql`/`databricks` — but
  `crates/cli/src/commands/recover.rs:92` always passes the literal string `"generic"`. There is
  no config knob anywhere, and **`"mysql"` is not one of the supported names**, despite MySQL
  being one of the most common Pentaho/Kettle ETL targets. Confirmed failing on the
  AdventureWorks fixture: files starting with a `#`-style MySQL line comment, and files with
  multiple statements, both fail to parse under `GenericDialect` (`sqlparser` errors: "Expected:
  an SQL statement, found: #" / "Expected: end of statement, found: update") — this is the root
  cause behind GitHub issue #3.

No RFC currently owns dialect selection — the existing behavior is ad hoc, described only in
code comments referencing an external, not-checked-in "implementation plan" document.

## Scope

- An explicit ANSI-SQL baseline (`GenericDialect`, sqlparser's existing default) as the
  unconditional fallback when no more specific dialect is configured — "first, follow generic
  ANSI SQL rules."
- Two new first-class, independently pluggable dialect parsers: **MySQL** and **PostgreSQL**,
  covering `CREATE TABLE` DDL, `SELECT`/`CREATE VIEW`, and `CREATE PROCEDURE`/`CREATE FUNCTION`
  *headers* plus any simple embedded SELECT/INSERT/UPDATE/DELETE statements in their bodies.
- Config-driven dialect selection per file path (a real workspace mixes dialects by folder, as
  the AdventureWorks fixture demonstrates — a single global setting can't express that).
- A registration mechanism new dialects can be added to without touching the two SQL recovery
  passes' internals — "connect a new one by configuration [+ a small new crate]."

## Non-goals

- **Dynamic/runtime plugin loading** (`dlopen`/`libloading`, a `.so`/`.dylib` loaded with no
  rebuild of `ekos` itself). Every existing "pluggable" concept in this codebase (`Observer`
  impls under `plugins/`) is compile-time: a separate Cargo workspace crate, statically linked,
  selected by name in Rust source. Introducing genuine dynamic loading would be new,
  unprecedented infrastructure requiring a new class of `unsafe` this codebase has never had —
  out of scope here; revisit only if a concrete need for out-of-tree/no-rebuild plugins appears.
  Same underlying non-goal as RFC 0006's. _Tracked as backlog: see `TODO.md` → "Promoted from
  RFC Non-Goals" → "MCP / connector infrastructure"._
- **Deep procedural-body parsing** (`IF`/`LOOP`/cursors/variable declarations) for MySQL or
  Postgres. Confirmed this session: `sqlparser` 0.53 (the version already pinned) only captures
  `CREATE FUNCTION`/`CREATE PROCEDURE` *headers* — the body is an opaque `Expr`/string for every
  dialect, not a structured AST. Real structured PL/pgSQL parsing exists via `pg_query` (Rust
  bindings to the real `libpg_query` C library) but pulls in a C dependency/bindgen build step;
  no mature equivalent exists for MySQL's procedural grammar at all. **Decision: skip this for
  v1 for both dialects.** Control-flow bodies become an honest `Unmapped` node, exactly the
  fidelity `sql_transform_analyzer.rs:508-546` already gives MSSQL procedures today — no
  dialect is silently worse than another, and no new dependency class is introduced.
  _Tracked as backlog: see `TODO.md` → "Promoted from RFC Non-Goals" → "Analyzers"._
- Re-architecting the already-working MSSQL/Databricks paths, or Informix's documented
  `GenericDialect` fallback (no dedicated `sqlparser` dialect exists for it).

## Design

### Why this cannot just be "add `\"mysql\"` to the existing `match`"

The one-line fix (add a `mysql` arm to `sql_transform_analyzer.rs`'s `dialect_for`) doesn't
address the actual problem: `SqlAnalyzerPass` has **no** dialect concept whatsoever, dialect
selection has no config surface at all today, and the two passes would keep diverging
independently. This RFC unifies both passes on one resolved dialect per file and gives that
resolution a real config surface, using the pattern already proven for `Observer` plugins
rather than inventing a new one.

### `SqlDialectParser` — new crate `ekos-sql-dialect-sdk`

Mirrors `observation-sdk`'s deliberately tiny trait surface (`Observer` is two methods, no
associated types, no macros):

```rust
pub trait SqlDialectParser: Send + Sync {
    /// Registry key — "mysql", "postgres", "generic".
    fn name(&self) -> &str;
    /// Which sqlparser dialect to parse with.
    fn sqlparser_dialect(&self) -> Box<dyn sqlparser::dialect::Dialect>;
    /// Dialect-specific text preprocessing before handing to sqlparser — e.g. stripping
    /// MySQL's `DELIMITER $$ ... $$` client convention, which is not real SQL grammar and no
    /// sqlparser dialect understands it. Default: identity (no preprocessing needed).
    fn preprocess(&self, sql: &str) -> String {
        sql.to_string()
    }
}
```

Deliberately does **not** duplicate DDL/transform-graph construction — `SqlAnalyzerPass` and
`SqlTransformAnalyzerPass` keep all their existing, working IR-building code. A dialect crate's
only job is supplying *which* `sqlparser::dialect::Dialect` to parse with and any text
preprocessing its dialect needs. This keeps the new crates genuinely small.

### Two new dialect crates, same shape as `plugins/pentaho`/`plugins/git`

- `plugins/sql-dialect-mysql` — `MySqlDialectParser`, wraps `sqlparser::dialect::MySqlDialect`
  (already available in the pinned `sqlparser = "0.53"`; handles backtick identifiers,
  `AUTO_INCREMENT`, `ENGINE=...` table options). `preprocess` strips `DELIMITER $$ ... $$` /
  `DELIMITER ;` lines.
- `plugins/sql-dialect-postgres` — `PostgresDialectParser`, wraps
  `sqlparser::dialect::PostgreSqlDialect` (already used today only inside
  `sql_transform_analyzer.rs`'s private `dialect_for`; this makes it independently selectable
  and testable, and — new — wires it into `SqlAnalyzerPass`, which has never had Postgres-aware
  DDL parsing).

Both depend only on `sqlparser` + `ekos-sql-dialect-sdk` — no dependency on
`compiler-core`/`cli`, matching existing plugin dependency discipline.

### Registry — the "connect a new one by configuration" mechanism

```rust
fn build_dialect_registry() -> HashMap<String, Box<dyn SqlDialectParser>> {
    let mut r: HashMap<String, Box<dyn SqlDialectParser>> = HashMap::new();
    r.insert("generic".into(), Box::new(GenericDialectParser));
    r.insert("mysql".into(), Box::new(MySqlDialectParser));
    r.insert("postgres".into(), Box::new(PostgresDialectParser));
    r
}
```

A new dialect is added exactly the way a new `Observer` is added today: write the crate, add it
as a workspace member + dependency, add one line to this registry. This is the existing pattern
in this codebase, not a promise of drop-in-a-`.so`-with-no-rebuild — stated explicitly so this
RFC isn't read as over-promising true dynamic plugins (see Non-goals).

### Config — per-path dialect rules, ANSI as the unconditional default

`EkosConfig` uses `#[serde(deny_unknown_fields)]` (`compiler-core/src/config.rs:4-19`) — add a
new, optional, additive `[recover.sql]` section:

```toml
[recover.sql]
default-dialect = "generic"        # ANSI/GenericDialect baseline — today's behavior, unchanged

[[recover.sql.dialect-rules]]
path-glob = "**/mysql/**/*.sql"
dialect = "mysql"

[[recover.sql.dialect-rules]]
path-glob = "**/postgres/**/*.sql"
dialect = "postgres"
```

`recover.rs`'s existing per-file `.sql` loop resolves a dialect once per file: first matching
`path-glob` wins, else `default-dialect` (defaults to `"generic"` if the section is omitted
entirely — no config change required for existing workspaces). The resolved
`Box<dyn SqlDialectParser>` (or its `sqlparser_dialect()`/`preprocess()` outputs) is passed to
**both** `SqlAnalyzerPass::new` and `SqlTransformAnalyzerPass::new`, replacing
`SqlAnalyzerPass`'s total lack of dialect awareness and `SqlTransformAnalyzerPass`'s private
`dialect_for` with one shared resolution.

## Alternatives Considered

**True dynamic/runtime plugin loading (`dlopen`, a `.so`/`.dylib` loaded without rebuilding
`ekos`).** Rejected for v1. This codebase has zero existing dynamic-loading infrastructure
(confirmed: no `libloading`/`cdylib`/`dlopen` anywhere), and the "zero `unsafe` unless formally
justified in an RFC" coding rule means this would need its own dedicated design (a stable
ABI or serialization boundary, process isolation questions, versioning). Nothing about SQL
dialect parsing specifically requires this — the compile-time-crate-plus-config-selection
pattern already used for every `Observer` plugin satisfies "write a new dialect, connect it by
configuration" without inventing new unsafe surface area. Revisit only if a concrete need for
out-of-tree, no-rebuild dialect plugins appears.

**Per-file dialect auto-detection by sniffing syntax** (e.g. "if we see backticks, assume
MySQL"). Rejected — unreliable (a file with no dialect-specific syntax gives no signal;
`GenericDialect`-compatible SQL is dialect-ambiguous by construction) and silently wrong
guesses are worse than an explicit, visible config requirement. Explicit `path-glob` rules also
directly match the real evidence found this session (dialect boundaries align with folder
structure, e.g. `DB Scripts/Destination MySQL/` vs. `DB Scripts/Source MSSQL/`).

**`pg_query` for full PL/pgSQL body parsing now.** Rejected for v1 — see Non-goals. A real,
available tool for Postgres specifically, but asymmetric with MySQL (no equivalent exists) and
a new C-dependency class for this workspace; revisit as a follow-up RFC if procedural body
fidelity becomes a concrete need.

## Testing

- Unit tests per dialect crate: `MySqlDialectParser`/`PostgresDialectParser` `preprocess()` and
  `sqlparser_dialect()` behavior, mirroring the existing fixture-style tests in
  `pentaho_analyzer.rs`.
- Regression fixtures already on disk from this session's testing:
  `etl_adventureworks_sales_purchases_datamart` (its MySQL folder should go from 0% mapped,
  per devlog_31, to real `Table`/`TransformNode` output once `dialect = "mysql"` is configured
  for that path) and `pih-pentaho` (already-passing SQL must keep its current 100%-mapped
  result — no regression from unifying the two passes on the registry).
- Full workspace gate unchanged: `cargo test --workspace`, `cargo clippy --workspace -- -D
  warnings`, `cargo fmt --check`.

## Acceptance Criteria

- [ ] `ekos-sql-dialect-sdk` crate exists with the `SqlDialectParser` trait as specified.
- [ ] `plugins/sql-dialect-mysql` and `plugins/sql-dialect-postgres` crates exist, each with
      unit tests, and depend only on `sqlparser` + `ekos-sql-dialect-sdk`.
- [ ] `EkosConfig` accepts an optional `[recover.sql]` section with `default-dialect` and
      `dialect-rules`; omitting the section preserves today's `"generic"`-everywhere behavior.
- [ ] `SqlAnalyzerPass` and `SqlTransformAnalyzerPass` both take a resolved
      `SqlDialectParser` instead of `SqlAnalyzerPass` hardcoding `GenericDialect` and
      `SqlTransformAnalyzerPass` owning its own private `dialect_for`.
- [ ] `etl_adventureworks_sales_purchases_datamart`'s MySQL `.sql` files parse successfully
      (>0% mapped) once configured with `dialect = "mysql"` for that path.
- [ ] `pih-pentaho`'s SQL recovery results are unchanged (no regression).
- [ ] `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`
      all pass.
