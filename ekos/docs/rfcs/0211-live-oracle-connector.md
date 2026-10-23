# RFC 0211 — Live Oracle Connector, Profiling, Code ↔ Database Conflicts

**Status:** Accepted
**Date:** 2026-10-23
**Depends on:** RFC 0043 (redaction at every raw-content entry point), RFC 0157 (`pg-live`: guarded
sessions, catalog reconciliation, profile tiers), RFC 0172 (`ConflictingEvidence`), RFC 0209
(`ModelTable`), RFC 0210 (Oracle dialect, PL/SQL IR, `SchemaChange`)
**Plan:** `oracle-idempiere-plan.md` v3, Part 0 and Part I.6
**Testbed:** iDempiere 12 seed on Oracle XE 21c (`docker-compose.oracle.yml`,
`deploy/fixtures/idempiere-oracle.sh`), source tree `release-13` @ `3355eb5fc9`

---

## 1. Motivation

RFCs 0209 and 0210 read everything iDempiere *says* about its database: generated model classes,
migration scripts, PL/SQL files. None of it is the database. The base schema ships as a Data Pump
dump, so 17,171 of the 31,620 recorded schema changes name tables that no `CREATE TABLE` in the
tree defines (devlog_287), and nothing can say whether a `ModelTable` column, a migration's
`ADD COLUMN` or a `db/oracle` function still matches what is installed.

The catalog is the authority. This RFC reads it — safely, completely, without a row value — and
compares it with the code's claims.

## 2. Goals and non-goals

**Goals**

1. A testbed anyone can rebuild: Oracle XE in Compose, the iDempiere seed imported by iDempiere's
   own script, a read-only user, and a committed **census** (objects by type and status, columns,
   constraints) that every coverage number is a fraction of.
2. `crates/ora-live`: guarded read-only sessions, catalog introspection from `ALL_*` views
   reconciled against `ALL_OBJECTS`, write probe, profiles, archive-log and supplemental-logging
   state as facts.
3. The catalog in the ledger at `ekos build` (an `Observer`) and `ekos recover` (an analyzer pass),
   as ordinary `Table` / `View` / `Procedure` / `Trigger` objects with `dialect: oracle` and
   `origin: live`.
4. Code ↔ database conflicts as RFC 0172 `ConflictingEvidence`.

**Non-goals**

- Writing to Oracle, ever. There is no sandbox session here (RFC 0164's `PgSandbox` has no Oracle
  twin in this RFC).
- Reading `AD_*` dictionary rows (plan decision D3: generated code is the dictionary's restatement;
  rows are an opt-in cross-check later).
- Oracle → ClickHouse / Iceberg migration (RFC 0216).
- CDC (RFC 0217). This RFC only records whether its prerequisites hold.

## 3. Design

### 3.1 Testbed (Part 0)

| Piece | What |
|---|---|
| `docker-compose.oracle.yml` | `gvenzl/oracle-xe:21.3.0-slim`, loopback port 51521, `TZ=UTC`, a named volume |
| `deploy/fixtures/idempiere-oracle.sh` | downloads `idempiereServer12.gtk.linux.x86_64.zip`, checks it against a pinned SHA-256, runs the four steps of the release's own `utils/oracle/ImportIdempiere.sh` inside the container with that release's SQL files (`CreateUser`, `CreateDataPumpDir`, `impdp`, `AfterImport`), then creates `ekos_ro`. The v12 script itself cannot be used: its `ORACLE_DOCKER_CONTAINER` support arrived in release 13 |
| `ekos_ro` | `CREATE SESSION`, `SELECT` on every table and view of the `ADEMPIERE` schema, `SELECT_CATALOG_ROLE`. Nothing else |
| census | `evals/oracle-idempiere/part0/census.md` — produced by a SQL script as `SYSTEM`, committed as measured |

The seed is release 12, one behind the source tree, so `migration/iD13` is the replay set: changes
dated after the seed's last applied migration are *pending*, not *missing* (§3.6).

### 3.2 Sessions (`ora-live::session`)

The driver is `oracle` (rust-oracle, ODPI-C) behind the crate feature **`oracle`**, off in the
default build (decision D1). ODPI-C compiles with a C compiler; Oracle Instant Client is loaded at
run time, so a machine without it builds and tests everything except the live suite. Without the
feature, `OraSource::connect` returns `OraError::DriverNotBuilt`.

Guards, applied before the first query, each a policy field:

| Guard | How |
|---|---|
| read only | `SET TRANSACTION READ ONLY` at the start of every introspection transaction |
| call timeout | ODPI-C call timeout (`Connection::set_call_timeout`), default 30 s |
| NLS pinned | `ALTER SESSION SET NLS_DATE_FORMAT`, `NLS_TIMESTAMP_FORMAT`, `NLS_TIMESTAMP_TZ_FORMAT`, `NLS_NUMERIC_CHARACTERS='.,'`, `NLS_SORT=BINARY`, `NLS_COMP=BINARY` |
| time zone | `ALTER SESSION SET TIME_ZONE='UTC'` |
| attribution | `module = "ekos"`, `action = <run id>` (visible in `V$SESSION`) |

**Write probe.** `INSERT INTO <owner>.<table> SELECT * FROM <owner>.<table> WHERE 1 = 0`, then
`ROLLBACK`. The privilege check happens at parse, so a read-only user gets `ORA-01031` and a user
who can write inserts zero rows — nothing to undo either way. A DDL probe is not used: Oracle DDL
commits implicitly. The session's `SESSION_PRIVS` are also recorded, so an `… ANY TABLE` privilege
is a finding even when the probed table refuses.

### 3.3 Catalog (`ora-live::catalog`)

**View family.** The catalog reads `DBA_*` views (filtered by owner) whenever the session can —
`SELECT_CATALOG_ROLE` grants them — and `ALL_*` otherwise, and the snapshot records which. `ALL_*`
is not a safe default for a read-only user: `ALL_OBJECTS` and `ALL_SOURCE` both hide routines the
user may not execute, so list and count shrink together and still reconcile (measured: 0 of 48
functions visible). Granting `EXECUTE` instead would let the user run routines that write.

All reads go through a `CatalogSource` trait (`fn rows(&self, sql, binds) -> Vec<Vec<Option<String>>>`),
implemented by the live session and by recorded fixtures in tests, so the mapping is tested without
a database.

| Kind | View | Notes |
|---|---|---|
| table | `*_TABLES` | `IOT_TYPE`, `TEMPORARY`, `PARTITIONED` kept; recycle-bin (`DROPPED='YES'`) excluded and counted |
| column | `*_TAB_COLUMNS` | `DATA_DEFAULT` read as the `LONG` it is (`DATA_DEFAULT_VC` exists only from 23ai), `CHAR_USED` for `BYTE`/`CHAR` semantics |
| constraint | `*_CONSTRAINTS`, `*_CONS_COLUMNS` | `SEARCH_CONDITION`; system-named `"C" IS NOT NULL` checks folded into column nullability and counted, not listed |
| index | `*_INDEXES`, `*_IND_COLUMNS` | |
| view | `*_VIEWS` | `TEXT`, the `LONG`, read in full by the driver (`TEXT_VC` would cut 40 of iDempiere's 160 views at 4,000 characters); a read shorter than `TEXT_LENGTH` is marked `text_truncated` |
| sequence | `*_SEQUENCES` | |
| routine, package, trigger, type | `*_SOURCE` | text joined by line, ordered by `LINE`; trigger binding from `*_TRIGGERS` |
| synonym | `*_SYNONYMS` | |
| comment | `*_TAB_COMMENTS`, `*_COL_COMMENTS` | |
| status | `*_OBJECTS` | `INVALID` objects become facts, not omissions |

**Reconciliation.** Every kind's count is compared with `*_OBJECTS` grouped by `OBJECT_TYPE`
(tables, views, sequences, indexes, procedures, functions, packages, package bodies, triggers,
types, synonyms); a mismatch is `OraError::CatalogIncomplete`, as in RFC 0157. An `OBJECT_TYPE`
the module does not know is an error too (`UnknownObjectType`), never a skipped row; storage and
out-of-scope types (LOB segments, partitions, Java, jobs) are counted in the census, not listed.

**Redaction.** Source text, view text, defaults and check conditions pass through
`ekos_common::redaction::redact` before they leave the crate — a raw-content entry point
alongside `pg-live`, as RFC 0043 requires.

### 3.4 Profiles (`ora-live::profile`)

| Tier | Source | Cost |
|---|---|---|
| P0 | `ALL_TABLES.NUM_ROWS`, `LAST_ANALYZED`, `ALL_TAB_COL_STATISTICS.NUM_DISTINCT`/`NUM_NULLS`/`AVG_COL_LEN` | dictionary only |
| P1 | `SELECT COUNT(*)` per table, under the call timeout | one scan per table |
| P2 | per-column `MIN`/`MAX` of numeric and date columns, `MAX(LENGTH)` of character columns | one scan per table |

`LOW_VALUE`/`HIGH_VALUE` are never selected: they are raw row values. P2 bounds go through the same
"suppressed for PII-class columns" rule as `pg-live`, and never reach the ledger for a column
classed as personal data.

**Log state facts** (for Part IV): `V$DATABASE.LOG_MODE`, `SUPPLEMENTAL_LOG_DATA_MIN`/`_PK`/`_ALL`,
and per-table `ALL_LOG_GROUPS`. Read through `SELECT_CATALOG_ROLE`; when refused, recorded as
`unknown` with the error, never as `off`.

### 3.5 Into the ledger

`plugins/oracle`'s scaffold `OracleDbClient` (a stub since RFC 0012) is replaced by a client over
`ora-live`. A project names a database in `ekos.toml` — the password's **variable name**, never its
value — and `ekos build` runs one observer per entry, emitting one artifact per catalog object
(target `oracle://<database>/<OWNER>/<kind>/<NAME>`):

```toml
[[databases]]
name = "idempiere-db"          # shares the [[sources]] namespace
kind = "oracle"
connect = "localhost:51521/XEPDB1"
user = "ekos_ro"
password-env = "EKOS_ORACLE_PASSWORD"
owners = ["ADEMPIERE"]
```

At `ekos recover`, `OracleCatalogPass` (`recovery/src/oracle_catalog_analyzer.rs`) turns them into:

- `Table` objects with `dialect: oracle`, `origin: live`, `columns` in the shape
  `sql_analyzer::columns_json` writes (so identity, docs and EKL treat them alike), `owner`,
  `status`, profile facts as properties;
- `View`, `Procedure`, `Trigger` objects keyed by owner and name; routine text goes through
  RFC 0210's PL/SQL reader, so live routines get the same `ProcedureStatement` objects as files;
- `ForeignKey` relationships from `ALL_CONSTRAINTS`;
- one `Custom("OracleCatalog")` summary (census by kind and status, log-state facts, write-probe
  result) per connection.

Live objects never merge with file-derived ones: their ids are derived from
`oracle-live-<kind>:<database>/<owner>/<name>`, and `identity::candidate_blocks` skips every object
with `origin: live`. Name lookups (`procedure_lineage::NameIndex`, `data_lineage`) consult live
objects only for a name no file defines — a fallback layer, so adding a database never makes a
previously unique file-defined name ambiguous.

### 3.6 Code ↔ database conflicts

A commit-time step (`semantic::live_conflicts`), after RFC 0210's `link_changes`, writes
`ConflictingEvidence` items (RFC 0172) — each cites both sides:

| Code claim | Database | Conflict |
|---|---|---|
| `ModelTable` T | no table T | `model_table_missing` |
| table T (owner schema) | no `ModelTable` T | `table_without_model` (customisation or leftover) |
| `ModelTable` column mandatory | column nullable | `mandatory_but_nullable` |
| `ModelTable` column | column absent | `model_column_missing` |
| `SchemaChange` add/modify column, set applied before the seed | column absent or different | `change_not_applied` (**not built**, see Results) |
| `SchemaChange` newer than the database | — | the `explanation` of the conflict it accounts for, **not** a conflict of its own |
| `db/oracle` routine file | `*_SOURCE` text differs (comments removed, case and whitespace normalised) | `routine_text_differs` |
| — | object `INVALID` | counted on the summary line (`invalid_objects`) |

**Explanations.** A conflict the source tree accounts for carries an `explanation`, shown by
`ekos conflicts show` and counted as *explained*:

- `pending_migration` — a `SchemaChange` that creates, adds or renames to the missing name (or makes
  the column `NOT NULL`) and is newer than the database. *Newer* has two horizons: the **time
  horizon** is the latest scripted addition the catalog contains; the **release horizon** is the
  newest release (migration directory) most of whose additions the catalog contains. A script is
  dated when written, not when shipped, so a release newer than the database's holds scripts dated
  before the time horizon that the database lacks (iD13's accounting dimensions, dated 2024-05).
  The database's own engine's script is cited first.
- `translation_table` — a `*_TRL` table whose base table has a model.

**Not compared.** Virtual columns (the generated setter throws `is virtual column`), names the
redaction baseline changed, and names that match more than one table or routine (counted as
ambiguous). A routine defined both in `db/oracle` and in migration scripts is compared with the
`db/oracle` definition.

`link_changes` also links changes to live `Table`s, which is what closes devlog_287's 17,171
unlinked changes. Every conflict kind is counted on the commit summary line; RFC 0172's `resolve`
stays human-only.

## 4. Phases

| Phase | Deliverable | Exit |
|---|---|---|
| 0 | testbed, `ekos_ro`, census | seed imported, census committed as measured |
| 1 | `ora-live`: session, guards, write probe, catalog, reconciliation | live suite (`EKOS_ORACLE_LIVE=1`) green; census reconciled exactly |
| 2 | observer + `OracleCatalogAnalyzerPass` + change links | whole-tree run measured: tables, links before/after |
| 3 | code ↔ database conflicts | counts by kind, each kind sampled and explained |
| 4 | profiles P0–P2 + log facts | profile run on the testbed under the call timeout; no row value in the ledger (test) |

## 5. Tests

- Mapping tests over recorded catalog rows (no database): every kind, `SYS_C` folding, truncated
  view text, unknown `OBJECT_TYPE` refused, reconciliation mismatch refused.
- Live suite behind `EKOS_ORACLE_LIVE=1`: guards observable in `V$SESSION`/`NLS_SESSION_PARAMETERS`,
  write probe refused for `ekos_ro` and permitted-then-empty for a writable user, census equality.
- A ledger scan after a profiled run finds no value from a seeded marker row (the "no row values"
  gate).

## 6. Risks

| Risk | Mitigation |
|---|---|
| Instant Client missing on a user's machine | feature off by default; `ekos doctor` names the library it could not load |
| `ALL_*` shrinks with privileges | reconciliation against `ALL_OBJECTS` makes it an error, not a smaller census |
| `LONG` columns in the dictionary | read as `LONG` (`_VC` is 23ai-only and cuts at 4,000); a read shorter than the recorded length is marked truncated |
| Seed import fails on XE 21c | 23ai Free is the second candidate; the PostgreSQL twin keeps steps 1–3 demonstrable |

## 7. Results

### Phase 0 — testbed (2026-10-23)

`evals/oracle-idempiere/part0/census.md`. The seed imported in 3 min 5 s with three impdp warnings:
the expected `ORA-31684` (user exists), and the rows of `TEST` and `AD_SCHEDULER` not loaded
(`ORA-30094`, time-zone file version 26 missing from the slim image — tables present, rows absent;
to be fixed before Phase 4). `AfterImport.sql` clean, 0 invalid objects. Census: 903 tables, 160
views, 2,109 indexes, 48 functions, 14 procedures, 1 type; 30,457 columns; 892 / 3,686 / 908 /
12,958 primary / foreign / unique / check constraints.

### Phase 1 — `ora-live` (2026-10-23)

Live suite, 5 tests, as `ekos_ro`: session guards visible in `NLS_SESSION_PARAMETERS` and
`V$SESSION`; write probe refused (`ORA-01031`) for `ekos_ro`, accepted for the schema owner; the
catalog equals the census for every kind (check constraints: 1,892 listed + 11,066 folded `NOT NULL`
= 12,958); introspection 1.5 s; no view text truncated (longest 49,346 characters). Through `ALL_*`
the same user sees 0 of 48 functions — the reason for the `DBA_*` default.

### Phase 2 — into the ledger (2026-10-23)

Whole tree (`release-13` @ `3355eb5fc9`) plus the testbed, release binary built with `--features
oracle`, scratch `EKOS_HOME`. `ekos build` read 10,613 catalog objects in 13 s. `OracleCatalogPass`:
903 tables, 3,686 foreign keys (0 unresolved), 160 views (0 unread), 62 routines (61 parsed
completely), 990 statements.

| | Before (devlog_287) | After |
|---|---|---|
| schema changes linked to their table | 11,748 of 31,620 | **28,916** of 31,620 |
| changes naming a table nothing defines ("not compiled") | 17,171 | **3** |

The remaining 2,701 are ambiguous names (980 names spread over more than one table), left unlinked
by design. Two cache-key defects surfaced: RFC 0209's Java pass had no logic version in its key, and
the catalog pass lacked the versions of the PL/SQL and view readers it calls — both served stale
results after a code change (15 routines briefly "differed" because of it).

### Phase 3 — code ↔ database conflicts (2026-10-23)

754 tables and 22 routines compared; 37 names ambiguous; 52 virtual and 8 redacted columns not
compared; 0 invalid objects. `ekos commit` 212 s, peak 2.8 GB (the whole commit, not this step).

| Conflict | Count | Explained | Unexplained, sampled |
|---|---|---|---|
| `model_table_missing` | 9 | 9 pending (7 iD13, 2 iD12) | — |
| `table_without_model` | 153 | 66 translation tables | 87: modules removed from code but not from the seed (`CM_*` web CMS, `K_*` knowledge base, `W_*` web store, `M_Promotion*`), plus `T_*` work tables |
| `model_column_missing` | 165 | 164 pending (135 iD13, 29 iD12) | `AD_TreeBar.AD_Menu_ID`: declared by the model since the 4.1 regeneration (2016), added by no script, absent from the database |
| `mandatory_but_nullable` | 116 | — | dictionary says mandatory, column nullable; whether rows hold nulls is Phase 4's question |
| `routine_text_differs` | 1 | — | `AD_Column_Sync`: the installed procedure builds `NVARCHAR2(n)` columns, the file builds `VARCHAR2(n)` |

Explained: 102 of 444 with the time horizon alone, 238 with the release horizon, **239** with
renames. The database's
release horizon is iD11 and its time horizon 2024-12-06T14:52.

**Explanations checked against the database's own record.** iDempiere writes every applied script
to `AD_MigrationScript` (1,211 rows, the last `202412222157_IDEMPIERE-6348_Version.sql`). Each of the
173 `pending_migration` explanations names a script that is **not** in it — 0 wrong. The record also
shows why a directory is not a release: the seed's applied scripts are labelled release 11 while the
tree files them under `iD12`, and `iD12` keeps receiving back-ports into 2026. The rename case
(`AD_Sequence_No.SequenceKey`, iD13 `RENAME COLUMN CalendarYearMonth TO SequenceKey`) was the
measured gap that made a pending rename an explanation too.

**Not built.** `change_not_applied` — reading `AD_MigrationScript` would decide it exactly, but it is
a dictionary row read, which decision D3 keeps out of this RFC; it is the first candidate for the
opt-in cross-check. Ambiguous routines (37 names) are counted, not compared. Profiles and log facts
are Phase 4.

