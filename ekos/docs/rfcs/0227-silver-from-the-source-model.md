# RFC 0227 — EKOS Lakehouse: silver from the source model, and the pipeline run (step 6)

**Status:** Accepted
**Date:** 2026-10-26
**Plan:** Oracle/iDempiere plan v3, Part II.6 (step 6), Part III.3 (deletes in silver)
**Repos:** ekos-lakehouse (`bronze`, `silver`, `lakehouse-ext`, the Dagster template); EKOS docs only
**Depends on:** RFC 0193 (silver), RFC 0209 (application model), RFC 0216 (Oracle source), RFC 0225/0226

## 1. Problem

Silver passes iDempiere through as Oracle stores it. Every flag is a `CHAR(1)` holding `Y` or `N` (1,881 columns are
declared that way by check constraints, 3,460 by the application dictionary); list columns such as `DocStatus` hold
codes the application defines; and many logical foreign keys exist only in the code (`I_C_Order.getC_BPartner()`), not
in the database. Silver tests none of it. Rows the source purges and the policy keeps (RFC 0225) are current in silver
with nothing saying so, and a snapshot never closes a deleted row. The generated Dagster code asks the source for a
PostgreSQL WAL position, and nothing runs the pipeline from the console.

## 2. Design

### 2.1 Facts into the plan

- **Declared**: the Oracle source reads check constraints of the form `column IN ('a', 'b', …)` (one column, literals
  only) into the column's facts as `check_values` with the constraint's name.
- **Code-declared**: `lakehouse plan` reads the ledger's `ModelTable` objects (RFC 0209) and attaches, per column,
  `code: {boolean, values, references}` — a `boolean` accessor, the list codes, the table a logical key points at.
  Matched by table and column name, case-insensitive; a column the model does not know has none.

Both are new optional fields in `ColumnFacts` (not written when empty: PostgreSQL lock files are unchanged), so the
plan digest (RFC 0226) covers them.

### 2.2 Silver

| Fact | Silver does | Test | Severity |
|---|---|---|---|
| flag: `check_values` = {Y, N}, or `code.boolean` on a one-character column | `case c when 'Y' then true when 'N' then false end`, type `boolean` | declared: none needed (the database enforces it); code only: `accepted_values [Y, N]` on the **bronze source** column, so no other value can turn into a silent NULL | error |
| declared list (`check_values`, not a flag) | — | `accepted_values` | error |
| code list (`code.values`) | — | `accepted_values` on the codes | **warn**: the dictionary can add values at run time |
| declared foreign key | — | `relationships` (as today) | error |
| code reference, no declared key on that column, target planned with a one-column key | — | `relationships` to the target's key | **warn**, labelled code-declared |
| table that retains purges | its current rows stay; new column `_ekos_purged_at_source`: when the purge was detected, NULL otherwise | — | — |

Every test carries `meta.ekos.rule` and evidence, as today: `check-constraint`, `code-flag`, `code-list`,
`code-reference`. Flags are converted unless `flags-as-text = true`. A masked column is never converted or tested.

### 2.3 History

A snapshot (`history`) gets `hard_deletes: invalidate` (dbt ≥ 1.9): a row deleted at the source, absent from silver,
is closed with `dbt_valid_to` = the snapshot run that first missed it. That time is the detection, not the delete
(unknown without CDC); the snapshot's `meta.ekos.delete_time` says `estimated`. Retained purges stay current, so they
are not closed.

### 2.4 Running it

- The Dagster template asks an Oracle source for nothing PostgreSQL-specific: the source position is recorded as unknown
  unless the engine is PostgreSQL.
- `lakehouse run [--job bronze] [--wait]` (class *external*): launches the job through Dagster's GraphQL API at
  `dagster-url`, prints the run id and its page, and with `--wait` follows it to the end (exit 1 unless it succeeded).
  The Lakehouse page gains **Run the pipeline**.

- `lakehouse run --in-process` runs every step in the run's own process, one after another: on a pipeline of hundreds
  of tables a process per step (each importing the definitions) costs more than the steps.
- The template prepares the dbt manifest only when it is missing or older than a project file; it used to run
  `dbt parse` on every import, that is, once per step process (57 s each for iDempiere's 849 models).

### 2.5 Names and YAML

Oracle allows `$` and `#` in names (`CREATE$JAVA$LOB$TABLE`, from `loadjava`); Dagster and dbt do not. A model and
asset name has each such character replaced by `_` and, when it changed, the FNV-1a hash of the original appended
(`safe_name`, mirrored by the template's `_safe`; a test pins one value both compute). `serde_yaml` writes the
scalars `<<` and `=` bare, which PyYAML (dbt) reads as YAML 1.1's merge and value keys: an iDempiere code list holds
both, so the emitter quotes them.

### 2.6 Not in this RFC

Business names and descriptions from the application model in silver's docs (licence rules, RFC 0214), gold (step 7),
the console showing dbt test results per table (they are already facts, RFC 0196).

## 3. Phases

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | facts in the plan | iDempiere: check lists and code facts counted against the database and the ledger |
| 2 | silver, snapshots, Dagster template, `lakehouse run` | unit tests per row of §2.2; Pagila silver unchanged except snapshots |
| 3 | a Dagster run of the whole iDempiere pipeline | every bronze table loaded and reconciled; silver built; test failures counted and explained |

## 4. Results (2026-10-26, iDempiere testbed)

**Phase 1.** 1,776 check lists on planned columns, against the database's 1,778 constraints (two `ISACTIVE` columns
carry two identical constraints each); every one is a `Y`/`N` flag. Code facts: 1,944 boolean columns (1,494 also
declared, 450 code only), 548 code lists, 2,376 code references, of which 2,367 have a declared foreign key, 7 are
tested, 1 points at an unplanned table and 1 at a table with a two-column key.

**Phase 2.** Unit tests for each row of §2.2; Pagila's generated pipeline is byte-identical except the Dagster template.

**Phase 3 — partly met.** The whole pipeline (849 tables) did not finish on this 15 GB machine: a first run
(`dagster dev`, a process per step) loaded 219 tables before memory ran out. What was run instead: a closed subset —
`C_ORDER`, `C_ORDERLINE`, `AD_ROLE`, `AD_MESSAGE` and `CREATE$JAVA$LOB$TABLE` with every table their declared or
code-declared references reach (165 tables) — in one Dagster process (`in_process` executor).

| Pass | Result |
|---|---|
| 1: bronze, 165 tables | 163 green; `AD_ROLE` diverged, `CREATE$JAVA$LOB$TABLE` failed to commit (both below); silver skipped |
| 2: the 16 reloaded tables + silver | 16/16 green; 165/165 silver models built; the sandbox Trino (2 GiB) was OOM-killed under dbt's 4 threads running 5,455 eagerly selected tests |
| 3: the subset's tests (`--indirect-selection cautious`, 2 threads) | 3,667 tests: 3,664 pass, 3 warn, 0 error |

Pass 3 by rule: not-null 2,393, foreign key 715, code flag (bronze) 205, primary key 164, code list 190 (187 pass,
3 warn). The three warnings are data, not pipeline faults: `AD_Workflow.PublishStatus = 'D'` and
`C_Payment.TrxType = 'X'` are outside the application's lists, and one `C_DocType.DocBaseType` is three spaces.

Found and fixed by the run:

- **`CHAR(n)` padding.** Trino's cast from `char(n)` to `varchar` drops trailing spaces; Oracle returns them
  (`AD_Role.UserLevel = 'S  '`). The load pads back to *n* (`rpad`); `CHAR(1)` keeps its plain cast (no padding to
  lose), so the fix changed only the 66 tables with a wider `CHAR`.
- **A changed load expression needs a reload.** An incremental load never rewrites an unchanged row, so a table
  loaded before the fix stayed diverged. `generate` now compares each column's expression with the deployed lock and
  requests a reload (RFC 0194's marker) for an incremental table whose expression changed: 45 tables here.
- **`$` in an Iceberg table name.** Trino reads `t$x` as a metadata table of `t`, so `CREATE$JAVA$LOB$TABLE` was
  created but never found again, and the next full load's `CREATE OR REPLACE` failed in Polaris ("not in the list of
  allowed locations"). Bronze and silver tables with a `$` take the safe name (`CREATE_JAVA_LOB_TABLE_5f05e67e`).
- **YAML 1.1 booleans.** iDempiere's `DocSubTypeSO` has a code `ON`; PyYAML read it as `true`. `yes`/`no`/`on`/`off`
  are quoted like `<<` and `=`.
- **Numeric code lists.** `AD_Color.StartPoint` is a number with a code list `1`…`8`; Trino has no common type for
  `bigint` and `varchar`, so the list is compared as numbers (`quote: false`), and a list with a non-numeric code is
  not tested on a numeric column.
- **The lock is always rewritten.** `generate` kept a lock edited by hand like any other file, so it could describe
  another pipeline than the SQL beside it.

Not done: the full 849-table run (it needs a larger Trino than the sandbox's 2 GiB and more free memory). 30
incremental tables outside the subset still have a reload requested.
