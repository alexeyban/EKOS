# RFC 0264 — Talend in the ledger: objects, relationships, Transformation IR, column references

**Status:** Accepted (2026-11-20); Phase 1 shipped 2026-11-21; Phase 2 (column lineage, joblets) amended 2026-11-21
**Umbrella:** RFC 0262 · **Model:** RFC 0263 · **Plan:** Part II

## Pass

`recovery/src/talend_analyzer.rs::TalendAnalyzerPass` reads every `talend` artifact, groups them by
`project_root`, rebuilds each project's file list and calls `ekos_talend::read_project`. Pure and
deterministic: the same artifacts give byte-identical KIR. It writes one knowledge artifact for all projects,
with `supersedes_earlier_runs` set, because it reads "all of Talend" (a deleted job must leave).

`ekos recover` prints `Talend: N project(s), J job(s), C component(s), D of them data components, X% mapped,
F of Q quer(y/ies) folded` and the diagnostics count. No model calls.

## Objects

Every id is `uuid_v5("talend:<kind>:<project root>:<key>")`, so a re-read gives the same id and a component
named `tMap_1` in two jobs gives two objects.

| Kind | Key | Name | Main properties |
|---|---|---|---|
| `TalendProject` | — | technical label | label, product_version, counts, `not_read`, diagnostics |
| `TalendJob` | job name + version | `<job>` | version, latest, folder, path, job_type, default_context, implicit_context_load, purpose, description, created, modified |
| `TalendComponent` | job + version + unique name | `<job>.<unique name>` | component_type, category, logical, engine, label, line, path, parameters (secrets as `{"secret": true}`), schemas, tMap mapping; for database components the folded `table`, `schema`, `query` (`text`, `holes`, `context_used`, `complete`), `data_action`, `keys` |
| `TalendContext` | item name + version | item name | path, groups with variables (no secret value) |
| `TalendContextVariable` | context item + group + name | `<item>.<name>` | type, default value, secret |
| `TalendDbConnection` | name + version | name | engine, host, port, database, schema, username, context_id, has_password, tables (name + columns) |
| `TalendSchema` | path | name | family, tables |
| `TalendRoutine` | name + version (custom only) | name | path, methods |

Talend's own system routines and SQL templates are counted on the project (`not_read`), not ledgered. Each
object's evidence cites its item file and, for components, the line of the `node` element.

## Relationships

| Kind | From → to | Properties |
|---|---|---|
| `Contains` | project → job, context, connection, schema, routine; job → component; context → variable | — |
| `Calls` | job → job | component, requested_version, via_latest, context, transmit_whole_context, die_on_child_error, dynamic |
| `RowLink` (`Custom`) | component → component, one per row link | link kind (`main`, `lookup`, `filter`, `reject`, `unique`, `duplicate`, `iterate`), label, metaname |
| `Triggers` (`Custom`) | component → component, one per trigger | kind, condition |
| `UsesConnection` (`Custom`) | component → `TalendDbConnection` | from the repository property (`PROPERTY:REPOSITORY_PROPERTY_TYPE`) |
| `UsesContext` (`Custom`) | job → `TalendContextVariable` | for each `context.x` the job's parameters and expressions read whose variable comes from a repository context item; `overridable_at_runtime` |
| `UsesRoutine` (`Custom`) | job → `TalendRoutine` | from `<routine>.<method>(` calls in the job's parameters and expressions, never from the declared routine list |

A call whose child is not in the project, or a variable whose item is missing, is recorded on the source
object (`unresolved_calls`, `unresolved_context`), not dropped.

## Transformation IR (knowledge path)

One `TransformGraph` per job version (`source_kind = "talend-job"`, `source_path` = the item path), built only
from **data components** (category input, output, row/set transform, join, flow, elt). Orchestration,
connections, code and logging stay `TalendComponent` objects with their triggers; they are not IR nodes, so the
mapped percentage is over data components.

| Talend | IR |
|---|---|
| database input | `Source { object_name: table }`, `schema`, folded `query` in properties |
| file input | `Source { object_name: folded file name }` |
| `tRowGenerator`, `tFixedFlowInput` | `Source { object_name: "<job>.<component>" }` (generated rows) |
| `tHashInput` / `tBufferInput` | `Source { object_name: "hash:<job>" }`, linked to the job's `tHashOutput` by name |
| database output | `Sink { object_name: table }`, `data_action`, `keys` |
| file output, `tHashOutput` | `Sink` (file name / `hash:<job>`) |
| `tFilterRow` | `Filter` (advanced condition, or the condition table written as `col op value` joined by the logical operator) |
| `tMap` | per lookup a `Join` (inner or left; keys `lookup.col = expression`); per output with a filter a `Filter`; per output column a `Calculate` (`output.column = expression`); a reject output carries `reject` / `reject_inner_join` |
| `tAggregateRow` | `Aggregate` (group by, operations) |
| `tUniqRow` | `Filter` with `unique on (keys)` and the component named |
| anything else among data components | `Unmapped` with the component type as the reason |

Edges follow row links; a tMap's incoming link enters its first node, and each output's nodes feed the link
whose `metaname` is that output. `data_lineage` (RFC 0075) links `Source`/`Sink` to `Table` objects at commit,
by unique name, as for every other IR source.

### Column references (Phase 1 of column lineage)

Every `Calculate`, `Filter` and `Join` node from a tMap records `column_refs`: the `input.column` pairs its
expression names (`row1.QTE_VENTE`), each resolved to the component that feeds that input. `Aggregate` records
its input columns. This is enough to walk lineage one component at a time.

## Phase 2 (amendment, 2026-11-21): column lineage and joblets

### Joblets in the ledger

| Kind | Key | Name | Main properties |
|---|---|---|---|
| `TalendJoblet` | `joblet:` + name + version | `<joblet>` | version, latest, folder, path, components, ports |

A joblet's components are `TalendComponent` objects keyed `joblet:<name>@<version>:<unique name>`, contained by
the joblet, with their `RowLink`s and `Triggers`. A use is `UsesJoblet` (`Custom`): the using component →
`TalendJoblet`, with `via_latest`. The IR of a job is built from the **expanded** job (RFC 0263 §Joblets), so a
job's graph runs through every joblet it uses; each node from a joblet carries `joblet: {name, version, use}`.
Data component counts and the mapped percentage are over the expanded job.

### Column lineage inside a job

`ekos_talend::lineage::job_lineage(&expanded_job, &dyn QueryReader) -> Vec<ColumnLineage>`, pure. For every
column of every **target** — a database output (`Table`), a file output (`File`), a `tBufferOutput` (`Buffer`,
rows handed to the calling job) — it walks back from the target column to where the value came from, one
component at a time. A column on a component is addressed as `(component, output, column)`; `output` is the tMap
output name, empty elsewhere.

| Component | How a column is derived |
|---|---|
| tMap | the output entry's expression; each `input.column` it names resolves through the link with that label **into this tMap**; `Var.x` is replaced by the variable's own expression and references; conditions: the output's filter, and the key expressions and join mode of every lookup the expression reads from |
| tAggregateRow / tAggregateSortedRow | an operation output → `function(input column)`; a group-by output → its input column; condition: `grouped by …` |
| tJavaRow | `output_row.X = <expr>;` statements of `CODE`, references `input_row.Y` |
| tFilterRow, tUniqRow, tSortRow, tReplicate, tUnite, tConvertType, tLogRow, tFlowMeter, tHashOutput, tBufferOutput, joblet ports | pass-through by column name from every row input; conditions: the filter, the unique keys |
| tHashInput | pass-through from the `tHashOutput` its `LIST` names (every one in the job when `LINK_WITH` is off) |
| database input | **origin** `Table`: when the folded query parses (`QueryReader`, sqlparser in `recovery`), the select item at the column's position — a plain column gives `table.column`, an expression gives its text and every column it names; else the component's table and the column's own name (`by_name: true`); the query's `WHERE` is a condition |
| file input | **origin** `File`: the folded file name, the column |
| tRowGenerator, tFixedFlowInput | **origin** `Generated`, with the value expression when the component states one |
| anything else | a **gap**: the component and its type; the walk stops there, nothing is invented |

Each `ColumnLineage` holds the target, the target component and data action (`update_key: true` for a key an
`UPDATE` matches on), the **steps** (each `{component, component_type, output, column, kind, expression, inputs,
conditions, joblet}`, deduplicated, target first), the **origins** and the **gaps**, plus a `formula`: the
target written over its origins by substituting each step's references (`sum(ODS_VENTE.QTE_VENTE*ODS_VENTE.PRIX_VENTE)`),
capped at 400 characters (`formula_truncated`). A cycle or a walk deeper than 64 steps stops with a gap.

### In the ledger

| Kind | Key | Name | Main properties |
|---|---|---|---|
| `TalendColumnLineage` | job + version + target component + column | `<target>.<column>` | job, job_version, latest, component, target `{kind, name, schema, column}`, data_action, update_key, formula, steps, origins, gaps, `upstream` origins across jobs, `description` (searchable: the target, the formula, the job) |

Relationships: the target `TalendComponent` `Contains` its lineages; `ColumnFeeds` (`Custom`) from a lineage
whose target is table column `T.c` to every lineage of another **latest** job version in the same project with
an origin `T.c` (names compared as `data_lineage::name_key` does: case, quotes and a schema missing on one side
ignored). `upstream` is the transitive set of origins across those links (cycle-guarded), so one object answers
"where does `FAIT_VENTE.NB_MT_VENTE` come from" back to the files. Lineage is computed for every job version;
links cross jobs only between latest versions.

### Exit (Phase 2)

On fixture B: every database and file output column has a `TalendColumnLineage`; the plan's acceptance examples
hold — `FAIT_VENTE.NB_MT_VENTE` has formula `sum(ODS_VENTE.QTE_VENTE*ODS_VENTE.PRIX_VENTE)` and its `upstream`
reaches the sales CSV columns; `FAIT_VENTE.NB_QTE_VENTE` shows `count(…ID_DIM_PRODUIT)`. Gaps are counted and
printed on the `Talend:` line. Joblets: synthetic tests in the shapes above, until fixture C. The 20-question
eval set stays open.

**Result (2026-11-21, devlog_344):** fixture B — 368 target columns, 368 lineages, 0 with a gap, 107
`ColumnFeeds` between jobs; `FAIT_VENTE.NB_MT_VENTE = sum(ODS_VENTE.QTE_VENTE*ODS_VENTE.PRIX_VENTE)` (the SQL
select lists parsed), `NB_QTE_VENTE = count(DIM_PRODUIT.ID_DIM_PRODUIT)`, and `ODS_VENTE.QTE_VENTE` back to
`ICOM_*_VENTE.csv` (the `tFileList` mask, set through `tSetGlobalVar`). Joblets: 0 in fixture B.

## Registry and guards

All ten kinds (`TalendJoblet` and `TalendColumnLineage` from Phase 2) are structurally keyed rows in `custom_kinds::REGISTRY`; `every_pipeline_custom_kind_is_registered`
covers them. Identity never merges them. `RowLink`, `Triggers`, `UsesConnection`, `UsesContext`, `UsesRoutine`,
`UsesJoblet`, `ColumnFeeds` are new `Custom` relationship kinds. Row links are not `FeedsInto`: that kind joins `TransformNode`s, and ten
consumers (dbt generation, docs, `transformation explain`) assume so; the IR's own `FeedsInto` edges are
unchanged.

## Exit (Phase 1)

On fixture B through `ekos build → recover → compile → commit`:
14 `TalendJob`, 272 `TalendComponent`, 11 `Calls`, 116 `RowLink`, 116 `Triggers` relationships; every
database input and output linked to its `Table` when the DDL beside the project defines it; 0 encrypted strings in
the ledger; 0 model calls.
