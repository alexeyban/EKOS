# Devlog 317 — PostgreSQL and ClickHouse as project databases

**Date:** 2026-11-01
**PRs:** none — committed to `main` directly
**Branch:** `main`

---

## Summary

The console's Sources tab offered only "Add an Oracle database": `[[databases]]` had one kind,
although EKOS already had a guarded read-only PostgreSQL connector (`pg-live`, EKOS Migrate's) and a
ClickHouse HTTP client. RFC 0235 adds `kind = "postgres"` and `kind = "clickhouse"`, read the way
RFC 0211 reads Oracle — once per build, read-only, a write probe that must be refused, `origin:
live` objects, a dropped object leaving the model — and the Sources form gets a Kind selector.
Verified against local PostgreSQL 16 and ClickHouse 24.8: census equal to compiled for tables,
views, routines and triggers, references 100% resolved. Two side fixes came out of the live run:
`database test` counted the append-only ledger (a dropped object read as a difference, Oracle too),
and the integrity check flagged ClickHouse built-ins.

---

## RFC 0235 — PostgreSQL and ClickHouse databases

### Problem / motivation
A PostgreSQL or ClickHouse project database could not be declared. ClickHouse had only RFC 0056's
environment-variable observer (one database per process, outside `ekos.toml`, no `origin: live`).

### What was built
| Component | What |
|---|---|
| `compiler-core` `DatabaseKind` | `Postgres`, `Clickhouse`; `as_str`; config check refuses `profile ≠ p0` and `dictionary` for them |
| `observation-sdk::live_catalog` | RFC 0211's artifact shape as shared types (`OwnerCatalog`, `LiveObject`, `artifacts`, `write_check`) |
| `plugins/postgres` (new crate) | `PgConnection::parse` (`host[:port]/database`), `introspect` + `reconcile` + `check_write_refused`, pure `normalize` |
| `plugins/clickhouse::catalog` | `ChConnection::parse` (`http[s]://host[:port]`), `system.tables`/`system.columns` with `readonly=1`, zero-row insert probe |
| `pg-live` catalog | constraint rows add `conkey`/`confkey` column lists and the referenced table; routine rows add `pg_get_functiondef`; stable `name`/`table` fields (all additive) |
| `recovery::oracle_catalog_analyzer` | `CatalogDialect` (scheme, id prefix, stamp, summary kind, view/routine readers); `OracleCatalogPass::for_dialect` |
| `kir::custom_kinds` | `PostgresCatalog`, `ClickHouseCatalog` (keyed) |
| CLI | `ekos database add --kind`, per-kind connect check, owners upper-cased for Oracle only; `database test` one JSON shape for every kind (async; PostgreSQL on a blocking thread) |
| Console | `DatabaseIn.kind`, `sources_write.database_add_argv(kind=…)` with per-kind connect patterns; Sources form Kind selector, hints, Oracle-only fields hidden, kind chip on each database |
| `semantic::xref_builtins` | `SqlDialect::ClickHouse` (a list + function-family prefixes); live objects choose built-ins by `dialect` |

### Implementation details worth remembering
- **Observers normalise, the analyzer stays one.** Both new observers emit `{kind, owner, name,
  status, detail}` in the Oracle vocabulary (`table` with folded `columns`, `primary_key`,
  `unique_constraint`, `check_constraint` with `condition`, `foreign_key` with `references_*`,
  `view` with `text`, `function`/`procedure`/`trigger` with `text`), so the analyzer change is only
  which reader a view or routine goes through.
- **PostgreSQL constraint names are unique per table, not per schema**, so constraint and trigger
  objects are named `<table>.<name>`; an overloaded routine is `<name>(<args>)`.
- `pg_get_viewdef` ends with `;` — stripped before `CREATE VIEW … AS` is put around it.
- `postgres` (the sync driver) starts its own runtime: `database test` runs it under
  `spawn_blocking`, as the observer does.
- A ClickHouse column is `nullable` only for `Nullable(…)`; sorting/primary keys are table
  properties (`sorting_key`, `engine_primary_key`), never a `primary_key` constraint — a ClickHouse
  primary key is not unique.
- Oracle's ids (`oracle-live-<prefix>:<key>`), pass name (`oracle-catalog:<ws>`) and diagnostics
  are unchanged; a test pins one id literally.

### Decisions
- *One analyzer, not three.* The table/FK/stale-object logic is the part worth not duplicating.
- *A new crate for PostgreSQL* (as `plugins/oracle` is for Oracle) rather than an observer inside
  `pg-live`, which stays the connector EKOS Migrate shares.
- *Profiles stay Oracle-only* and are refused, not ignored, for the other kinds.
- *RFC 0056's ClickHouse observer is kept*; the new one has its own source kind (`clickhouse-live`)
  so the two analyzers never read each other's artifacts.

---

## Fixes found on the live run

| Fix | Why |
|---|---|
| `database test` counts from `ckm/model.json` (ledger only when no model) | after dropping a view, recover dropped it but the test still said `views 1 / 2 ← differs`: the ledger is append-only |
| ClickHouse built-ins in the integrity check | `toDate` in a ClickHouse view was an `UnresolvedReference` error; live Oracle views had the same gap (their `oracle://` targets never matched the `/oracle/` path rule) |

---

## Knowledge Captured
- **Counting "compiled" from the ledger is wrong for anything that can disappear.** The ledger keeps
  every object ever written; the latest CKM is the current model.
- **`pg_get_constraintdef` qualifies a referenced table only when it is outside the search path** —
  read `confrelid`'s schema instead of parsing the definition.
- **`node_modules/.vite` owned by root** (an earlier container run) breaks `vitest`; run it with a
  config whose `cacheDir` points elsewhere instead of changing ownership.
- The migrate sandboxes (`docker-compose.migrate.yml`, ports 55432 / 58123) are a ready live testbed
  for both kinds; a read-only role is `GRANT USAGE` + `GRANT SELECT`, a ClickHouse one
  `SETTINGS readonly = 1` + `GRANT SELECT ON db.*, system.*`.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0235-postgres-and-clickhouse-databases.md` | the RFC |
| `ekos/crates/compiler-core/src/config.rs` | two kinds, Oracle-only checks, test |
| `ekos/crates/observation-sdk/src/{lib.rs,live_catalog.rs}` | shared artifact shape |
| `ekos/plugins/postgres/` | new observer crate |
| `ekos/plugins/clickhouse/src/{lib.rs,catalog.rs}` | catalog observer |
| `ekos/crates/pg-live/src/catalog.rs` | column lists, functiondef, stable names; test |
| `ekos/crates/recovery/src/{oracle_catalog_analyzer.rs,lib.rs}` | `CatalogDialect`; PostgreSQL/ClickHouse tests |
| `ekos/crates/kir/src/custom_kinds.rs` | two catalog kinds |
| `ekos/crates/cli/src/{app.rs,commands/{build,recover,database}.rs}`, `Cargo.toml`s | wiring, `--kind`, test per kind, CKM counts |
| `ekos/crates/semantic/src/{xref.rs,xref_builtins.rs}` | ClickHouse built-ins, dialect from the object |
| `web/api/app/{sources_write.py,routes/sources.py}`, `web/api/tests/test_sources.py` | kind, per-kind validation |
| `web/ui/src/pages/Sources{.tsx,.test.tsx}` | Kind selector, hints, kind chip |
| `README.md`, `docs/deployment.md`, `docs/generated/ekos-self-documentation.html`, `TODO.md`, `CLAUDE.md` | docs |
