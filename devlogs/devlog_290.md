# Devlog 290 — Write behaviour: how each table is updated and deleted (RFC 0212)

**Date:** 2026-10-23
**PRs:** none (local)
**Branch:** `main` (local, not pushed)

---

## Summary

The last core RFC of Part I of the Oracle/iDempiere plan. Every table now carries a
`WriteBehaviour`: whether updates always move its `Updated` timestamp, and every way its rows can
disappear — application code (raw `DELETE` or `PO` delete), users (the application dictionary),
cascading foreign keys, age-based purges — each with a citation. On iDempiere, 144 of 903 tables
have a raw `UPDATE` that leaves `Updated` alone (a watermark on it misses those changes), and 754
are deletable by users. That second number is invisible in code: it comes from an opt-in read of
`AD_Table`'s flags, without which a code-only classification would call most tables "never
deleted".

---

## RFC 0212 — write behaviour

### Problem / motivation
An incremental copy needs two facts no schema states: does every change move a usable timestamp,
and do rows ever disappear. The inputs were partly in the ledger (embedded-SQL `DELETE` edges to 72
tables, 16 PL/SQL deletes, 426 cascading foreign keys) and partly missing: the columns an `UPDATE`
sets, the shape of a `DELETE`, about 300 `PO` delete calls, and the dictionary's
`IsDeleteable` / `IsChangeLog`.

### What was built

| Component | What it does |
|---|---|
| `plpgsql_footprint` | `update_sets` (columns each `UPDATE` sets) and `delete_shapes` (`all_rows`, `age`, `selective`) per table |
| `java_sql` / `java_analyzer` | the same on Java statements; per-statement shapes on `WritesTo` edges |
| `recovery/src/java_po.rs` | `delete(…)` / `deleteEx(…)` receivers resolved from method-local declarations; test code counted apart |
| `java_analyzer` | class → table map (generated, business and sub-classes); `WritesTo {via: "po"}` edges |
| `ora-live/src/dictionary.rs` | opt-in `AD_Table` flags read (`[[databases]] dictionary = "idempiere"`) |
| `OracleCatalogPass` | `dictionary` flags on each live table |
| `semantic/src/write_behaviour.rs` | the classifier: update class, delete classes, change log, citations |
| `ekos commit` | `Write behaviour:` summary line; one `WriteBehaviour` per table, `Describes` its table |

### Measured (whole tree + release 12 seed, P2, dictionary on)

| | |
|---|---|
| `PO` deletes | 86 attributed, 63 unresolved receiver, 198 in test code |
| dictionary (`ekos_ro`) | 1,056 `AD_Table` rows: 765 deletable, 718 change-logged — equal to a direct count |
| update classes | `po_only` 737, `raw_sql_skips_updated` 144, `raw_sql_sets_updated` 8, `no_updated_column` 14 |
| delete classes | users 754, cascade 298, app 107, purged 12, soft-delete only 57, never deleted in code 9 |
| change log | recorded 713, not recorded 180, unknown 10 (tables `AD_Table` does not list) |

Sampled against source: `NoteDelete` purges `AD_Note` by age (`(Created+N) < getDate()`);
`StorageCleanup` and `MProduct.removeStorageRecords` delete `M_StorageOnHand`; `Cleanup.sql`
empties `AD_PInstance`; `MRole.updateAccessRecords` updates `AD_Workflow_Access` without touching
`Updated`.

### Implementation details worth remembering
- `age` = a `WHERE` with an ordering comparison one side of which mentions the current time
  (`SYSDATE`, `getDate()`, `NOW()`, `CURRENT_TIMESTAMP`, …). A date column compared with a literal
  date is `selective`.
- Only `delete`/`deleteEx` calls **with arguments** count: `PO.delete(boolean …)` always has one,
  `File.delete()` never.
- Statements in migration scripts are ignored — a migration's data fix runs once. Undated historic
  scripts are migrations too (any path with a `migration…` directory).
- A live routine's statements count only when no file defines a routine of that name.
- `PO.saveUpdate` sets `Updated` unless the caller did; `PO.delete` writes `AD_ChangeLog` event `D`
  per column only when `IsChangeLog` is on **and** a session exists. Both checked in the pinned
  source; the change-log fact carries that caveat.

### Decisions
- **Dictionary read opt-in.** Plan decision D3 keeps dictionary rows out by default; this is the
  opt-in cross-check it anticipated — one query, five named flag columns of `AD_Table`, no business
  data. Without it the classes say `unknown` / `never_deleted_in_code` rather than guess.
- **Delete classes are a set.** A table is often deleted several ways (`AD_Note`: users and purge).
- **Receivers from declarations only.** No call graph; unresolved calls are counted, not guessed.
- **Citations, not evidence.** EKL reserves `evidence` for an object's evidence ids.

---

## Knowledge Captured

- **In iDempiere, the dictionary — not code — decides most deletes.** 765 of 1,056 `AD_Table`
  entries are user-deletable through the UI (`GridTab` checks `IsDeleteable`). Any write-behaviour
  analysis of an iDempiere-family estate without the dictionary is wrong for most tables.
- **The seed's `AD_ChangeLog` is empty** (0 rows), so change-log completeness cannot be measured
  on the testbed; only the configuration (`IsChangeLog`) can.
- **198 of 347 `PO` delete calls are in tests** — a test tree can dominate counts; separate it.
- **EKL property names that collide with built-in columns** (`id`, `name`, `kind`, `evidence`,
  `created_at`) silently return the column, not the property.
- `scripts/cognitive-complexity.sh` flagged four functions this change pushed over the limit
  (`derive` 72, `recover_catalog` 40, the footprint visitor 32, `commit::run_with` 31); all were
  split back under it before commit.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0212-write-behaviour.md` | new RFC with results |
| `ekos/crates/recovery/src/plpgsql_footprint.rs` | `update_sets`, `delete_shapes`; `update` / `delete` visitor helpers |
| `ekos/crates/recovery/src/java_po.rs` | new: `PO` delete receivers |
| `ekos/crates/recovery/src/{java_sql,java_analyzer}.rs` | shapes on statements and edges; class → table map; `PO` edges; logic version 3 |
| `ekos/crates/recovery/src/{plpgsql_analyzer,plsql_analyzer}.rs` | shapes on statements; logic versions 8 / 4 |
| `ekos/crates/recovery/src/oracle_catalog_analyzer.rs` | dictionary flags on tables; helpers split out; logic version 4 |
| `ekos/crates/ora-live/src/dictionary.rs` | new: `AD_Table` flags reader |
| `ekos/crates/ora-live/tests/live.rs` | dictionary count test |
| `ekos/plugins/oracle/src/lib.rs` | dictionary artifact per owner |
| `ekos/crates/compiler-core/src/{config,lib}.rs` | `[[databases]] dictionary` |
| `ekos/crates/semantic/src/write_behaviour.rs` | new: the classifier |
| `ekos/crates/kir/src/custom_kinds.rs` | `WriteBehaviour` (keyed) |
| `ekos/crates/cli/src/commands/{commit,recover,build}.rs` | commit step and line; `PO` delete counts; dictionary wiring |
