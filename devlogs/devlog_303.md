# Devlog 303 — Step 6: silver from the source model, and the whole iDempiere pipeline in Dagster (RFC 0227)

**Date:** 2026-10-26
**PRs:** none (direct to `main`); ekos-lakehouse devlog_17
**Branch:** `main`

---

## Summary

Silver now reads what the source and EKOS know about each column. Oracle check constraints (`c IN ('Y','N')`) and the
application model's facts (boolean accessors, `AD_Ref_List` codes, logical foreign keys) reach the plan as column
facts. Silver turns `Y`/`N` flags into booleans, tests declared value lists strictly and code-declared lists and
references as warnings, flags retained purges, and closes deleted rows in history. `lakehouse run` launches the Dagster
job, and the whole iDempiere pipeline was run through Dagster for the first time.

The whole 849-table pipeline did not finish on this machine (memory), so a closed 165-table subset was run in one
Dagster process: every bronze table green after two fixes, 165 silver models built, 3,667 tests with 3,664 passing,
3 warnings that are real data outside the application's lists, and no errors. The run found six bugs, all fixed
(below). The full run is still open.

---

## RFC 0227 — silver from the source model, and the run

### Problem / motivation
iDempiere stores every flag as `CHAR(1)` `Y`/`N`, list columns as codes the application defines, and many foreign keys
only in code. Silver passed it all through untested. Retained purges were current in silver with nothing saying so,
snapshots never closed a deleted row, the Dagster template asked an Oracle source for a PostgreSQL WAL position, and no
command started a run.

### What was built

| Component | Change |
|---|---|
| `bronze` `ValueList`, `ColumnCheck`, `CodeFacts`; `ColumnFacts.check`/`.code`; `attach_checks`/`attach_code` | the new column facts (not written when empty: PostgreSQL locks unchanged) |
| `source::Source::value_lists`, `source_oracle::parse_in_list` | Oracle check constraints `c IN ('a', …)`, one column, literals only; a list a redaction would change is left out |
| `facts::ledger_facts` | write behaviour and `ModelTable` code facts in one ledger pass |
| `silver` | flags → boolean (declared / code), bronze-source flag test, `check-constraint` / `code-list` (warn) / `code-reference` (warn) tests, `_ekos_purged_at_source`, snapshot `hard_deletes: invalidate`, `flags-as-text` |
| `bronze::safe_name` + template `_safe` | Dagster/dbt-safe names for Oracle's `$`/`#` |
| `silver::quote_yaml11` | `<<` and `=` quoted for PyYAML |
| Dagster template | no WAL query on Oracle; dbt manifest prepared only when stale |
| `typemap_oracle` | `CHAR(n>1)` padded back with `rpad` |
| `sync::expression_changes` + `generate` | a reload requested for an incremental table whose column expression changed |
| `bronze::iceberg_name` + template `_iceberg_name` | no `$` in Iceberg table names (bronze, silver, snapshots, Pentaho refs) |
| `silver::typed_values_arg` | numeric value lists compared as numbers; YAML 1.1 booleans quoted |
| `plan_cmd::write` | the lock is always rewritten |
| `run_cmd.rs`, `lakehouse run [--in-process] [--wait]` | launch through GraphQL, follow to the end; console button |

### Decisions
- **Strict for the database, warn for the code.** A check constraint cannot be violated, so a failure there is a
  pipeline bug. The application's lists can be extended in its dictionary at run time, and a logical key in code is
  not enforced: a violation is a finding about the data, not a reason to stop the build.
- **A code-only flag is tested on bronze, not silver**: after the conversion an unexpected value is already NULL.
- **Declared facts win**: a column with a check constraint is never tested on the code's list; a column with a declared
  foreign key gets no code-reference test.

---

## Knowledge Captured

- **Dagster runs `definitions.py` in every step process.** Calling `DbtProject.preparer.prepare` unconditionally
  meant one `dbt parse` per step — 57 s each for 849 models. Prepare only when the manifest is older than a file.
- **`launchRun` on a job of ~1,700 steps takes minutes** (the queue coordinator launched it ~4 min after the request);
  a 60 s client timeout gave up while the run was still being created.
- **Dagster refuses `$` in names**; Oracle's `loadjava` creates `CREATE$JAVA$LOB$TABLE` and `JAVA$CLASS$MD5$TABLE` in
  the application schema.
- **`serde_yaml` emits `<<` and `=` bare**, and PyYAML (dbt) reads them as YAML 1.1 merge/value keys — "could not
  determine a constructor for the tag 'tag:yaml.org,2002:merge'". iDempiere's operator list has both.
- **A home project's orchestrator needs the source checkout**: `ekos` refuses a config whose `[workspace] root` is not
  a directory, so the Dagster container mounts it (read-only).
- **Trino drops `char(n)` padding on a cast to `varchar`**, and Oracle returns it (`'S  '`): reconciliation caught it
  on `AD_Role`. The load pads back with `rpad`; `CHAR(1)` keeps its plain cast, so the fix did not touch every flag.
- **An incremental load never rewrites an unchanged row**, so a fix to a load expression leaves old rows behind for
  good. `generate` now requests a reload for an incremental table whose column expression changed against the lock.
- **Trino reads `t$x` as a metadata table** (`t$files`, `t$snapshots`): a table named with `$` can be created but not
  found again, and the next `CREATE OR REPLACE` gets a new location that Polaris refuses ("not in the list of allowed
  locations"). Iceberg names drop `$` (`iceberg_name`).
- **PyYAML (YAML 1.1) reads a bare `ON`/`off`/`yes` as a boolean**; serde_yaml (1.2) leaves them bare. iDempiere's
  `DocSubTypeSO` has a code `ON`.
- **`accepted_values` on a numeric column** must not quote: Trino has no common type for `bigint` and `varchar(1)`.
- **dagster-dbt selects tests eagerly**: building a subset of models also ran 1,828 relationship tests whose child
  models were outside it (all errors: table missing). For a subset use `dbt test --indirect-selection cautious`.
- **The sandbox Trino (2 GiB) is OOM-killed by dbt's 4 threads** over a few thousand tests; 2 threads ran 3,667 tests
  cleanly. `dagster asset materialize -c` with `execution: config: in_process: {}` runs a selection in one process.
- Of iDempiere's 2,376 code-declared references, 2,367 already have a declared foreign key: on Oracle the code adds
  almost no relationships (7 tested). All 1,776 check lists are `Y`/`N` flags.
- On the testbed, two `ISACTIVE` columns carry two identical `Y`/`N` constraints each (1,778 constraints, 1,776 columns).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0227-silver-from-the-source-model.md` | new RFC with results |
| `README.md`, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | RFC 0227 |
