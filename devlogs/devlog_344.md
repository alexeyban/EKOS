# Devlog 344 — Talend column lineage and joblets (RFC 0264 Phase 2)

**Date:** 2026-11-21
**PRs:** none (pushed to `main` after local gates)
**Branch:** main

---

## Summary

Every column a Talend job writes now has a lineage: the steps back to where its value came from, the origin
columns, a formula over them and any component the walk could not pass, named. Lineage crosses jobs through the
tables one job writes and another reads, back to the first source files. Joblets are read and inlined into each
job's Transformation IR and lineage, as Studio does at code generation. On fixture B (a real Talend Open Studio
8.0.1 project): 368 target columns, 368 lineages, 0 gaps, 107 links between jobs, and the plan's acceptance
examples hold — `FAIT_VENTE.NB_MT_VENTE = sum(ODS_VENTE.QTE_VENTE*ODS_VENTE.PRIX_VENTE)` back to
`ICOM_*_VENTE.csv`, and `NB_QTE_VENTE = count(DIM_PRODUIT.ID_DIM_PRODUIT)` (the probable defect: the quantity
measure counts product keys). No model calls.

---

## Column lineage

### Problem / motivation
Phase 1 recorded, per tMap expression, the `input.column` pairs it reads. That answers "what does this one step
read", not "where does `FAIT_VENTE.NB_MT_VENTE` come from": the answer runs through two tMaps, an aggregate, an
in-memory `tHashOutput` → `tHashInput` hand-over, a SQL select list, and another job that loaded the ODS table
from CSV files.

### What was built
| Piece | Where |
|---|---|
| `lineage::job_lineage` — per target column (database output, file output, `tBufferOutput`): steps, origins, gaps, formula | `crates/talend/src/lineage.rs` |
| `QueryReader` trait; `SqlQueries` reads a SELECT's projection item by item and its WHERE (sqlparser, PostgreSQL then generic) | `talend/src/lineage.rs`, `recovery/src/talend_lineage.rs` |
| `link_jobs` — lineages linked across the latest job versions through tables (`ColumnFeeds`), `upstream` origins transitively | `recovery/src/talend_lineage.rs` |
| `TalendColumnLineage` objects (`Contains` from the target component), searchable `description` | `recovery/src/talend_analyzer.rs` |
| `globalMap.get("k")` folds to a `tSetGlobalVar` constant; a `tFileList`'s current file is named by directory + mask | `talend/src/java.rs`, `resolve.rs`, `lineage.rs` |
| `replace_qualified` (substitute `input.column` references outside literals) | `talend/src/java.rs` |

### Implementation details worth remembering
- A column is addressed `(component, output, column)`; `output` is the tMap output a link's `metaname` names.
  Rules per component: tMap (expression, `Var.x` inlined, inner joins and the lookups an expression reads as
  conditions, the output filter), aggregate (`function(input)`, group-by pass-through), `tJavaRow`
  (`output_row.X = …;`), pass-through by name (filter, uniq, sort, replicate, unite, convert, log, hash/buffer
  output, joblet ports), `tHashInput` from the `tHashOutput` its `LIST` names. Anything else is a gap.
- A database input's column is matched to the select item at the same position, because that is how Talend binds
  a query's result to the schema; without a parsed query, by name, marked `by_name: true`.
- An inner join drops rows for **every** output column, so it is a condition on all of them; a left join only on
  the columns that read the lookup.
- The formula substitutes each reference by the upstream formula, parenthesised unless atomic, and drops one pair
  of parentheses around a whole substituted expression (`sum((a*b))` → `sum(a*b)`).
- Cross-job links use only the latest job versions, skip a job reading its own output, ignore `UPDATE` key
  columns as writers (they match rows, they do not write values), and compare names as `data_lineage::name_key`
  does with the schema rule "equal, or missing on one side".

### Decisions
- **One object per target column**, not column objects with edges: columns are properties of `Table` in the
  ledger, and a self-contained lineage object answers a question in one read (`ekos_search` finds it by its
  `description`). `ColumnFeeds` keeps the graph walkable.
- **SQL parsing stays out of `ekos-talend`** (the crate is `roxmltree` + `serde`): the `QueryReader` trait lets
  `recovery` supply sqlparser, and the crate's own tests use a stub.
- **`globalMap` constants only when every `tSetGlobalVar` agrees.** Which value a reader sees otherwise depends on
  the run; a hole is honest.

---

## Joblets

### Problem / motivation
`joblets/**` was counted as out of scope. A job that uses a joblet had an `unknown` component in the middle of
its flow, so its IR and any lineage stopped there.

### What was built
- `Role::Joblet`; `parse_job` accepts `model:JobletProcess` and reads `jobletNodes` as well as `node`; joblets go
  to `Project::joblets`, all versions, latest marked. Ports `INPUT`/`OUTPUT` (and `tJobletInput`/`tJobletOutput`)
  are `joblet_input`/`joblet_output`; a component whose type is a joblet's name is category `joblet`.
- `Project::expand(&job)`: every use inlined, components renamed `<use>__<name>`, internal links copied, a
  parameter whose whole value names an inner component (`LIST`, `CONNECTION`) renamed; **ports elided** by
  joining links — source end from the upstream link, target end (and label) from the downstream one; nested
  joblets recursively; `TALEND005` (port not choosable), `TALEND006` (cycle or depth > 8).
- Ledger: `TalendJoblet`, its components keyed `joblet:<name>@<version>:<unique>`, `UsesJoblet` from the use.
  IR and lineage are built from the expanded job; `TalendComponent`s describe the un-expanded one.

### Decisions
- **Elide ports instead of a pass-through IR node**: the Transformation IR has no pass-through node, and adding
  one would reach every IR consumer (dbt generation, docs, explain).
- **Labels resolved per reader.** Two inlined copies of a joblet keep the same link labels, so the IR's
  label → source map is now keyed `(reading component, label)`; a project-wide map would silently cross wires.
- **No fixture holds a joblet.** The format comes from Talend's EMF model (`joblet.ecore`) and two independent
  open-source readers (read for the format only, nothing copied). Tests are synthetic, and the RFC marks the
  section *(doc)* until fixture C.

---

## Knowledge Captured

- **Talend binds a query's result to the component schema by position**, not by name — the select list of
  `tDBInput` must be read item by item to know which table column a schema column is.
- **Fixture B's file names are `tFileList` + `tSetGlobalVar`:** `FILENAME` is
  `globalMap.get("tFileList_1_CURRENT_FILEPATH")`, the list's mask is `globalMap.get("masque_fichier")`, and a
  `tSetGlobalVar` sets that key to `"ICOM_*_VENTE.csv"`. Without folding `globalMap` constants the lineage ended
  in an unnamed hole.
- **A cached Talend pass prints no `Talend:` line** (its stats are only filled when it runs) — same as the other
  passes' summary counters; re-run on a fresh workspace to see the numbers.
- **`ekos resolve` exits 1 on fixture B** (14 `JavaType` vs `TalendJob` same-name conflicts) because the Java
  observer reads the generated code under `poms/`; use `--force` until that exclusion lands (TODO).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/talend/src/lineage.rs` | new: column lineage per target column, `QueryReader`, rules, formula |
| `ekos/crates/talend/src/joblet.rs` | new: joblet uses, `Project::expand`, `TALEND005`/`006` |
| `ekos/crates/talend/src/{lib,job,model,catalogue,xml,java,resolve}.rs` | `Role::Joblet`, `JobletProcess`/`jobletNodes`, `Project::joblets`, `Component::joblet`, `Column::db_name`, joblet ports and `Category::Joblet`, `replace_qualified`, `global_ref` + `Job::global_constants` |
| `ekos/crates/talend/tests/fixture_b.rs` | lineage acceptance examples on fixture B |
| `ekos/crates/recovery/src/talend_lineage.rs` | new: `SqlQueries`, `link_jobs` |
| `ekos/crates/recovery/src/talend_analyzer.rs` | `TalendJoblet`, joblet components, `UsesJoblet`, IR from the expanded job, `TalendColumnLineage`, `ColumnFeeds`, stats; pass `v2` |
| `ekos/crates/recovery/src/talend_ir.rs` | labels resolved per reader, joblet origin on nodes, `tHashInput` linked to the output its `LIST` names |
| `ekos/crates/kir/src/custom_kinds.rs` | `TalendJoblet`, `TalendColumnLineage` (keyed) |
| `ekos/crates/common/src/lib.rs` | `PIPELINE_LOGIC_VERSION` 8 (the observer now captures joblet items) |
| `ekos/crates/cli/src/commands/recover.rs` | the `Talend:` line counts joblets, lineages, gaps, cross-job links |
| `ekos/docs/rfcs/0263-…`, `0264-…` | joblets amendment; Phase 2 design and result; `globalMap` constants |
| `CLAUDE.md`, `TODO.md`, `CHANGELOG.md`, `README.md`, `docs/generated/ekos-self-documentation.html` | updated |
