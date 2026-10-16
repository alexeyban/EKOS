# RFC 0193 — EKOS Lakehouse M2: silver

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-16
**Builds on:** RFC 0190 (EKOS Lakehouse), RFC 0192 (bronze), RFC 0117 (dbt analyzer),
RFC 0157 (PII classification), RFC 0170 (business semantics)
**Code:** `ekos-lakehouse`, crate `silver` (pure) and `lakehouse-ext`

## Goal

One clean, conformed, tested silver table per bronze table, generated as a dbt-trino project
from facts EKOS already has — no model in the loop (RFC 0190: silver is derivable, so an LLM adds
risk and no information). Exit (RFC 0190): silver for Pagila (and LedgerSMB, once it has a live
sandbox source) passes every generated test, lint and the lineage check.

## Design

### 1. One model per bronze table

`models/silver/<schema>/<schema>__<table>.sql`, aliased to `<table>` in schema `silver_<schema>`
(the file name is unique for EKOS's dbt analyzer, which keys models by name). Materialized as a
table, rebuilt each run. Shape, chosen so EKOS's column lineage (`output_lineage`) follows every
pass-through column to bronze:

```sql
with bronze as (select * from {{ source('bronze_public', 'film') }}),
latest as (select *, row_number() over (partition by "film_id" order by "_ekos_loaded_at" desc) as "_ekos_rn" from bronze)
select "film_id", "title", … from latest where "_ekos_rn" = 1 and "_ekos_op" <> 'delete'
```

A full-mode bronze table needs no dedup (one snapshot per load): `select … from bronze`. A table
without a primary key keeps every row. Bronze's `_ekos_*` columns are not carried, except
`_ekos_loaded_at` as `_ekos_bronze_loaded_at` for freshness.

### 2. Masking, approved only

PII candidates come from RFC 0157's name classifier and are **reported, never applied**. Columns
a person lists in `[extensions.lakehouse.pii]` are masked: `method = "null"` keeps the column and
drops its values, `method = "hmac"` replaces each value with HMAC-SHA256 under a key from the
environment (`EKOS_LH_PII_KEY`; the key reaches dbt's compiled SQL under `target/`, which the
generated `.gitignore` excludes). A masked column is computed, so lineage reports it as masked,
not as missing.

### 3. Tests from facts

| Fact | dbt test |
|---|---|
| primary key | `unique` (single column) or `dbt_utils`-free combined-key test, and `not_null` per key column |
| `NOT NULL` column | `not_null` |
| declared foreign key to a planned table | `relationships` to that table's silver model |
| enum | `accepted_values` with the enum's labels |

Every test carries `meta: {ekos_rule, evidence}` naming the catalog fact it came from. Inferred
keys (RFC 0158) and RFC 0158's DQ rules are M2b.

### 4. History, declared only

Tables listed in `history` get a dbt snapshot (SCD2): `strategy: timestamp` on the bronze
watermark when it is proven (RFC 0192 §3), else `strategy: check` on all columns.

### 5. Docs

Table and column descriptions from the source's `COMMENT ON` text, redacted (RFC 0043) before
it is written anywhere. Confirmed RFC 0170 concepts are M2b.

### 6. The project compiled back, and code quality measured

`lakehouse check` compiles the generated dbt project with EKOS's own dbt analyzer and reports:

| Metric | How |
|---|---|
| lineage completeness | every silver column resolves to a bronze column, or is a listed computation (mask, cast) |
| test coverage | share of models with key tests, `not_null` tests, relationship tests where keys exist |
| documentation coverage | share of models and columns with a description |
| drift from generated | files whose content differs from what `generate` would write now |
| lint | `sqlfluff` (dialect trino, jinja templater, generated `.sqlfluff`), zero errors |

Each is recorded as a `LakehouseMetric` with how it was measured.

### 7. Orchestration

The generated Dagster project gains the dbt project as assets (`dagster-dbt`), each silver model
depending on its bronze asset; dbt tests run with the models and fail the run.

## Found while building it (2026-10-16)

Decisions forced by Pagila and LedgerSMB, now part of the design:

- **`±infinity` timestamps are misread by Trino's connector** — LedgerSMB's `tax.validto =
  'infinity'` came back as year -171979, silently, and loaded. Plan time counts ±infinity in
  timestamp and date columns (tables up to `full-refresh-max-rows`) and refuses the column (so
  the table) with the count; reconciliation catches what plan time did not measure.
- **A value one side cannot render canonically is a divergence**, reported by tier, not an
  error that stops `reconcile` (it first surfaced as "cannot parse '' as u32"). The planted
  control is skipped once a tier has diverged: that run already proved the comparison can fail.
- **A source can declare the same foreign key twice** (LedgerSMB): one `relationships` test per
  (column, target, target column), naming every constraint; dbt refuses two tests of one name.
- **Generated files are tracked by a manifest** (`.generated.json`, path → SHA-256 of what was
  written). A file whose content no longer matches was edited by a person: `generate` keeps it
  and reports it, `check` lists it as drift. A file the plan no longer produces is removed
  unless edited; a stale model kept feeding dbt a source that no longer existed. A file that
  was never generated here is refused.
- **Snapshots are YAML** (dbt ≥ 1.9 `snapshots:` with `relation:`): the legacy
  `{% snapshot %}` block is not parseable by sqlfluff's jinja templater.
- **The masking key is a project var with no default**, written only when an `hmac` mask
  exists: an unset key stops dbt instead of hashing with an empty key.

## Out of scope

Gold (M4), schema changes (M3), type fixes beyond bronze's mapping (need approved
dispositions), inferred keys and RFC 0158 rules (M2b).
