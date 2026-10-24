# RFC 0212 — Write Behaviour: How Each Table Is Updated and Deleted

**Status:** Accepted
**Date:** 2026-10-23
**Depends on:** RFC 0163 (statement footprints), RFC 0209 (`ModelTable`, embedded SQL), RFC 0210
(PL/SQL), RFC 0211 (live catalog, foreign keys, profiles)
**Plan:** `oracle-idempiere-plan.md` v3, Part I.7 (core part). Part III (detecting deleted rows in
the lakehouse) consumes this and is out of scope here.
**Testbed:** iDempiere `release-13` @ `3355eb5fc9` + the release 12 seed (RFC 0211)

---

## 1. Motivation

An incremental copy of a table needs two facts the schema does not state: whether every change
moves a timestamp it can use as a watermark, and whether rows ever disappear. iDempiere's `PO` layer
sets `Updated` on every save (`PO.saveUpdate`, unless the caller set it), but raw `UPDATE`s in Java
and PL/SQL may not; rows leave through `PO.delete`, raw `DELETE`s, `ON DELETE CASCADE`, age-based
purges (`DBA_Cleanup`: `DELETE FROM AD_Session WHERE Created < SysDate-7`) and, for most tables, the
user interface.

Measured on the testbed before this RFC: 97 embedded-SQL `DELETE` edges reach 72 model tables;
PL/SQL holds 16 delete statements; the live catalog has 426 `ON DELETE CASCADE` and 148
`SET NULL` foreign keys; about 300 `delete(bool)` / `deleteEx(bool)` calls on model objects are not
recorded at all (no call graph, RFC 0209 §2). And 705 of 828 active tables are marked deletable by
users in the application dictionary (`AD_Table.IsDeleteable`), which no source file states — so a
classification from code alone would call most tables "never deleted" and be wrong.

## 2. Goals and non-goals

**Goals**

1. Per table, deterministic and cited: an **update class** and a set of **delete classes**, each
   with the evidence that put it there.
2. The missing inputs: the columns each `UPDATE` sets, the shape of each `DELETE`, and `PO`
   `delete`/`deleteEx` calls whose receiver type is declared in the method.
3. An **opt-in dictionary reader** for iDempiere-family schemas: `AD_Table.IsDeleteable` and
   `IsChangeLog`, through RFC 0211's read-only session.
4. The change-log fact: whether the application records deletes of a table, with the conditions
   verified in the pinned source.

**Non-goals**

- Detecting deleted rows, tombstones, mass-delete guards (Part III, the lakehouse).
- A Java call graph. Receivers are resolved only from declarations in the same method.
- Reading `AD_ChangeLog` rows (they are row values) or any table other than `AD_Table`.

## 3. Design

### 3.1 Statement shapes (`plpgsql_footprint`)

`Footprint` gains, per written table:

- `update_sets: {table: [column]}` — the `SET` targets of each `UPDATE` (lowercased; `SET (a, b) =`
  expands).
- `delete_shapes: {table: shape}` — `all_rows` (no `WHERE`, or `TRUNCATE`), `age` (a `WHERE` that
  compares a column to the current time: `SYSDATE`, `CURRENT_DATE`, `CURRENT_TIMESTAMP`, `NOW()`,
  `getDate()`, optionally minus or plus an interval), `selective` (anything else).

Both are recorded on `ProcedureStatement` (PL/SQL, PL/pgSQL) and on Java `WritesTo` edges
(`sets`, `delete_shapes`). Bump the footprint consumers' logic versions.

### 3.2 `PO` calls (`java_analyzer`)

In a method body, a call `v.delete(…)`, `v.deleteEx(…)` where `v` is a local variable, parameter or
enhanced-`for` variable whose declared type is a model class (`M*`, `X_*`, or any class the
application model maps to a table) becomes `WritesTo {operations: ["delete"], via: "po"}` to that
table's `ModelTable`. Inside a model class, an unqualified `delete(…)` / `deleteEx(…)` or
`this.delete…` targets the class's own table. Receivers whose type is not declared in the method
(`getLines()[i].delete…`, a field, a generic `PO`) are counted as `po_delete_unresolved`, not
guessed. Test code (`src/test`, `org.idempiere.test`, `*Test.java`) is counted separately and never
gives a class.

### 3.3 Dictionary reader (opt-in)

`[[databases]] dictionary = "idempiere"` makes the RFC 0211 observer also read

```sql
SELECT TableName, IsDeleteable, IsChangeLog, IsView, IsActive FROM <owner>.AD_Table
```

inside the same read-only transaction. These are flags describing tables, not business data; the
query names its columns and table literally, and the result is one `dictionary` artifact per owner.
Default off (plan decision D3: dictionary rows are an opt-in cross-check). Without it, every class
that depends on the dictionary says `unknown`, never a guess.

### 3.4 Classification (`semantic::write_behaviour`, at `ekos commit`)

For each live table (or each `ModelTable` when no database is configured), by unique name:

**Update class**

| Class | When |
|---|---|
| `po_only` | no raw `UPDATE` of the table anywhere in code |
| `raw_sql_sets_updated` | raw `UPDATE`s exist and every one sets `Updated` |
| `raw_sql_skips_updated` | at least one raw `UPDATE` does not set `Updated` — the watermark misses it |
| `no_updated_column` | the table has no `Updated` column |

**Delete classes** (a set; a table can be deleted several ways)

| Class | Evidence |
|---|---|
| `deleted_by_app` | `PO` delete call (§3.2), or a `selective` raw `DELETE` in Java / PL/SQL / a trigger |
| `deleted_by_users` | dictionary `IsDeleteable = 'Y'` (the UI checks it in `GridTab`) |
| `deleted_by_cascade` | a foreign key to a parent with `ON DELETE CASCADE` (live catalog) |
| `purged` | an `age` or `all_rows` `DELETE` / `TRUNCATE` |
| `soft_delete_only` | none of the above, an `IsActive` column, and dictionary `IsDeleteable = 'N'` |
| `never_deleted_in_code` | none of the above and the dictionary not read — **not** a claim about users |

**Change log** — `recorded` when the dictionary says `IsChangeLog = 'Y'`, `not_recorded` when
`'N'`, `unknown` without the dictionary. Verified in `PO.delete` (`release-13`): a delete is logged
per column only when the table's flag is on **and** a session exists, so a background process
without a session leaves no trace; raw SQL is never logged. The fact carries that caveat.

Output: one `Custom("WriteBehaviour")` per table (keyed, id from database/owner/table or the model
table), `Describes` its table, with `update_class`, `delete_classes`, `change_log`, and `evidence`:
one entry per fact — kind (`po_delete`, `raw_delete`, `raw_update`, `cascade`, `dictionary`),
source path and line or catalog target, and the statement shape. The commit summary line counts each
class. Queried through EKL (`FIND Object WHERE kind = 'WriteBehaviour' AND …`).

## 4. Phases

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | statement shapes + `PO` delete calls | unit tests; whole-tree counts of resolved / unresolved `PO` deletes |
| 2 | dictionary reader | live suite reads `AD_Table` flags; counts equal a direct query |
| 3 | classification at commit | counts by class on the whole tree; each class sampled against source |

## 5. Tests

- Shapes over SQL fixtures: `all_rows`, `TRUNCATE`, `age` with `SysDate-7`, `NOW() - INTERVAL`,
  `selective`; `SET (a, b) =`.
- `PO` receivers: local, parameter, enhanced `for`, unqualified inside a model class; unresolved
  receivers counted; test code excluded.
- Classification over hand-built graphs, one per class, and the dictionary-absent case producing
  `unknown` / `never_deleted_in_code`.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Receiver resolution misses deletes (fields, chained calls) | counted as unresolved and reported; the dictionary covers user deletes |
| A dynamic `DELETE` (`'DELETE FROM ' || t.TableName`) names no table | counted on the routine; not attributed to a table |
| The dictionary differs from what the UI enforces in another release | the claim is cited to `GridTab` / `PO` in the pinned release |

## 7. Results

All on iDempiere `release-13` @ `3355eb5fc9` + the release 12 seed, release build, scratch
`EKOS_HOME`, `[[databases]] profile = "p2"`, `dictionary = "idempiere"`. No model calls.

### Phase 1 — statement shapes and `PO` deletes (2026-10-23)

Embedded SQL unchanged: 3,207 statements (1,992 parsed, 996 partial, 219 unparsed). `PO` delete
calls with arguments: **86 attributed to a table**, 63 whose receiver the method does not declare
(fields, `getLines()[i]`, a generic `PO`), 198 in test code. Before this RFC none were recorded.

### Phase 2 — dictionary reader (2026-10-23)

Live suite, 7 tests. As `ekos_ro`, `AD_Table` lists 1,056 entries (views and inactive tables
included): 765 deletable by users, 718 change-logged — equal to a direct `COUNT` in the same test.

### Phase 3 — classification (2026-10-23)

903 live tables, each with a `WriteBehaviour` (commit step about 150 s for the whole commit, peak 3 GB):

| Update class | Tables |
|---|---|
| `po_only` | 737 |
| `raw_sql_skips_updated` | 144 |
| `raw_sql_sets_updated` | 8 |
| `no_updated_column` | 14 |

| Delete class (a table can have several) | Tables |
|---|---|
| `deleted_by_users` | 754 |
| `deleted_by_cascade` | 298 |
| `deleted_by_app` | 107 |
| `purged` | 12 |
| `soft_delete_only` | 57 |
| `never_deleted_in_code` | 9 |

Change log: 713 `recorded`, 180 `not_recorded`, 10 `unknown` — the 10 tables `AD_Table` does not
list (Oracle's `JAVA$…`/`CREATE$JAVA$…` tables, `T_*` helper tables, `M_Product_BOM_Old`, …).

Sampled against source: `AD_Note` is purged by `NoteDelete.doIt` (`(Created+N) < getDate()`) and
users; `M_StorageOnHand` is deleted by `MProduct.removeStorageRecords` and purged by
`StorageCleanup` (age-based); `AD_PInstance` by `Cleanup.sql` (`DELETE FROM AD_PInstance`, all rows)
and `DBA_Cleanup`; `AD_Workflow_Access` skips `Updated` in `MRole.updateAccessRecords` (an
`UPDATE … SET AD_Workflow_Access_UU = generate_uuid()`), so a watermark on `Updated` misses that
change; `C_DunningRunEntry` is deleted through `MDunningRun.deleteEntries` (`PO` delete).

**Defects found while sampling, fixed:**

- Statements in **undated** historic migration scripts (`migration-historic/…/456_FR2783860.sql`,
  which redefines `DBA_Cleanup`) were counted: they never became RFC 0210 change sets. Any path
  with a `migration…` directory now counts as a migration script (classes moved by 1–2 per class).
- The citations property was first named `evidence`, which EKL reserves for an object's evidence
  ids — queries returned an empty list. It is `citations`.

**Not done:** user deletes and change logging stay `unknown` without the dictionary (by design);
the 63 unresolved `PO` deletes would need field and return types (a call graph, out of scope).
