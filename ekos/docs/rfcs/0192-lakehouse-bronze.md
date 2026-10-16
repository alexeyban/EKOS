# RFC 0192 — EKOS Lakehouse M1: bronze

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-16
**Builds on:** RFC 0190 (EKOS Lakehouse), RFC 0155/0156 (canonical form, tiers), RFC 0157 (live
PostgreSQL), RFC 0160 (statement classifier)
**Code:** `ekos-lakehouse` repository (extension), crates `bronze` (pure) and `lakehouse-ext`

## Goal

A PostgreSQL source lands in Iceberg bronze tables, faithfully and on a schedule, with every
table reconciled against the source on every run and planted controls proving the
reconciliation can fail. Exit (RFC 0190): Pagila does this on the sandbox.

## What was measured first (sandbox, 2026-10-16)

On Pagila (PostgreSQL 18.6, Trino 483), comparing PostgreSQL's catalog with what Trino's
`postgresql` connector exposes:

- **With its default settings the connector silently drops columns** of a type it does not
  map: a domain (`year` over `integer`), `text[]` and `tsvector` — 3 of 94 columns, no error,
  no warning. A bronze generated from the connector's view would lose data quietly.
- With `postgresql.array-mapping=AS_ARRAY` and `unsupported-type-handling=CONVERT_TO_VARCHAR`
  every column appears: arrays as arrays, the rest as `varchar` (a domain too, its base type lost).
- The connector lists views and every partition of a partitioned table as tables (Pagila: 55
  `payment_p…` partitions beside `payment`).
- Iceberg rejects `smallint` and the type an unconstrained `numeric` becomes (`number`).

## Design

### 1. The source of truth is PostgreSQL's catalog, checked against the connector

The table list and every column's type come from PostgreSQL (the RFC 0157 catalog read, plus
each column's type kind and a domain's base type). Base tables and partitioned parents only;
partitions, views and materialized views are not bronze tables. Then the generator reads the
connector's column list for each table and **refuses the table** when a source column is missing
from it, naming the column and the two settings above. The source catalog in Trino must have
both settings; `lakehouse doctor` checks it.

### 2. Type map (PostgreSQL type, connector type) → Iceberg type, with a lossiness class

| Class | Meaning | Examples |
|---|---|---|
| `exact` | same values, same type family | boolean, integer, bigint, real, double, decimal(p≤38,s), text/varchar/enum → varchar, bytea → varbinary, uuid, date, time → time(6), timestamp → timestamp(6), timestamptz → timestamp(6) with time zone, `vector(n)` → array(real), arrays of exact element types; a domain cast back to its base type |
| `widening` | every value fits a wider type | smallint → integer |
| `as_text` | the PostgreSQL text rendering, kept exactly; the type's semantics are not | tsvector, interval, inet/cidr, json/jsonb, other connector-converted types |
| `narrowing_safe` | an unconstrained `numeric` given `decimal(38,s)` because the profile measured precision and scale that fit | needs P1 profile evidence; recorded with it |
| `refused` | no faithful landing | unconstrained `numeric` without evidence, numeric(p>38), a type the connector cannot read |

Each mapping records its rule id, the Trino `SELECT` expression (`CAST` where the type changes)
and the evidence. A `refused` column refuses its table: bronze never drops a column.

### 3. Load mode per table, chosen from facts and recorded with them

- **`full`** (default): `CREATE OR REPLACE TABLE … AS SELECT` — one atomic commit, a new Iceberg
  snapshot, history kept.
- **`incremental`** by a watermark column, only when **both**: the column is declared in
  `[extensions.lakehouse] watermarks` **and** the source proves it moves on every write — a
  `BEFORE UPDATE` row trigger whose function assigns `NEW.<column>` (Pagila's `last_updated`
  triggers do). The P1 `monotonic` sample is reported, never sufficient: it only reads rows in
  physical order. Loads `>=` the highest watermark already in bronze (ties are re-read, never
  skipped; silver dedups). Tables with a primary key also get a **key-set diff** each run that
  appends a tombstone for every key bronze has and the source no longer does.

A table larger than `full-refresh-max-rows` (default 5 000 000) with no proven watermark stays
`full`, and the plan says why.

### 4. Bronze shape

`iceberg.<prefix><source schema>.<table>` (prefix `bronze_`), the mapped source columns in source
order, then `_ekos_loaded_at timestamp(6) with time zone`, `_ekos_run_id varchar`,
`_ekos_source_lsn varchar`, `_ekos_op varchar` (`load` | `upsert` | `delete`). Append-only per
run; nothing is cleaned. Maintenance per table, scheduled: `expire_snapshots`
(retention 7 d), `optimize`, `remove_orphan_files`.

### 5. Generated, deterministic, in the pipeline repository

`lakehouse plan` prints the decisions; `lakehouse generate --out <dir>` writes:

```
lakehouse.lock.json             the plan: tables, mappings, modes, evidence (M3's "deployed snapshot")
bronze/<schema>/<table>/load.sql         template: {{run_id}}, {{lsn}}, {{watermark}}
bronze/<schema>/<table>/deletes.sql      incremental tables with a key
bronze/<schema>/<table>/maintenance.sql
dagster/                        assets per table + maintenance schedule (step 6)
```

Byte-identical output for the same inputs (golden tests). EKOS still moves no rows: the
orchestrator runs these files against Trino.

### 6. Reconciliation and controls on every run

After a load, `lakehouse reconcile <table>` runs V1–V3 with PostgreSQL read directly and bronze
read through the read-only `TrinoReader`. For an incremental table the target is bronze's
current view (latest version per key, tombstones removed). Columns with no canonical rule
(`as_text`, arrays) are listed as not compared, never silently skipped. **A planted control runs
with every reconciliation**: V3 repeated with one row's target value perturbed in the query; if
V3 does not flag it, the run is `unverified`, not green.

### 7. Facts

Plans, run reports and reconciliation results are `Lakehouse*` objects (RFC 0190 registry rows)
appended to the project's ledger with their evidence — never a row value.

## Out of scope

CDC (v2), schema-change handling (M3), silver (M2), production deployment gates (M5).

## Tests

Pure: type map table, mode choice (each evidence combination), renderers (golden). Live
(`EKOS_LAKEHOUSE_LIVE=1`): every mapping round-trips through Trino into Iceberg and reconciles
at V3; Pagila end to end: plan, generate, load, reconcile, control fires; a column the connector
drops refuses its table.
