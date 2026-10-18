# RFC 0201 — The deep Kettle model: packages, steps, variables, links

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-18
**Builds on:** RFC 0200 (umbrella), RFC 0043 (redaction), RFC 0149/0191 (extension seam)
**Code:** `ekos-lakehouse` — `crates/kettle` (pure), the reader in `crates/lakehouse-ext`

## Goal

Read a whole Pentaho Data Integration package into one typed model that the translator
(RFC 0202–0204) can lower without going back to XML: every transformation and job, every step
with its configuration, the hops between them, the connections they use, the variables they
reference, and how transformations and jobs call each other.

## What a package is

A directory (a project's source) containing any of:

| File | Read as |
|---|---|
| `*.ktr` | one transformation |
| `*.kjb` | one job |
| repository export (`<repository>` root, any `.xml`) | every `<transformation>` and `<job>` inside, each at a virtual path `<directory>/<name>.ktr` / `.kjb` |
| `shared.xml` | shared connections, used when a file names a connection it does not define |
| `kettle.properties` | variable values |
| `plugins/**/plugin.xml`, `plugins/**/*.jar` | plugin step ids → class names and jar paths (the jar bytes are read only by a `JavaDecompiler`, RFC 0203) |

Anything else is ignored. A file that does not parse becomes a diagnostic; the rest of the
package is still read (no whole-package abort — the failure shape of `sqlparser` on whole files,
RFC 0176, is not repeated).

## Secrets

The reader is a raw-content entry point. It applies `ekos_common::redaction::redact` to every
file before parsing; RFC 0200 added two built-in patterns for Kettle's reversible
`Encrypted <hex>` passwords and XML password elements. The model then drops every element whose
name contains `password`/`passwd`, so a password can never be read back from it whatever the
redaction table says. Usernames, hosts and database names are kept: the translator needs them to
match a connection to a lakehouse source.

## The model

- `Package { transformations, jobs, shared_connections, properties, plugins, diagnostics }`.
- `Transformation { path, name, description, directory, parameters, connections, steps, hops,
  error_handling }`; `Job { path, name, parameters, connections, entries, hops }`.
- `Step { name, type_id, kind, copies, distribute, xml, line }` — `xml` is an owned element tree
  (element names, attributes, text, children, source line), so every later stage reads typed
  values from it without re-parsing. `kind` places the type id in the step catalogue.
- `Hop { from, to, enabled, error }`, from `<order>` and `<step_error_handling>`; job hops carry
  `evaluation` (success/failure) and `unconditional`.
- `Connection { name, type, access, host, database, port, username, schema? }`.
- `Parameter { name, default, description }`.

### Step catalogue

Each known type id maps to a category used by the medallion split and by coverage reports:
`input`, `row_transform` (row-local), `set_transform` (combines rows: sort, group, window, dedupe),
`join`, `lookup`, `output`, `flow` (filter, switch, append, dummy, abort), `code` (JavaScript, Java
class, Java expression, Java filter, formula), `sub_transformation` (Mapping, Simple Mapping,
Transformation/Job Executor, Single Threader, Metadata Injection), `file` (text/Excel/XML/JSON
input and output), `orchestration` (variables, rows to/from result, log, delay), or `unknown` (a
plugin or an id not in the catalogue). `unknown` is not a parse error: the step is kept with its
XML and becomes a gap at translation.

### Variables

`${NAME}` and `%%NAME%%` are resolved, in order, from: the transformation's or job's own parameter
defaults, `kettle.properties`, and the internal variables (`Internal.Transformation.Filename.Directory`,
`Internal.Job.Filename.Directory`, `Internal.Entry.Current.Directory`, `Internal.Transformation.Name`,
`Internal.Job.Name`), which are the file's own package-relative directory and name. Each
resolution records where the value came from; an unresolved name is kept in the text and listed,
never guessed. Values set at run time (Set Variables steps and entries, command-line `-param`)
are listed as run-time variables: their value is unknown to a static reader.

### Links

A `TRANS`/`JOB` job entry, and a Mapping, Simple Mapping, Transformation Executor, Job Executor or
Single Threader step, names another file by filename (after variable resolution, relative to the
package root) or by repository directory and name. Each becomes a `Link { from, to, via }`
resolved against the package; an unresolved target is a diagnostic. Cycles are detected and
reported (Kettle allows a job to call itself; a translation cannot).

### Coverage

`Package::coverage()` counts steps per type id and category and job entries per type — the
baseline every later milestone reports against.

## Measured

On Pentaho's own samples (pentaho-kettle `assemblies/samples`, 188 transformations and 19 jobs,
970 steps across 100 step types) and the AdventureWorks corpus in
`test-runs/run-20260831T212115Z/workspace/` (8 transformations, 2 jobs, MSSQL → MySQL); numbers
are in the lakehouse devlog that ships this RFC.

## Non-goals

- Typing each step's configuration and expressions (RFC 0202).
- Reading plugin jar contents (RFC 0203, through `JavaDecompiler`).
- Writing to the ledger (RFC 0204 writes `Pentaho*` facts once a translation exists).
