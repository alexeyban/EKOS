# RFC 0204 — Pentaho to silver and gold dbt models

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-18
**Builds on:** RFC 0200–0202, RFC 0193 (silver), RFC 0195 (gold)
**Code:** `ekos-lakehouse` — `crates/pentaho-dbt` (pure), `lakehouse pentaho translate|report`

## Goal

From a package's IR (RFC 0202) and the deployed lakehouse plan, write dbt-trino models into the
pipeline repository — silver for each transformation's cleansing, gold for its outputs — with a
report a reviewer can check step by step, on a branch a person merges.

## Resolving what a transformation reads

A Table input reads through a Kettle connection. The connection is a **lakehouse source** when its
type is PostgreSQL and its database is the configured `source-database` (the host may differ:
Pentaho often points at a replica). Its tables resolve to the plan's tables (schema from the SQL,
else the connection's preferred schema, else `public`), and each becomes `{{ ref('<schema>__<table>') }}`,
the silver model RFC 0193 already builds, with the plan's Iceberg types as field types.

A table that one transformation writes and another reads is the writer's gold model, by
`ref()`. Translation runs until those references stop changing (at most five passes).

Anything else — another database, a table not in the plan — is a **question** in the proposal
and no SQL is written for the branch that needs it.

The Table input's SQL is PostgreSQL. It is rewritten for Trino by a **whitelist walker**: only
constructs whose meaning is the same in both engines pass (`SELECT` with joins, filters,
grouping, ordering, limits, sub-queries, CTEs, set operations, a listed set of functions);
`::` casts become `CAST`, `ILIKE` becomes `lower() LIKE lower()`, unconstrained `numeric` becomes
`decimal(38, 10)` (noted). Any other construct stops the branch with its name. The result must
parse again with the Trino grammar.

Kettle reads `''` as null, so every string field a silver model takes from a source passes through
`nullif(c, '')` (setting `pentaho-empty-string-is-null`, default on).

## The split

Per transformation, from each source (Table input, Data grid, Generate rows) the chain of
row-local nodes (project, filter, distinct without count) with one consumer each becomes one
**silver** model `pentaho__<package>__<transformation>__<step>` in schema `silver_pentaho`.
Everything from there to a sink becomes the sink's **gold** model `gold__<target table>` in schema
`gold`: one model per target, the upstream nodes as a chain of CTEs (a node shared by two sinks is
repeated in each — noted). An Abort step becomes a singular dbt test that fails when any row
reaches it.

| Sink | dbt |
|---|---|
| Table output, truncate | `table` |
| Table output, append | `incremental`, append — Kettle appends on every run (noted) |
| Insert / update | `incremental`, `merge` on the key columns; insert-only columns excluded from `merge_update_columns` |
| Update | `incremental`, `merge`, limited to keys already in the table (noted) |
| Delete | gap |
| Dimension lookup/update | a dbt **snapshot** (check strategy on the type 2 columns) plus a gold model over it: technical key = `dbt_scd_id`, version = row number per natural key, valid from/to = `dbt_valid_from` / `dbt_valid_to` (open end `2199-12-31`), type 1 columns the latest value across versions (noted: the key is a hash, not an integer) |
| Dimension lookup (no update) | a join to that gold model on the natural keys and the validity range at the stream date |

## A step that is not translated

A model with a gap upstream is written **disabled** (`config(enabled=false)`) with every gap, its
reason and the original step text in a header comment, and it is listed under *Not translated*
in the proposal. A disabled model never breaks `dbt parse` (the orchestrator parses the whole
project at load, RFC 0193), and it cannot be run by accident. RFC 0200's "failing test" is
replaced by this: the same visibility, without stopping every other model.

## Output

In the pipeline repository, recorded in `.generated.json` like every generated file (a file a
person edited is left alone):

```
dbt/models/silver/pentaho/<package>/pentaho__<package>__<trans>__<step>.sql
dbt/models/gold/pentaho/<package>/gold__<target>.sql + _gold__<target>.yml
dbt/snapshots/pentaho/<package>/<target>_snapshot.sql
dbt/tests/pentaho/<package>/assert_<trans>__<step>.sql
pentaho/<package>/translation.json         per step: layer, model, fidelity, notes, gap,
                                           original text, generated SQL
proposals/pentaho-<package>.md            the reviewer's view of the same
```

`lakehouse pentaho translate <package>` writes them on branch `ekos/pentaho/<package>/<id>` (the
RFC 0194 branch code); EKOS never merges it. `lakehouse pentaho report [--package] --json` reads
`translation.json` back (class read), for the console (RFC 0207) and the MCP tool
`ekos_pentaho_translation`.

## Checks before a proposal is written

Each model's SQL (refs replaced by the tables themselves) must parse with the Trino grammar.
With a Trino URL configured, every enabled model is also planned with `EXPLAIN`; a model Trino
refuses is disabled and reported with Trino's message.
