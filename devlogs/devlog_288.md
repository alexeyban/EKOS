# Devlog 288 — Live Oracle connector and code ↔ database conflicts (RFC 0211, Part 0)

**Date:** 2026-10-23
**PRs:** none (local)
**Branch:** `main` (local, not pushed)

---

## Summary

Third step of the Oracle/iDempiere plan, and the first that reads a database. A Compose testbed
imports the iDempiere 12 seed into Oracle XE 21c with a read-only user; `crates/ora-live` reads the
catalog in guarded read-only sessions and reconciles it exactly with a census; `ekos build` /
`recover` bring the catalog into the ledger as ordinary `Table` / `View` / `Procedure` objects with
`origin: live`; and `ekos commit` compares the code's claims with the database as RFC 0172
conflicts. On the whole iDempiere tree, schema changes linked to their table go from 11,748 to
28,916 of 31,620 (changes naming a table nothing defines: 17,171 → 3), and 444 code ↔ database
conflicts are found, 239 of them explained by the source tree itself — every one of the 173
"pending migration" explanations confirmed against the database's own `AD_MigrationScript`.

---

## RFC 0211 — Live Oracle connector, code ↔ database conflicts

### Problem / motivation
RFCs 0209/0210 read what iDempiere *says* about its database: model classes, migration scripts,
PL/SQL files. The base schema ships as a Data Pump dump, so 17,171 recorded schema changes named
tables no file defines, and nothing could say whether a model column or a migration still matches
what is installed. The catalog is the authority.

### What was built

| Component | What it does |
|---|---|
| `docker-compose.oracle.yml` | `gvenzl/oracle-xe:21.3.0-slim`, loopback port 51521, named volume |
| `deploy/fixtures/idempiere-oracle.sh` | downloads the v12 server zip (SHA-256 pinned), runs the release's own import steps inside the container (`CreateUser`, `CreateDataPumpDir`, `impdp`, `AfterImport`), creates `ekos_ro` |
| `evals/oracle-idempiere/part0/` | census SQL + measured output (903 tables, 160 views, 62 routines, 30,457 columns, 0 invalid) |
| `crates/ora-live` `session.rs` | `OraSource` (feature `oracle`, rust-oracle 0.6.3 / ODPI-C): NLS + `TIME_ZONE='UTC'` pinned, module/action, call timeout, `SET TRANSACTION READ ONLY`, write probe (`INSERT … SELECT … WHERE 1=0` → `ORA-01031`), session privileges; `DriverNotBuilt` without the feature |
| `crates/ora-live` `catalog.rs` | 15 named queries over `DBA_*` (or `ALL_*`), `CatalogSource` trait (fixtures in tests), `introspect` + `reconcile` against `*_OBJECTS`, `SYS_C` `NOT NULL` checks folded into nullability |
| `plugins/oracle` | rewritten over `ora-live`: `OracleObserver`, one artifact per catalog object, targets `oracle://<db>/<OWNER>/<kind>/<NAME>` |
| `[[databases]]` (`compiler-core/config.rs`) | name, kind, connect, user, `password-env` (the variable's **name**), owners; shares the `[[sources]]` namespace |
| `recovery/src/oracle_catalog_analyzer.rs` | `OracleCatalogPass`: tables in `sql_analyzer`'s shape, FKs, views through the view reader, routines through the PL/SQL reader, one `OracleCatalog` summary |
| `semantic/src/live_conflicts.rs` | commit-time comparison: `model_table_missing`, `table_without_model`, `model_column_missing`, `mandatory_but_nullable`, `routine_text_differs`, each citing both sides, with an `explanation` when the tree accounts for it |
| `identity`, `procedure_lineage`, `data_lineage` | live objects never merge; name lookups use them only for names no file defines |

### Measured (iDempiere `release-13` @ `3355eb5fc9` + the v12 seed, release build, scratch `EKOS_HOME`, no LLM)

| | |
|---|---|
| seed import | 3 min 5 s; `TEST` / `AD_SCHEDULER` rows not loaded (`ORA-30094`, the slim image lacks time-zone file v26) |
| catalog read (live suite) | 1.5 s; equals the census for every kind; check constraints 1,892 listed + 11,066 folded = 12,958 |
| `ekos build` | 10,613 catalog objects, 13 s |
| `OracleCatalogPass` | 903 tables, 3,686 FKs (0 unresolved), 160 views (0 unread), 62 routines (61 complete), 990 statements |
| schema changes linked | 11,748 → **28,916** of 31,620; "not compiled" 17,171 → **3** |
| `ekos commit` | 212 s, peak 2.8 GB |

Conflicts (754 tables and 22 routines compared, 37 names ambiguous, 52 virtual and 8 redacted
columns not compared):

| Conflict | Count | Explained | Unexplained |
|---|---|---|---|
| `model_table_missing` | 9 | 9 pending migrations | — |
| `table_without_model` | 153 | 66 translation tables | 87 — modules removed from code, not from the seed: `CM_*`, `K_*`, `W_*`, `M_Promotion*`, `T_*` |
| `model_column_missing` | 165 | 164 pending migrations | `AD_TreeBar.AD_Menu_ID` — in the model since 2016, in no script, not in the database |
| `mandatory_but_nullable` | 116 | — | real; whether rows hold nulls is Phase 4 |
| `routine_text_differs` | 1 | — | `AD_Column_Sync`: installed builds `NVARCHAR2(n)`, file builds `VARCHAR2(n)` |

Explained over the session: 102 (time horizon) → 238 (release horizon) → 239 (renames) of 444.

### Implementation details worth remembering
- Introspection runs inside `SET TRANSACTION READ ONLY`; a write there is `ORA-01456`, so a bug in
  a query cannot write even for a privileged user.
- `LONG` dictionary columns (`TEXT`, `DATA_DEFAULT`, `SEARCH_CONDITION`) are read directly; the
  driver returns them whole (longest view 49,346 characters). `DATA_DEFAULT_VC` is 23ai-only
  (`ORA-00904` on 21c) and `TEXT_VC` cuts 40 of 160 views at 4,000.
- Routine text is compared by `plpgsql_analyzer::routine_text_hash`: comments masked (Oracle
  rules), lowercased, whitespace collapsed, `CREATE [OR REPLACE] [EDITIONABLE]` head and trailing
  `/`/`;` dropped. Without the comment mask, routines "differed" by their license header alone (Compiere vs
  Adempiere).
- A routine defined in both `db/oracle` and migration scripts is compared with the `db/oracle`
  copy — migrations are history.
- A virtual column is recognised from the generated setter (`throw new IllegalArgumentException
  ("… is virtual column")`) and not compared.
- The explanation cites the database's own engine's script first (`max_by_key((engine match, at,
  path))`) — the first version cited PostgreSQL scripts for an Oracle database.
- A `rename_column` change introduces its *target* name (`name` property); `column.name` is the old
  one.

### Decisions
- **`DBA_*` by default, not `ALL_*`.** Through `ALL_*` the read-only user sees 0 of 48 functions,
  and the list and the count shrink together, so reconciliation passes. `SELECT_CATALOG_ROLE` is
  the safe grant; `EXECUTE` would let the user run routines that write.
- **Two horizons.** A script is dated when written, not when shipped. iD13's accounting dimensions
  are dated 2024-05, before the database's latest scripted addition (2024-12-06), and still pending.
  The release horizon is the newest migration directory most of whose additions the catalog holds
  (majority, so one coincidental column cannot move it).
- **Pending is an explanation, not a conflict kind.** The conflict stays (the code does claim a
  column the database lacks); the explanation says why it is expected.
- **`AD_MigrationScript` not read by EKOS.** It decides `change_not_applied` exactly, but it is a
  dictionary row read, which plan decision D3 keeps out of this RFC. It was used here only to check
  the explanations by hand; it is the first candidate for the opt-in cross-check.
- **Live objects as a fallback name layer.** Adding a database must not make a name that one file
  defines ambiguous, or every existing link would drop.
- **Local XE, not a cloud Oracle.** The testbed rebuilds from the repo at no cost; a cloud database
  adds credentials and an outside dependency without changing a measurement.

---

## Knowledge Captured

- **A migration directory is not a release.** The v12 seed records its applied scripts as release
  11 while the tree files them under `migration/iD12`, and `iD12` keeps receiving back-ports into
  2026. Derive the database's position from what the catalog contains, never from a directory name.
- **iDempiere's v12 `ImportIdempiere.sh` cannot target a container** (`ORACLE_DOCKER_CONTAINER`
  arrived in release 13); run its four steps inside the container instead.
- **The slim XE image lacks time-zone file v26**: rows with `TIMESTAMP WITH TIME ZONE` data fail to
  import (`ORA-30094`), silently apart from an impdp warning. Use the full image for profiling.
- **Pass cache keys must include every reader's logic version.** The Java pass had none and the
  catalog pass omitted the PL/SQL and view readers it calls; both served stale objects after a code
  change. When a pass calls another analyzer's functions, fold that analyzer's `LOGIC_VERSION` in.
- **Instant Client must be on `LD_LIBRARY_PATH`** at run time; the `oracle` feature compiles
  without it (ODPI-C loads it lazily), so a build that passes says nothing about a machine that
  can connect.
- **Memory:** the whole-tree `ekos commit` peaks at 2.8 GB; with Oracle XE, the lakehouse stack and
  browsers on a 15 GB machine, an earlier run was killed mid-commit. Re-run the whole pipeline after
  a killed commit (cached stages are cheap) rather than resuming an append-only ledger halfway.

---

## Files Changed

| File | Change summary |
|---|---|
| `docker-compose.oracle.yml` | new: Oracle XE 21c testbed |
| `deploy/fixtures/idempiere-oracle.sh` | new: seed download, import, `ekos_ro` |
| `evals/oracle-idempiere/part0/` | new: census SQL, output, summary |
| `ekos/docs/rfcs/0211-live-oracle-connector.md` | new RFC, Phases 0–3 results |
| `ekos/crates/ora-live/` | new crate: sessions, guards, write probe, catalog, reconciliation, live suite |
| `ekos/plugins/oracle/` | rewritten over `ora-live` |
| `ekos/crates/compiler-core/src/config.rs`, `lib.rs` | `[[databases]]` |
| `ekos/crates/cli/src/commands/{build,recover,commit,conflicts}.rs`, `app.rs`, `Cargo.toml` | observer per database, catalog pass + stats, live conflicts at commit, explanations in `conflicts show`, feature `oracle` |
| `ekos/crates/recovery/src/oracle_catalog_analyzer.rs` | new: `OracleCatalogPass` |
| `ekos/crates/semantic/src/live_conflicts.rs` | new: code ↔ database conflicts + explanations |
| `ekos/crates/recovery/src/plpgsql_analyzer.rs` | `routine_text_hash`, `text_hash` property, logic version 7 |
| `ekos/crates/recovery/src/{plsql_analyzer,view_analyzer,java_analyzer,schema_history}.rs` | logic versions (Java gains one), `release` on `SchemaChange` |
| `ekos/crates/recovery/src/app_model.rs` | `virtual_column` on `ModelColumn` |
| `ekos/crates/semantic/src/{procedure_lineage,data_lineage,triggers,schema_history,business_semantics}.rs` | live objects as a fallback name layer |
| `ekos/crates/identity/src/lib.rs` | `origin: live` never a merge candidate |
| `ekos/crates/kir/src/custom_kinds.rs` | `OracleCatalog` (keyed) |
| `ekos/crates/plsql/src/lib.rs` | re-exports `mask_comments` |
| `ekos/Cargo.lock`, `tests/integration/Cargo.lock` | new crates; `ctutils` 0.4.2 → 0.4.5 (RUSTSEC-2026-0385, published 2026-10-23, transitive via `digest`) |
