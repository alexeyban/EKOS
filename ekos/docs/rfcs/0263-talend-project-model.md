# RFC 0263 — The Talend project model

**Status:** Accepted (2026-11-20)
**Umbrella:** RFC 0262
**Crate:** `ekos-talend` (`ekos/crates/talend`), pure: no I/O, no async, `roxmltree` + `serde`

## What a project is

Verified on fixture B (Talend Open Studio 8.0.1) unless marked *(doc)*, which means "from Talend's documentation,
not yet seen in a real project".

| Path (relative to the project root) | Read as |
|---|---|
| `talend.project` | label, technical label, product version |
| `process/**/<Name>_<ver>.item` | one job version |
| `process/**/<Name>_<ver>.properties` | the job's id, label, version, folder (`ItemState/@path`), created/modified dates |
| `context/**/*.item` | a context group set: `context` elements, each with `contextParameter`s |
| `metadata/connections/*.item` | a database connection: engine, host, port, database, schema (values often `context.*`) |
| `metadata/genericSchema/*.item`, other `metadata/**` | repository schemas: name and columns |
| `code/routines/**/*.item` | a routine; `system/` routines are Talend's own and are counted apart |
| `joblets/**/<Name>_<ver>.item` + `.properties` *(doc)* | one joblet version (see [Joblets](#joblets-amendment-2026-11-21)) |
| `process_mr/`, `process_storm/`, `services/`, routes *(doc)* | recognised and counted as out of scope |

The input to the model is a list of `(relative path, text)` pairs for **one** project. Finding projects and reading
files is the observer's job.

## Job items

A job item is `talendfile:ProcessType`:

- `context` elements (one per context group of the job) with `contextParameter{name, type, value, prompt,
  comment, repositoryContextId}`; the root's `defaultContext` names the default group.
- `node` elements, one per component: `componentName` (the type), `componentVersion`, and `elementParameter{field,
  name, value}` children. `UNIQUE_NAME` is the component's name in the job. A parameter whose `field` is `TABLE`
  holds rows as a flat list of `elementValue{elementRef, value}`; a row ends when an `elementRef` already seen in
  the current row comes again. `metadata{connector, name, label}` elements carry the output schemas
  (`column{name, type, key, nullable, length, precision, pattern, comment, sourceType}`). A tMap carries
  `nodeData` (below).
- `connection` elements: `connectorName`, `source`, `target`, `label`, `metaname`, `lineStyle`. Row links are
  `FLOW` (a lookup when `lineStyle="8"`), `FILTER`, `REJECT`, `UNIQUE`, `DUPLICATE`, `ITERATE`; triggers are
  `COMPONENT_OK`, `COMPONENT_ERROR`, `SUBJOB_OK`, `SUBJOB_ERROR`, `RUN_IF` (condition in the `CONDITION` element
  parameter).
- `subjob` elements: kept as counts only in v1.

Component types repeat their unique names across jobs (`tMap_1` everywhere), and a unique name does not say the
type (`tDBInput_1` is a `tPostgresqlInput`): identity is project → job → version → unique name.

### tMap

`nodeData` (`TalendMapper:MapperData`) has:

- `inputTables{name, matchingMode, lookupMode, innerJoin, activateExpressionFilter, expressionFilter}` with
  `mapperTableEntries{name, type, expression, operator}`. The first input is the main flow; the others are
  lookups. A lookup entry with an expression is a join key: `lookup.column = <expression over earlier inputs>`.
  Input names are the labels of the incoming row links (`row1`, `row2`).
- `varTables` with entries `{name, type, expression}`.
- `outputTables{name, reject, rejectInnerJoin, activateExpressionFilter, expressionFilter}` with entries
  `{name, type, expression}`. An output's name is the `metaname` of the outgoing link that carries it.

## Component catalogue

Each type maps to a category:
`input`, `output`, `row_transform`, `set_transform`, `join`, `lookup`, `flow`, `code`, `elt`, `orchestration`,
`connection`, `file`, `logging`, `unknown`. Engine-specific names normalise to a logical component plus an engine:
`tPostgresqlInput` → `db_input` on `postgresql`; the generic `tDBInput` takes its engine from its `TYPE`
parameter. An unknown type is kept with its parameters, never an error.

## Resolution

- **Context.** `context.X` resolves from the job's default group. With implicit context load on in the project
  settings, or a `tContextLoad` in the job, or an assignment to `context.X` in a `tJava`, a resolved value carries
  `overridable_at_runtime = true`.
- **Java strings.** Parameters holding a Java string expression (SQL, table names, file names) are folded:
  string literals (with Java escapes), `+`, and resolved `context.*`. The result is `Folded { text, holes }`: a
  hole is an operand that is not a literal or a resolved context variable (`globalMap.get(...)`, a method call),
  written into the text as `{{hole}}` and listed. A fully folded value has no holes. *Amendment (2026-11-21):*
  `globalMap.get("k")` (cast or not) folds to the value every `tSetGlobalVar` of the job gives `k`, when that
  value is one constant (a literal, or one that folds completely over the default context); a key set to two
  values stays a hole (`Job::global_constants`, `Folded::globals_used`). A file name read from a `tFileList`'s
  current file is named, for lineage, by the list's folded directory joined to each mask. A parameter that is not a
  Java expression at all is returned as `NotJava`.
- **Calls.** `tRunJob` gives `{child name, child id, version ("Latest" resolves to the highest version present,
  and says so), context, transmit whole context, die on child error}`.
- **Passwords.** The model never stores a value from a `PASSWORD` element parameter, an `id_Password` context
  parameter or a `password` attribute: it records only that one is set.

## Joblets (amendment, 2026-11-21)

*(doc)* when written; checked against fixtures C and D below. The shape below is taken from Talend's own
EMF model (`joblet.ecore`: `JobletProcess` extends `ProcessType`, `jobletNodes` are `NodeType`s) and from two
independent open-source readers of Studio files. Every rule here is checked again against the first
Studio-written joblet, and a difference is a bug in this section, not in the project.

- **The item.** `joblets/**/<Name>_<ver>.item`, root `model:JobletProcess`, with the same `context`, `node`,
  `connection` and `nodeData` children as a job. Its boundary ports may be written as `jobletNodes` instead of
  `node`; both are read. The `.properties` file (`JobletProcessItem`) gives id, label, version and folder, as for a
  job. A joblet is read into the same `Job` structure and kept in `Project::joblets`, all versions, latest marked.
- **Ports.** Components `INPUT`, `OUTPUT` (Studio's names; `tJobletInput` / `tJobletOutput` also accepted) are row
  ports: category `flow`, logical `joblet_input` / `joblet_output`, pass-through. `TRIGGER_INPUT`,
  `TRIGGER_OUTPUT` (and `tJobletTrigger*`) are trigger ports, category `orchestration`.
- **A use.** In a job (or another joblet), a `node` whose `componentName` is the name of a joblet in the project is
  a joblet use: category `joblet`, logical `joblet`. It resolves to the latest version, and says so (`via_latest`),
  unless the node names a version in `PROCESS_TYPE_VERSION`.
- **Expansion.** `Project::expand(&job)` returns the job with every use inlined, as Studio does at code
  generation: each joblet component is copied in as `<use>__<component>` (a nested joblet's as
  `<use>__<inner use>__<component>`), its internal links copied with both ends renamed, link labels kept, and
  a parameter whose whole value names one of the joblet's components (`LIST`, `CONNECTION`) renamed with it.
  Ports are not copied: a link into the use is joined to the joblet's links out of the input port it enters (the
  only one, else the port its connector or `metaname` names), and a link out of the use to the joblet's links
  into the output port it leaves (the port its connector or `metaname` names, else the only one). A joined link
  takes its source end (`from`, `metaname`) from the upstream link and its target end (`to`, `label`) from the
  downstream one, so every tMap still finds its inputs by the labels it was written with. A port that cannot be
  chosen is diagnostic `TALEND005` and the link is dropped from the expanded job, never guessed.
  Joblet context variables the job does not define are added to the job's default group. Nesting deeper than 8,
  or a joblet that uses itself, is `TALEND006` and that use is left unexpanded. Each expanded component records
  `joblet: {name, version, use}`.
- **Labels.** Because two inlined copies keep the same link labels, a label is resolved only among the links
  into the component that reads it (`(target, label)`), never project-wide.

The un-expanded job is what the ledger's `TalendComponent` objects describe (a use is one component); the
expanded job is what the Transformation IR and column lineage are built from (RFC 0264).

### Checked against Studio-written joblets (amendment, 2026-11-21)

Fixtures C (`KompallyAkhil/Talend2`, 9 joblets, 4 uses) and D (`singhSurajsehwag/Talend_SQL`, 5 joblets, a
two-port joblet used by its second port) are joblets as Studio 8 writes them. Three rules above were wrong or
missing; the rest held:

- **The envelope.** A joblet item is an `xmi:XMI` document holding `model:JobletProcess`, its icon
  (`TalendProperties:ByteArray`) and sometimes a `Signature` — not `JobletProcess` as the root. Every one of the
  23 joblet items in the candidates read had it; the reader takes the process out of the envelope.
- **Links named after ports.** A caller's link out of a joblet use carries the port's name as its connector
  (`connectorName="OUTPUT_2"`, `metaname="OUTPUT_2"`, `TRIGGER_OUTPUT_1`), so only its line style says what it
  is: 8 is a lookup, 1/3/4/5/6 are `SUBJOB_OK`/`COMPONENT_OK`/`SUBJOB_ERROR`/`COMPONENT_ERROR`/`RUN_IF` (measured
  over 4,000 links of fixture B and the candidates). Such a trigger keeps the port as `Trigger::port`.
- **Triggers through trigger ports.** A link into a use never names the use's input port (Studio writes the
  *source's* port as the connector); it must be the only one. A trigger into a use fires what the joblet's
  `TRIGGER_INPUT` fires; a trigger out of a use is fired by what fires the `TRIGGER_OUTPUT` its connector names
  (else the only one). Unmatched: `TALEND005`, dropped. Before this, a trigger between two trigger joblets was a
  row link and `TALEND005`.

`PROCESS_TYPE_VERSION` was absent from every use read: uses resolve to the latest version, as above.

### Component-framework settings (amendment, 2026-11-21)

Components of Talend's component framework (`tSnowflake*`, cloud storage, …) keep their settings as JSON in one
`PROPERTIES` parameter (Daikon serialization: each setting `{…, "storedValue": …, …, "name": …}`). The reader lifts
`table.tableName`, `connection.schemaName` and `outputAction` into `TABLE`, `SCHEMA_DB` and `DATA_ACTION` when the
component has none, and `query` into `QUERY` when `manualQuery` is on (stored as `true` or as
`{"@type": "boolean", "value": true}`; fixture E's 14 Snowflake inputs), and keeps the JSON with the `storedValue` of every setting named like a secret (`pass`,
`secret`, `token`, `apikey`, `privatekey`) set to null. Before the parse, `ekos build`'s redaction removes those
values from the text (RFC 0043, `json-secret-property`): the regex rules could not tie a value to the name written
after it, and one candidate project held a plain-text password there.

## ELT maps (amendment, 2026-11-21)

An ELT job does not move rows through Java: `tELTInput` names a table, `tELTMap` writes a SELECT over its inputs
and `tELTOutput` runs it into a table (`INSERT … SELECT`, `DELETE … WHERE`) inside the database. Fixture E shows the
item shape:

- `tELTMap`'s `nodeData` is `TalendDBMap:DBMapData`: `InputTables` (`name`, `alias`, `tableName`, `joinType`
  `NO_JOIN` | `INNER_JOIN` | `LEFT_OUTER_JOIN` | …, entries with `join="true"`, `operator` and the join
  `expression`) and `OutputTables` (`name`, `tableName`, entries with an SQL `expression`, `FilterEntries` with
  `filterKind` `WHERE_FILTER` | `OTHER_FILTER` — the second holds `GROUP BY …` text).
- Links between ELT components are `connectorName="TABLE"`, line style 9. A link into the map carries the input's
  `tableName` as its `label`; a link out carries the output's `name` as its `metaname`.
- `tELTInput` / `tELTOutput` name their table in `ELT_TABLE_NAME` / `ELT_SCHEMA_NAME` (Java strings, folded);
  `tELTOutput` has `DATA_ACTION` and, with `USE_DIFFERENT_TABLE`, writes `DIFFERENT_TABLE_NAME` instead.
- The engine of a generic ELT component is its `MAPPING` (`snowflake_id` → `snowflake`); an engine form
  (`tELTOracleMap`) names it in the type.

The model keeps the map as `Component.elt` (`EltMap`: inputs with alias, table, join type and join keys; outputs
with table, entries, WHERE filters and other clauses) beside `mapper`, never inside it: expressions are SQL, not
Java, and the translation path must not read one as the other. A `TABLE` link is `LinkKind::Table`.

## Diagnostics

A file that does not parse is a diagnostic with its path; the rest of the project is still read. Every diagnostic
has a code (`TALEND001`…) listed in the crate docs. `TALEND005` (a joblet port that cannot be chosen) and
`TALEND006` (joblet nesting too deep or cyclic) come from expansion.

## Exit (gate for Slice 1's reader)

On fixture B: 14 jobs, 272 components, 232 links, the per-type census exactly; 11 `tRunJob` calls resolved to jobs
present; no encrypted string anywhere in the serialized model.
