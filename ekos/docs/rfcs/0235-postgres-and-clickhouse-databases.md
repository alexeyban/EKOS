# RFC 0235 — PostgreSQL and ClickHouse as project databases

**Status:** Accepted
**Date:** 2026-11-01
**Builds on:** RFC 0211 (live Oracle catalog), RFC 0223 (Sources tab), RFC 0157 (`ekos-pg-live`),
RFC 0056 (ClickHouse HTTP client)

## 1. Problem

`[[databases]]` has one kind, `oracle`. The console's Sources tab therefore offers only "Add an
Oracle database", although EKOS already has a guarded read-only PostgreSQL connector
(`ekos-pg-live`, used by EKOS Migrate) and a ClickHouse HTTP client. A project whose database is
PostgreSQL or ClickHouse cannot declare it; the only ClickHouse path is RFC 0056's environment
variables (`EKOS_CLICKHOUSE_URL`), one database per process, outside the project's config and
without `origin: live`.

## 2. Decision

`DatabaseKind` gains `postgres` and `clickhouse`. Each kind is read the way RFC 0211 reads Oracle:
once per build, read-only, outside the build fingerprint, a failure fatal; its objects carry
`origin: live`, `database`, `owner` and its `dialect`, and the newest read replaces earlier ones.

| | `connect` | `owners` | read through |
|---|---|---|---|
| `oracle` | `host:port/service` | schemas (upper-cased) | `ekos-ora-live` |
| `postgres` | `host[:port]/database` (5432) | schemas, as spelled | `ekos-pg-live` (`default_transaction_read_only`, timeouts) |
| `clickhouse` | `http[s]://host[:port]` (8123 / 8443) | databases | HTTP, every catalog query with `readonly=1` |

`profile` other than `p0` and `dictionary` stay Oracle-only; the config check refuses them for the
other kinds instead of ignoring them. *(Amended by RFC 0252, 2026-11-10: profiles are read for every
kind; only `dictionary` stays Oracle-only.)*

### 2.1 Observers

Both observers emit RFC 0211's artifact shape — one `catalog` artifact per owner (census, what was
listed, the write probe) and one `object` artifact per object, `{kind, owner, name, status, detail}`
— so one analyzer reads all three:

- **PostgreSQL** (`ekos-plugin-postgres`): `ekos_pg_live::introspect` + `reconcile` (a short catalog is
  an error), then a pure normalisation: columns folded into their table; `PRIMARY KEY` / `UNIQUE` /
  `FOREIGN KEY` column lists from `conkey`/`confkey` (added to the constraint query, additively), a
  check's expression from its definition; views and materialized views with `pg_get_viewdef`;
  routines with `pg_get_functiondef` (added to the routine query); triggers with
  `pg_get_triggerdef`. The census counts every catalog kind; the write probe is pg-live's
  `check_write_refused`.
- **ClickHouse** (`ekos-plugin-clickhouse`, module `catalog`): `system.tables` and `system.columns`
  for the listed databases. A column is `nullable` only when its type is `Nullable(…)`. The
  sorting and primary keys are table properties — a ClickHouse primary key is not unique, so it is
  never a `primary_key` constraint. Views and materialized views keep `as_select`. The write probe
  is a zero-row `INSERT … SELECT … WHERE 0` into the first table, sent without `readonly`: refused
  is what EKOS wants; accepted inserts nothing and is reported.

### 2.2 Analyzer

`oracle_catalog_analyzer` becomes dialect-parameterised (`CatalogDialect`): target scheme, id
prefix, `dialect` stamp, catalog summary kind, and the readers for views and routines. Oracle's
output, ids and pass name are unchanged. PostgreSQL views go through RFC 0169's reader with the
PostgreSQL dialect's rewrites, routines through RFC 0163's PL/pgSQL reader, triggers through the
trigger reader. ClickHouse views go through the view reader with the ClickHouse dialect; it has no
routines. New summary kinds `PostgresCatalog` and `ClickHouseCatalog`, keyed (database, owner).

### 2.3 CLI and console

`ekos database add --kind oracle|postgres|clickhouse` (default `oracle`, so existing scripts keep
working); `connect` is checked per kind; owners are upper-cased only for Oracle. `ekos database
test` prints the same JSON for every kind (`test.label`, `family`, `owners[].census`,
`write_check`), so the console's test panel needs no per-kind code. The Sources tab's form gets a
kind selector that changes the connect hint and the owners label and hides profile and dictionary
for the other kinds; the API validates `connect` per kind.

## 3. Not in scope

Profiles (`p1`/`p2`) for PostgreSQL and ClickHouse; MySQL, SQL Server, Snowflake (no live reader
yet — each needs its own read-only session guard first); replacing RFC 0056's environment-variable
ClickHouse observer, which keeps working.

## 4. Tests

Pure: normalisation of a recorded PostgreSQL and ClickHouse catalog; the analyzer per dialect
(tables, keys, FKs, views, routines, ids stable and dialect-scoped, Oracle ids unchanged); config
refusals; `database add` per kind; the console's argv builder per kind; the form's kind selector.
Live: `docker-compose.migrate.yml`'s PostgreSQL and ClickHouse, `ekos database test` and a build →
recover → commit with each.
