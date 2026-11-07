# RFC 0246 — Putting ingested Apache Ossie models to work

**Status:** Accepted
**Created:** 2026-11-07
**Builds on:** RFC 0245 (Apache Ossie interchange), RFC 0242/0243 (text-to-SQL), RFC 0036
(dbt-gen), RFC 0170 (business semantics), RFC 0121/0123 (query understanding, REASON),
RFC 0155–0158 (EKOS Migrate validation and data quality)

## 1. Problem

RFC 0245 reads the Apache Ossie models a team keeps in its sources into `OssieModel`,
`OssieDataset` and `OssieMetric` objects, and links each dataset to its table. Only
`ekos export ossie` reads them back. The model states things the schema cannot:

- what "revenue" means (a metric);
- the business's words for a table (synonyms);
- joins the DDL never declared;
- keys;
- descriptions;
- which columns are time axes.

## 2. Rule

An Ossie model is a team's **stated** knowledge.

- It is used as evidence with its origin (`ossie:<path>`).
- It never overrides the schema. Where the two disagree, RFC 0245's `ossie_vs_schema` conflict
  stands.
- Only files the latest `recover` found are used (`ossie_links::is_current`).
- What cannot be resolved is skipped with a reason, never guessed.

## 3. One resolver: `ekos_ossie::knowledge`

`resolve(objects, relationships, current_files) -> OssieKnowledge` (pure) is the single place that
turns ingested objects into facts about the compiled tables.

**Per table:**
- synonyms, description and instructions;
- `primary_key` and `unique_keys` as column names;
- per column: description, synonyms, `is_time`, datatype.

**Joins:** relationships whose two datasets link to tables and whose fields are plain columns.

**Metrics:** each one rewritten over the tables, following the spec's name resolution:
- a field becomes its expression;
- `source.col` becomes the column;
- a model metric's `dataset.field` becomes `<table>.<expression>`.

The rewrite is an AST walk (`sqlparser`). A metric that names an unknown field, refers to another
metric, or belongs to an unlinked dataset is listed under `skipped` with the reason.

## 4. Consumers

| Consumer | What it gains |
|---|---|
| Text-to-SQL (`sqlgen`, `ekos sql check\|generate`, `ekos_sql_check`) | a `metric` tool and a "Defined metrics" prompt block; synonyms in `search_schema`/`relevant`; declared joins in `join_path` (marked as declared); unique keys in L002; lint **L012** when a query names a defined metric but computes something else |
| dbt (`ekos export dbt-semantic`) | MetricFlow `semantic_models:` (entities from keys and joins, dimensions with time axes) and `metrics:` (simple and ratio); key tests; anything else listed in `skipped.md` |
| Business semantics | Ossie descriptions and synonyms are a meaning source (`description_source: "ossie"`), and a field description with a code legend gives `EnumMeaning`s from source `ossie` |
| Ask / query | a mention that is an Ossie synonym or metric name resolves to its table, and the plan says so |
| EKOS Migrate | declared keys are measured as `Unique` claims at `assess`; tier **V2m** reconciles declared single-table metrics between source and target, with a planted control |

## 5. Not in this RFC

- Executing metrics anywhere but EKOS Migrate's validation.
- Writing back to an Ossie file.
- Exporting window or nested-aggregate metrics to MetricFlow.
