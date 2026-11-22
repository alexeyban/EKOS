# Devlog 350 — Talend translation, RFC 0267 phase 2: ELT maps, statements, watermarks, row order

**Date:** 2026-11-22
**PRs:** none (direct to `main`); lakehouse devlog_26
**Branch:** `main`

---

## Summary

Phase 2 of the Talend lowering (RFC 0262 Slice 3), in ekos-lakehouse's `talend-rel`. It needed one EKOS change: a
component-framework Row's query is now lifted into `QUERY`.

Fixture E goes from 73% of its data components lowered to 100%:
- its ELT maps are read through sqlparser;
- its Row statements become a new shared node;
- its watermark `tJavaRow`s are lowered.

Fixture B reaches 98%.

The run also fixed a phase 1 mistake: 12 queries in E were marked `exact` while they read a context variable a
`tJavaRow` sets from a query at run time.

---

## RFC 0267 phase 2 (EKOS `69577df4`, `859708c4`; lakehouse `dcfdcff`)

### Problem / motivation

Phase 1 left these as gaps:
- in fixture E, 21 ELT maps and 42 `tSnowflakeRow` statements;
- in fixture B, 7 `Numeric.sequence` uses;
- in fixtures C and E, three custom routines;
- `tJavaRow`, which was never part of the flow at all.

Its result also asked for `UNIQUE_MATCH` to be checked against fixture B's generated Java.

### What was built

| Part | What |
|---|---|
| RFC 0267 §Phase 2 design | Written and committed before any code, from a survey of B–E |
| `ekos-talend` `framework_params` | A Row with no `manualQuery` setting has its `query` lifted into `QUERY`. Talend pass v7. |
| `talend-rel` `sql` | SQL → `etl-expr` by whitelist, in the engine's dialect (Snowflake functions; coercion and division rules) |
| ELT maps | Joins, `WHERE`, `GROUP BY` (aggregates anywhere in an output), `ORDER BY`, then the projections |
| `Op::Statement` (`etl-rel`) | `tDBRow` / `tXRow`; `UPDATE` / `DELETE` with their `SET` and `WHERE` lowered |
| `tJavaRow` | `output_row` assignments become a `Project`; `context.v = input_row.c` becomes `ContextFromData`; printing is dropped |
| Run-time context | A query or statement reading a variable set from a row, or replaceable at run time, is `rewritten` |
| Row order | `Numeric.sequence` becomes a window column; `tSampleRow` a line number and a filter |
| Digest routines | A custom MD5 / SHA-1 / SHA-256 hex routine is recognised, with its case, null check and charset |

### Result

| Fixture | Phase 1 | Phase 2 | Expressions lowered / gaps |
|---|---|---|---|
| B | 93% | 98% (123 / 125) | 482 / 4 |
| C | 87% | 89% (87 / 98) | 167 / 4 |
| D | 62% | 69% (29 / 42) | 40 / 2 |
| E | 73% | 100% (288 / 288) | 1353 / 0 |

The Pentaho golden manifests are byte-identical after adding `Op::Statement`:
- 5 lines on bench-gold;
- 286 on Pentaho's samples;
- 48 on AdventureWorks.

A fixture test parses the printed SQL of every lowered expression on B–E.

### Decisions

- **`HASH` becomes the MD5 of the same values as text, `rewritten`.** Snowflake's 64-bit hash has no equal on
  another engine. The same rows get the same key, but the values are new, so RFC 0268's differential run must not
  compare these columns.
- **`UNIQUE_MATCH` stays `rewritten`.** Fixture B's tMap code hands the mode to `AdvancedMemoryLookup`, a library
  class the fixture does not contain. The only lookup source the fixture has, tXMLMap's `DocumentLookupCache`, keeps
  the last row put. That is evidence, not proof, and a gated test pins both.
- **A sequence after a `tSortRow` stays `rewritten`**, against the design's `exact`: rows that tie on the sort keys
  can be numbered either way.
- **Components on a row link are part of the flow**, whatever their category, so the denominators of C and E grew.

---

## Knowledge Captured

- **Watermarks in Talend.** A `tJavaRow` does `context.last_loaded_date = input_row.LAST_LOADED_DATE`, and the
  next subjob's query concatenates that variable in. Folding the query with the context's default gives complete
  text and the wrong meaning.
- **`String.format("%02X", b)` is upper-case hex.** A routine named `generateMD5` is not `lower(md5(x))`. Read the
  body.
- **Talend's `Numeric.sequence` counts calls.** Two calls to one name in a row get two numbers.
- **A component-framework Row** (`tSnowflakeRow`) keeps its statement in `PROPERTIES.query.storedValue`, and has
  no `manualQuery` setting.
- **sqlparser in a debug build overflows the stack** on a long `||` chain, so parse such text on a thread with a big
  stack.
- **Count before writing a number down.** The design said 13 watermark queries; the test found 12. Two of the 14
  watermark jobs never read their variable back.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0267-talend-expressions-and-lowering.md` | Phase 2 design (before code) and result |
| `ekos/crates/talend/src/job.rs` | A Row's query lifted into `QUERY`; test |
| `ekos/crates/recovery/src/talend_analyzer.rs` | Talend pass v7 |
| `TODO.md`, `CLAUDE.md` | Phase 2 ticked, later items listed; devlog pointer |
| ekos-lakehouse (`dcfdcff`) | `talend-rel` `sql` / `routine` / lowering, `etl-rel` `Op::Statement`, tests; lakehouse devlog_26 |
