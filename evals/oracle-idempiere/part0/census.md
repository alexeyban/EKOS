# Part 0 — Oracle testbed census (RFC 0211)

**Measured:** 2026-10-23, `census.sql` as `SYSTEM` (raw output: `census.out`)
**Database:** Oracle Database 21c Express Edition 21.3.0.0.0, `gvenzl/oracle-xe:21.3.0-slim`, AL32UTF8
**Seed:** `idempiereServer12.gtk.linux.x86_64.zip` (SHA-256 `e58d527f…aee68f`, MD5 as published by
SourceForge), `data/seed/Adempiere.jar` → `Adempiere.dmp`, imported by
`deploy/fixtures/idempiere-oracle.sh` in 3 min 5 s. Schema `ADEMPIERE`.

## Findings

**BLOCKER:** none.

**Import warnings (3, from impdp's own count):**

| Error | Object | Meaning |
|---|---|---|
| `ORA-31684` | user `ADEMPIERE` already exists | expected: the release's `CreateUser.sql` creates it before the import |
| `ORA-31693` + `ORA-30094` | table data `TEST`, `AD_SCHEDULER` | rows not loaded: time-zone file version 26 is not in the slim image. The tables exist; only their rows are missing. Matters for profiling (RFC 0211 Phase 4), not for the catalog. The full `gvenzl/oracle-xe:21.3.0` image is the fix to try then |

`AfterImport.sql` (`DBA_Cleanup`, `DBA_AfterImport`, recompile) ran clean: **0 invalid objects**.

## Census

| Object type | Valid | Invalid |
|---|---|---|
| TABLE | 903 | 0 |
| VIEW | 160 | 0 |
| INDEX | 2,109 | 0 |
| FUNCTION | 48 | 0 |
| PROCEDURE | 14 | 0 |
| TYPE | 1 | 0 |
| LOB | 45 | 0 |

No triggers, packages, sequences or synonyms in the schema.

| What | Count |
|---|---|
| table columns | 18,520 |
| view columns | 11,937 |
| primary keys (`P`) | 892 |
| foreign keys (`R`) | 3,686 |
| unique constraints (`U`) | 908 |
| check constraints (`C`) | 12,958 |
| views longer than 4,000 characters | 40 |
| recycle bin | 0 |
| rows, by optimizer statistics | 182,848 |

## The connector against the census (`ekos-ora-live`, live suite)

As `ekos_ro` (`CREATE SESSION`, `SELECT` on the 1,063 tables and views, `SELECT_CATALOG_ROLE`):

| What | Census | `introspect` (DBA_* views) |
|---|---|---|
| tables / views / indexes | 903 / 160 / 2,109 | 903 / 160 / 2,109 |
| functions / procedures / types | 48 / 14 / 1 | 48 / 14 / 1 |
| columns (table + view) | 30,457 | 30,457 |
| check constraints | 12,958 | 1,892 listed + 11,066 system-named `NOT NULL` folded into nullability |
| primary / foreign / unique | 892 / 3,686 / 908 | 892 / 3,686 / 908 |
| views with truncated text | — | 0 (the `LONG` is read in full; longest 49,346 characters) |

Introspection took 1.5 s. Write probe: `ekos_ro` refused with `ORA-01031`; the schema owner
accepted (zero rows, rolled back).

**The `ALL_*` trap, measured:** read through `ALL_*` views, the same user sees **0 of the 48
functions** — `ALL_OBJECTS` and `ALL_SOURCE` both hide routines the user may not execute, so the
list and the count shrink together and reconcile with each other. Only the `DBA_*` family gives the
real denominator.
