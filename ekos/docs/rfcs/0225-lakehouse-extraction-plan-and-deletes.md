# RFC 0225 — EKOS Lakehouse: the extraction plan from write behaviour, and deletes without a marker

**Status:** Accepted
**Date:** 2026-10-25
**Plan:** Oracle/iDempiere plan v3, Part II.5 (step 5) and Part III; the lakehouse half of RFC 0212
**Repos:** ekos-lakehouse (`bronze`, `lakehouse-ext`); EKOS docs only
**Depends on:** RFC 0192 (bronze load modes), RFC 0155/0156 (buckets), RFC 0212 (`WriteBehaviour`), RFC 0216
(source engines)

## 1. Problem

On an Oracle source every table loads in full (RFC 0216): a watermark is accepted only when a database trigger
proves it, and iDempiere has none — its `Updated` column is set by the application (`PO.saveUpdate`). EKOS already
knows, per table, whether the application keeps `Updated` (RFC 0212's update class) and how rows are deleted (delete
classes), but the lakehouse does not read it.

Once a table loads incrementally, a hard delete leaves no trace a watermark can see. Today's answer is a full
key-set diff through Trino on every run (RFC 0192 §3), which reads every source key every time and applies whatever
it finds: a lost permission, a truncated table or a wrong database would tombstone the whole table.

## 2. Design

### 2.1 Inputs

`lakehouse plan` reads the project's current `WriteBehaviour` objects (`table`, `owner`, `update_class`,
`delete_classes`, `change_log`) from the EKOS ledger and passes them to `bronze::plan_with` keyed by
`OWNER.TABLE`. Without a ledger or without facts for a table, the table is planned as before.

### 2.2 Application watermark (plan D4)

New setting `application-watermark = "UPDATED"`: the column an application maintains on every write it makes. A table
is incremental on it when all hold:

| Condition | Evidence recorded |
|---|---|
| the column exists and can order writes | its type |
| `update_class` is `po_only` or `raw_sql_sets_updated` | the class, from `WriteBehaviour` |
| the table has a primary key | the key, for delete detection |
| `row_estimate ≥ incremental-min-rows` (default 0) | the estimate |

`raw_sql_skips_updated` keeps the table in full, with the reason. The evidence names its limit: *an update made
outside the application (SQL console, a DBA script) does not set the column*; such a change is missed by the load
and shown by the next reconciliation (V3 compares whole rows). A trigger-proven watermark (RFC 0192) still wins.

### 2.3 The delete plan (plan III.2)

Every incremental table chosen from facts gets a `deletes` plan in its decision (absent otherwise, so existing lock
files keep their shape):

| Delete classes | Cadence | What a found delete is |
|---|---|---|
| any of `deleted_by_app`, `deleted_by_users`, `deleted_by_cascade` | every run | `delete` |
| `purged` and none of the above | every run | `purge`, policy `retain` (default) or `propagate` |
| `soft_delete_only`, `never_deleted_in_code` | audit, every `delete-audit-days` (default 7) | `delete`, flagged **contradicts the code** |

Method: **bucketed key diff** (plan III.1 method 3). `deletes.sql` (the full key-set diff) is not generated for these
tables; the orchestrator runs `lakehouse deletes --table T` after the load instead.

### 2.4 `lakehouse deletes --table T [--run-id R] [--accept]`

1. Not due (audit cadence, last check younger than the interval): prints so and stops. Last checks are kept in
   `<state>/lakehouse/deletes.json` (time only).
2. Hash every key into 256 buckets on both sides — the source read directly in its dialect, bronze's current state
   through Trino — and compare per-bucket counts and key-hash sums (RFC 0156's V3 machinery over the key alone).
3. Read keys only in the buckets that differ, on both sides; a key bronze holds and the source lacks is a delete.
4. **Mass-delete guard** (plan III.4, D9): more than `mass-delete-percent` (default 5) of bronze's keys, or more than
   `mass-delete-rows` (default 10,000) — nothing is applied; the table is `deletes_pending`, the command fails with
   the counts, and the operator runs it again with `--accept` once they know why.
5. Otherwise one tombstone per key: bronze's last version of the row, its columns cleared except the key,
   `_ekos_op = 'delete'` or `'purge'`. Bronze's current state excludes both, so reconciliation still compares bronze
   with the source exactly; with `retain`, the rows stay in bronze's history and silver (step 6) keeps them flagged.
6. A fact per check — a `LakehouseMetric` with `metric: deletes` (no new kind, so no EKOS registry row): method, buckets compared and differing, keys read on each side, deletes
   found and applied, kind, guard, contradiction with the code, the interval `[last_seen, detected]` — the true
   delete time is unknown without CDC and is never invented. **No key value is recorded.**

A key that reappears is loaded as a new version by the next incremental load. Deletes of tables loaded in full need
nothing: the replacement drops them.

### 2.5 Not in this RFC

The application change log (`AD_ChangeLog`, method 2: incomplete — a process without a session logs nothing), CDC
(RFC 0217), silver's handling of `retain` and the SCD2 close (step 6), surfacing a contradiction as an EKOS
`ConflictingEvidence` item in the review (it is in the fact and the command output), the console's Extraction page.

## 3. Phases

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | facts → plan: application watermark, delete plans, `plan --json`/summary | iDempiere plan: counts by mode and cadence equal the write-behaviour classes |
| 2 | `lakehouse deletes`, guard, tombstones, fact, orchestrator call | unit tests: the guard, bucket diff over recorded rows |
| 3 | live on the testbed | planted single delete, delete + re-insert, purge-table delete, mass delete trips the guard and `--accept` applies it; reconciliation green after each |

## 4. Tests

Pipeline location: a relative `pipeline-dir` of an EKOS home project whose workspace is a source tree resolves beside
the project's `ekos.toml` in the home — `generate` wrote `pipeline/` into the sources before (EKOS RFC 0178: a project
run leaves its sources untouched). The installer's projects (marker `.lakehouse/project.json`) keep theirs.

Plan: each update class and delete-class combination → mode, cadence, kind; no facts → unchanged decision; PostgreSQL
lock unchanged. Deletes: the guard's two thresholds; buckets that agree are never read key by key; tombstone SQL holds
no source value but the key. Live as in Phase 3, the source written by the schema owner on rows the test inserted.

## 5. Results

iDempiere testbed (`ADEMPIERE`, 903 tables; the `idemdocs` project's ledger holds RFC 0212's `WriteBehaviour` facts),
`application-watermark = "UPDATED"`, 2026-10-25.

**Phase 1 — the plan from facts.** `lakehouse plan` (release, 19 s with the ledger read):

| Decision | Tables | From the facts |
|---|---|---|
| incremental on `UPDATED`, deletes every run | 685 | |
| incremental on `UPDATED`, deletes audited every 7 days (a delete would contradict the code) | 54 | |
| **incremental, total** | **739** | 745 `po_only` + `raw_sql_sets_updated`, less 5 without a primary key and `C_CONVERSION_RATE` (refused, RFC 0216) |
| full: some code updates the table without setting `UPDATED` | 143 | 144 `raw_sql_skips_updated`, less `I_CONVERSION_RATE` (refused) |
| full: no primary key | 5 | |
| full: no `UPDATED` column | 14 | 14 `no_updated_column` |

No table came out as `purge`: all 12 purged tables are also deleted by users or the application, so a found key is a
`delete` there. `lakehouse generate` on the home project: 5,833 files, written under the project in the EKOS home;
the iDempiere checkout stayed clean (`git status` empty before and after). The Pagila pipeline's SQL, lock and silver
models are byte-identical; only the Dagster definitions gained the `lakehouse deletes` step.

**Phase 3 — deletes, live** (`oracle_live.rs`, `AD_MESSAGE`, 2,061 rows; rows planted by the schema owner above id
9,000,000 and removed afterwards):

| Scenario | Buckets differing (of 256) | Found | Result |
|---|---|---|---|
| single delete | 1 | 1 | tombstoned; reconciliation green, control fired |
| delete, then the same key inserted again | 1, then 0 | 1, then 0 | the key is current again as a new version; green |
| mass delete, 148 of 2,210 keys (6.7 %) | 114 | 148 | **guard tripped, nothing applied**; reconciliation diverged, as it must; accepted → applied → green |
| purge, policy `retain` (purged classes declared) | 1 | 1 | `_ekos_op = 'purge'`; bronze's current state excludes it, green; history keeps the row |

CLI (`lakehouse deletes --table ADEMPIERE.AD_MESSAGE`, release, after a load from the generated files): 1 delete
found and applied in 2.2 s, the next check found 0, `lakehouse reconcile` green; the two facts carry counts and the
`[last_seen, detected]` interval only.

**Not done:** the console's Extraction page (step 5's UI), silver's flag for retained purges and the SCD2 close
(step 6), a contradiction as an EKOS `ConflictingEvidence` review item, a cascade delete and a real purge routine
planted on the testbed (cascade and purge are classified, not yet exercised), and a Dagster run of the whole pipeline.

