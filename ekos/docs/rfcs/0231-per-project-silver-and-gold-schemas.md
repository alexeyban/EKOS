# RFC 0231 — EKOS Lakehouse: each project's own silver and gold schemas

**Status:** Accepted
**Date:** 2026-10-26
**Repos:** ekos-lakehouse (`bronze`, `silver`, `gold`, `lakehouse-ext`); EKOS docs only
**Depends on:** RFC 0192 (bronze namespace prefix), RFC 0193 (silver), RFC 0195/0228 (gold, marts)

## 1. Problem

Found while recording the end-to-end video demo: a second lakehouse project on the iDempiere testbed (`idemsales`,
namespace prefix `bronze_sales_`) loaded its bronze into its own `bronze_sales_adempiere`, but its silver into
`silver_adempiere` — the schema of the first project (`idemdocs`, prefix `bronze_idem_`). Silver was always
`silver_<source schema>` and gold always `gold`, whatever the project's prefix: two projects on one catalog rebuild
each other's silver tables, and marts with the same name overwrite each other's gold.

## 2. Design

A project's **layer tag** is its namespace prefix without the default `bronze_` lead (`bronze_sales_` → `sales_`; a
prefix without that lead is its own tag, `raw_` → `raw_`). Silver is `silver_<tag><schema>`; gold is `gold` when the
tag is empty, else `gold_<tag without its trailing _>`.

| Namespace prefix | Bronze | Silver | Gold |
|---|---|---|---|
| `bronze_` (default) | `bronze_public` | `silver_public` | `gold` |
| `bronze_sales_` | `bronze_sales_adempiere` | `silver_sales_adempiere` | `gold_sales` |
| `raw_` | `raw_public` | `silver_raw_public` | `gold_raw` |

- `ekos_lakehouse_bronze::plan::{layer_tag, gold_schema}`; `silver::silver_schema(t)` takes the prefix from the
  table's namespace; `gold::compile` writes the gold schema into the model's config and keeps the tag for
  `engine_sql` (EXPLAIN, reconciliation); `lakehouse marts reconcile` reads the project's gold schema.
- **A project on the default prefix generates exactly what it did before** (Pagila unchanged).
- A project with a custom prefix gets new silver and gold schemas on its next `lakehouse generate`; the old tables are
  left where they are (an operator drops them when no longer read).

## 3. Not in this RFC

The Pentaho translation's fixed `silver_pentaho` / `gold` (a separate flow, RFC 0200–0207).

## 4. Results (2026-10-27)

Unit tests in all three crates (default unchanged; `bronze_sales_` tagged). On the testbed, `idemsales` regenerated
(only the 33 silver models' `schema=` line changed) and reloaded through Dagster: 36 steps succeeded, its 33 silver
tables in `silver_sales_adempiere`, its marts in `gold_sales`.
