# Devlog 349 — Talend Slice 3, phase 1: jobs as the shared relational IR (RFC 0267)

**Date:** 2026-11-21
**PRs:** none (pushed to `main` in both repositories)
**Branch:** main (EKOS `89cedb25`); main (ekos-lakehouse `28dee30`, lakehouse devlog_25)

---

## Summary

RFC 0267's first delivery turns a Talend job, with its joblets inlined, into the relational IR that Pentaho's dbt
printer already reads (RFC 0266). The new crate is `talend-rel`, in the extension. It lowers Talend's Java
expressions with Talend's own rules, which are plain Java's, not Kettle's, and turns components into nodes.

Coverage of data components on the real fixtures: B 93%, C 87%, D 62%, E 73%. The rest are gaps with their
text.

In EKOS, the reader now lifts a component-framework input's manual query into `QUERY`. Fixture E's 14 Snowflake
inputs had neither a query nor a table before this.

---

## RFC 0267 — expressions and the component lowering

### What was built

| Piece | Where |
|---|---|
| RFC 0267 with the measured result and amendments | `ekos/docs/rfcs/0267-talend-expressions-and-lowering.md` |
| Manual query → `QUERY` (`framework_params`), test, Talend pass v6 | `ekos/crates/talend/src/job.rs`, `recovery/src/talend_analyzer.rs` |
| `talend-rel` (`expr`, `lower`, fixture and purity tests) | ekos-lakehouse `crates/talend-rel` |

### Implementation details worth remembering
- **Name resolution.**
  - A tMap renames every input's columns to `<input>__<column>`, so an expression's `row1.ID` and `row2.ID`
    stay distinct through lookups.
  - Expressions name inputs by link label.
  - `Var.x` inlines the variable's own expression.
  - `context.x` becomes its default value, `rewritten`, because a run may replace it.
- **Lookups.** `UNIQUE_MATCH` and `FIRST_MATCH` lookups are `Lookup` nodes, `rewritten` until a key is proven
  unique. Talend's documentation says `UNIQUE_MATCH` keeps the last match; that still has to be checked against
  fixture B's generated Java.
- **Reject outputs.** An inner-join reject output reads the all-left chain and keeps rows whose inner keys are
  null. A reject output keeps rows no other output's filter accepted.
- **SQL statements stay visible.** A SQL statement component (`tDBRow`, `tSnowflakeRow`) has no row link but
  changes data. It must never fall into the "disconnected" bucket; doing so first inflated fixture E to a false
  86%.

### Decisions
- **`ekos-talend` is pinned by `rev`** in the lakehouse rather than moving every EKOS crate to the current main.
  It depends on no other EKOS crate, and the rest of the lakehouse stays on its tested revision.
- **A file input reads a landed table** (`files."<name>"`), `rewritten`. Whether that becomes a seed or a source
  is RFC 0268's choice.

---

## Results

| Fixture | Data components lowered | Expressions lowered / gaps | Main remaining gaps |
|---|---|---|---|
| B | 116 / 125 (93%) | 475 / 11 | `Numeric.sequence` ×7 |
| C | 79 / 91 (87%) | 144 / 6 | custom `HashUtil` routines |
| D | 26 / 42 (62%) | 40 / 2 | `tNormalize`, `tSampleRow`, `tMysqlSCD`, tLogRow fed by code |
| E | 199 / 274 (73%) | 337 / 12 | ELT maps ×21, `tSnowflakeRow` ×42, `HashUtils.generateMD5` ×11 |

The Pentaho golden manifests are still byte-identical, the lakehouse workspace tests and clippy pass, and the
EKOS Talend tests pass.

---

## Knowledge Captured

- **Two revisions of one repository.** Cargo resolves a dependency pinned by `rev` as a separate source from the
  same repository's `branch = "main"` entries.
- **Completing a commit hash by hand fails with a "network error".** Always read the full hash with
  `git rev-parse`.
- **Component-framework flags.** A boolean setting can be stored as `true` or as
  `{"@type": "boolean", "value": true}`; read both.
- **Lowering gaps measure what is honestly unknown.** On fixture B, the first run's 106 expression gaps were
  mostly `jobName` and `globalMap` values the job itself determines. After resolving those, 11 remain, all real:
  row order, locale, randomness, and routines outside the catalogue.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0267-talend-expressions-and-lowering.md` | new RFC, with the result |
| `ekos/docs/rfcs/0263-talend-project-model.md` | the manual-query lift |
| `ekos/crates/talend/src/job.rs`, `ekos/crates/recovery/src/talend_analyzer.rs` | manual query, pass v6 (EKOS `89cedb25`) |
| `CLAUDE.md`, `TODO.md` | RFC pointer, the lakehouse pin, TODO |
| ekos-lakehouse (its devlog_25) | `crates/talend-rel` |
