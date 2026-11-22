# Devlog 352 — Talend patterns to dbt, RFC 0268 phase 2 (idioms)

**Date:** 2026-11-22
**PRs:** none (direct to `main`); lakehouse devlog_28
**Branch:** `main`

---

## Summary

RFC 0268 phase 2's pattern idioms are designed, measured on the fixtures and shipped in ekos-lakehouse. A Talend job
keeps state in its targets, and phase 1 left most such models disabled. Five shapes are now recognised by their
structure:
- a hand-built SCD2 becomes a dbt snapshot;
- a delete of the matched rows then an insert becomes one merge on the key;
- a rejects table with many writers becomes one `rej_` model;
- a write-back flag has no model;
- a lookup on the target reads `{{ this }}`.

A job that only looks like one of these is refused, with the difference named. The work also found five lowering
defects that no gate saw; a new scope gate catches the first kind. The console tab and `ekos_talend_explain` are
the rest of phase 2.

---

## RFC 0268 phase 2 idioms (EKOS docs; lakehouse devlog_28)

### Problem / motivation

Phase 1 disabled most of fixture B's gold models, for three reasons:
- **two writers of one table** (32 models);
- **a model reading what it writes** (16 models);
- **every model reading one of those.**

These are Talend's state-keeping patterns: SCD2 by hand, rejects tables, lookups on the target. They need dbt
forms, recognised safely.

### What was built

| Part | What |
|---|---|
| RFC 0268 §Phase 2 design | The five shapes, each with its conditions, its dbt result and its refusals; the lowering changes for B's calendar; an exit; a result with amendments |
| Pattern recognition (lakehouse `talend-dbt`) | Snapshot, keyed replace, write-back, `rej_` union, self-read through `{{ this }}`; `Report.patterns` |
| Scope gate | Every column a node reads is in its own input |
| `lakehouse talend translate` | `--faithful <job>`, `--locale <tag>` |
| Capabilities page | The patterns added to the Talend → dbt paragraph |

### Result (synthetic plan, `--locale fr_FR`)

| Fixture | Phase 1 enabled | Phase 2 enabled |
|---|---|---|
| B | 10 / 73 | 18 / 44 (2 / 5 snapshots) |
| C | 42 / 61 | 42 / 61 |
| D | 9 / 23 | 9 / 23 |
| E | 17 / 184 | 25 / 164 |

- **B, recognised:**
  - five of seven SCD2 shapes are snapshots;
  - `ODS_REJET` is one `rej_` model over 18 writers;
  - the `ODS_VENTE` write-back has no model;
  - the calendar is enabled.
- **B, refused by name:**
  - jOdsClient's "close" *inserts* a NULL row;
  - jOdsSousCategorie inserts a new version of every matched row, changed or not.

  Everything downstream of these two stays disabled, which is most of B's DWH, until a reviewer answers.
- **E:**
  - 10 jobs are keyed replaces (3 enabled; the rest read sources outside the lakehouse);
  - 10 more jobs are refused by name.
- **Gates:** no model on B–E is disabled by a gate.
- **`dbt parse`** accepts B's project: 16 models, 2 snapshots.
- **Golden:** the Pentaho manifests are identical on the three corpora.

### Decisions

- **Refuse rather than recognise something that only looks like an SCD2,** and state the difference. The plan's
  risk table names this case.
- **The snapshot keeps history that Talend overwrites.** B's closing updates write NULL into every version under
  the key. A `snapshot` question says so.
- **`--locale` states the JVM's locale.** Week numbers and month names follow it, and the project does not say
  which it is.

---

## Knowledge Captured

- **Measure the fixtures before designing idioms.** The plan expected "two writers → one merge". The fixtures held
  three different shapes (SCD2, delete-then-insert, many-writer rejects), and two SCD2-looking jobs that are not
  SCD2s.
- **An anti-invention gate that checks names against the whole job misses scope errors.** A column read after its
  flow dropped it passed every gate. Check each node against its own input.
- **Three-part column references (`schema.table.column`) break once a table becomes an aliased `ref()`.** Only
  Trino's `EXPLAIN` would have said so; the parse gate accepts them.
- **Run fixture translations on a main-thread-sized stack** (8 MiB in tests, 64 MiB in the command). A test
  thread's or async worker's 2 MiB overflows in the SQL parser on long concatenations.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0268-talend-to-dbt.md` | phase 2 design, exit, result and amendments |
| `docs/generated/ekos-self-documentation.html` | Talend patterns |
| `TODO.md`, `CLAUDE.md` | phase 2 idioms ticked, console/MCP and the chained keyed replace listed; devlog pointer |
| ekos-lakehouse (devlog_28) | `talend-dbt::patterns`, snapshots, `rej_`, self-read, scope gate, `talend-rel` lowering fixes, `etl-dbt` qualifier fix, flags |
