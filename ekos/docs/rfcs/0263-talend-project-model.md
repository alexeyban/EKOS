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
| `joblets/**` *(doc)* | joblets |
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
  written into the text as `{{hole}}` and listed. A fully folded value has no holes. A parameter that is not a
  Java expression at all is returned as `NotJava`.
- **Calls.** `tRunJob` gives `{child name, child id, version ("Latest" resolves to the highest version present,
  and says so), context, transmit whole context, die on child error}`.
- **Passwords.** The model never stores a value from a `PASSWORD` element parameter, an `id_Password` context
  parameter or a `password` attribute: it records only that one is set.

## Diagnostics

A file that does not parse is a diagnostic with its path; the rest of the project is still read. Every diagnostic
has a code (`TALEND001`…) listed in the crate docs.

## Exit (gate for Slice 1's reader)

On fixture B: 14 jobs, 272 components, 232 links, the per-type census exactly; 11 `tRunJob` calls resolved to jobs
present; no encrypted string anywhere in the serialized model.
