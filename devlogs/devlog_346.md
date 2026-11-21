# Devlog 346 — Talend ELT maps, fixture E, Studio's generated documentation

**Date:** 2026-11-21
**PRs:** none (pushed to `main`)
**Branch:** main

---

## Summary

The last Slice 1 item of the Talend plan was a fixture with ELT components and several database engines in
one project. A GitHub code search found one public project with generic ELT maps used for real:
`magrcrsdg/POC_DataCloudTalend`. It is a Talend Cloud 8.0.1 project with 66 jobs, using Snowflake, Oracle,
PostgreSQL, MySQL and SingleStore. It became fixture E.

Before this change, EKOS read every component but understood none of the ELT ones: only 35% of data components
were mapped, and an ELT target column got no lineage. ELT maps are now read, lowered into the Transformation IR and
traced.

The run also found that Studio's generated PDF documentation made a workspace with no schema look as if it had
one. That turned 136 honest coverage gaps into errors.

---

## Fixture E and the ELT model

### Problem / motivation
RFC 0262 §1.3 listed ELT as the one family of components none of fixtures A–D contains. An ELT job never moves
rows through Java:
- `tELTInput` names a table;
- `tELTMap` writes a SELECT over its inputs;
- `tELTOutput` runs `INSERT … SELECT` or `DELETE … WHERE` inside the database.

### What was built

| Piece | Change |
|---|---|
| `scripts/talend-fixtures.sh`, `scripts/talend-census.py` | fixture E pinned at `facebc7c`. The census counts ELT inputs, outputs, expressions, filters and join types (B, C and D unchanged) |
| `talend/src/model.rs` | `Component.elt: Option<EltMap>` (`EltInput`: alias, table, join type, join keys; `EltOutput`: table, entries, WHERE filters, other clauses). `LinkKind::Table` |
| `talend/src/job.rs` | `elt_map` reads `TalendDBMap:DBMapData`. A generic ELT component's engine comes from `MAPPING` (`snowflake_id` → `snowflake`) |
| `talend/src/lineage.rs` | `tELTOutput` is a table target; `elt_map` and `elt_input` rules; `elt_conditions`; `sql_qualifiers` |
| `recovery/src/talend_ir.rs` | `lower_elt_map`: a `Join` per joined input, a `Filter` for the WHERE filters, a `Calculate` per column (`clauses` kept as a property), `Source`/`Sink` for the ELT input and output. `data_io` names ELT tables |
| `observation-sdk`, `plugins/localdocs` | `is_generated_tool_output` also covers `documentations/generated/` beside a `talend.project`. The documents observer skips it. `PIPELINE_LOGIC_VERSION` 11 |
| RFC 0262 / 0263 / 0264 | fixture E; "ELT maps" model amendment; "ELT lineage" amendment |

### Implementation details worth remembering
- **How inputs bind.** A link into an ELT map is labelled with the input's `tableName` (`STG.DIM_MATERIAL`),
  while expressions name inputs by alias (`A`, or a table-like name such as `SUPPLY_WASTE`). An alias resolves to
  an input, and the input to the link with that label. This held for all 78 inputs in fixture E.
- **How outputs bind.** A link out of the map names the output in `metaname` (`table3`), as a tMap's does.
  `Walker::upstream` now takes output names from either kind of map.
- **References no input declares.** `qualified_refs` only finds qualifiers it is given, so `C.GONE` with no
  input `C` disappeared silently. `sql_qualifiers` lists every qualifier the SQL uses, so an alias no input
  declares becomes a gap with its text. The synthetic test caught this before any fixture did.
- **What counts as a condition.** Every join is a condition of an ELT step, unlike a tMap, where only inner
  joins and the lookups an expression reads are. In SQL, a left join can multiply rows too.
- **DELETE.** An ELT `DELETE` writes no value. Every column of its schema is a match key (`update_key`),
  whatever the key flags say.
- **Not handled yet.** Quoted identifiers (`"A"."X"`) are not read; fixture E has none.

### Decisions
- **`EltMap` is separate from `Mapper`.** Reusing `Mapper` would have saved code in lineage and the IR. But a
  tMap's expressions are Java and an ELT map's are SQL, and Slice 3's expression parser must never read one as
  the other.
- **The fixture's sensitive content stays out of public material.** Fixture E names a real database account (URL
  and user), so nothing from it is quoted beyond counts and table or column names.

---

## Studio's generated documentation

### Problem / motivation
On fixture E, `ekos check refs` reported 136 errors: every table the jobs name was unresolved. Fixture E has
no DDL, so each of these should be an info-level coverage gap ("no relations are compiled"). But the documents
observer had read the PDFs Studio renders under `documentations/generated/`, and their tables became `Table`
objects. xref then concluded that a schema existed.

### What was built
`documentations/generated/` beside a `talend.project` is generated tool output, like `poms/`, and the documents
observer skips it. Result: 136 errors → 136 info items. `documentations/` itself, where people keep their own
documents, is still read.

---

## Results (fixture E, fresh workspace, release build)

| | Before | After |
|---|---|---|
| Data components mapped | 35% | 85% |
| Column lineages | 334 | 911 (577 of them ELT), 0 with a gap |
| Links between jobs (`ColumnFeeds`) | 0 | 771 |
| `check refs` errors | 136 | 0 (136 info: no schema in the workspace) |
| Secrets in the sources / stored | 1,538 / — | 1,538 / 0 (planted control found) |

The reader's counts equal the census: 66 jobs, 458 components, 348 links, 116 `TABLE` links, and ELT maps with 78
inputs, 41 outputs and 723 expressions. 0 diagnostics, 0 model calls.

---

## Knowledge Captured

- **ELT links.** Studio writes links between ELT components as `connectorName="TABLE"`, `lineStyle="9"`. The
  link into a map carries the input's table name as its `label`, which is how an input is bound. The alias is
  never on the link.
- **`OTHER_FILTER` entries** on an ELT output hold free SQL (`GROUP BY …`, with CR/LF and tabs). They are kept
  whitespace-collapsed, as written, never parsed.
- **Generated documentation.** Talend Studio's `documentations/generated/` (HTML and PDF per job) is generated
  tool output. The documents observer turns PDF tables into `Table` objects, and any `Table` object makes xref
  believe a schema exists.
- **Running the audit script.** `python3 -I` drops the user site-packages, so `zstandard` (user-installed) is
  not found. Run the audit script with plain `python3` from its own directory.
- **Finding real projects.** `gh search code 'componentName="tELTMap"' --extension item` finds real
  projects using a component. Group the hits by repository to see which projects use it in many jobs.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/talend/src/{model,job,lineage}.rs` | ELT model, parsing, lineage, tests |
| `ekos/crates/talend/tests/fixture_elt.rs` | gated fixture E test: census equality, engines, 0 gaps |
| `ekos/crates/talend/tests/fixture_b.rs` | `LinkKind::Table` |
| `ekos/crates/recovery/src/talend_ir.rs` | ELT lowering, `data_io` |
| `ekos/crates/recovery/src/talend_analyzer.rs` | pass version v4; ELT test |
| `ekos/crates/observation-sdk/src/lib.rs` | `documentations/generated/`; misplaced doc comment fixed; test |
| `ekos/plugins/localdocs/src/lib.rs` | skips generated tool output |
| `ekos/crates/common/src/lib.rs` | `PIPELINE_LOGIC_VERSION` 11 |
| `ekos/docs/rfcs/0262…`, `0263…`, `0264…` | fixture E, ELT amendments |
| `scripts/talend-fixtures.sh`, `scripts/talend-census.py`, `evals/talend/census.json` | fixture E, ELT counts |
| `CLAUDE.md`, `TODO.md`, `README.md`, `CHANGELOG.md`, `docs/generated/ekos-self-documentation.html` | docs |
