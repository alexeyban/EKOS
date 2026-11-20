# RFC 0264 — Talend in the ledger: objects, relationships, Transformation IR, column references

**Status:** Accepted (2026-11-20); Phase 1 below is what Slice 1 builds first
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

**Phase 2** (next): lineage across components and jobs: an output column's chain back through tMap and
aggregate steps, across `tHashOutput` → `tHashInput`, and between jobs through the tables one writes and
another reads; plus the eval set of RFC 0262. Not built in Phase 1.

## Registry and guards

All eight kinds are structurally keyed rows in `custom_kinds::REGISTRY`; `every_pipeline_custom_kind_is_registered`
covers them. Identity never merges them. `RowLink`, `Triggers`, `UsesConnection`, `UsesContext`, `UsesRoutine`
are new `Custom` relationship kinds. Row links are not `FeedsInto`: that kind joins `TransformNode`s, and ten
consumers (dbt generation, docs, `transformation explain`) assume so; the IR's own `FeedsInto` edges are
unchanged.

## Exit (Phase 1)

On fixture B through `ekos build → recover → compile → commit`:
14 `TalendJob`, 272 `TalendComponent`, 11 `Calls`, 116 `RowLink`, 116 `Triggers` relationships; every
database input and output linked to its `Table` when the DDL beside the project defines it; 0 encrypted strings in
the ledger; 0 model calls.
