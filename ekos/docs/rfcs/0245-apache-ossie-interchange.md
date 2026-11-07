# RFC 0245 — Apache Ossie interchange: export, ingest, review round trip

**Status:** Accepted
**Created:** 2026-11-07
**Builds on:** RFC 0170 (business semantics → LinkML), RFC 0172 (conflicting evidence),
RFC 0043 (redaction), RFC 0117 (dbt analyzer pattern), RFC 0163 (`procedure_lineage::NameIndex`),
RFC 0181/0182 (scoped MCP), RFC 0186 (command catalogue)

## 1. Problem

[Apache Ossie](https://ossie.apache.org) (incubating since 2026-06-22, formerly Open Semantic
Interchange, `github.com/apache/ossie`) is the vendor-neutral YAML/JSON format for semantic models
that Snowflake, dbt, ThoughtSpot, Dremio, Power BI, Sigma and others read and write. A model has:

- `datasets` — a `source` table or view, `fields` (`expression.sql`, `datatype`,
  `dimension.is_time`), `primary_key`, `unique_keys`, dataset-scoped `metrics`;
- `relationships` — `from`/`to` datasets and `from_columns`/`to_columns` (**fields**, not source
  columns);
- model-scoped `metrics`;
- `ai_context` (a string, or `instructions`/`synonyms`/`examples`) on every element;
- `custom_extensions` — `{vendor_name, data}` where `data` is an opaque JSON **string**.

EKOS already compiles the raw material: tables, columns, types, keys, foreign keys, author
comments, and RFC 0170 business meanings with a review status. It has no way to hand them to that
ecosystem, and it ignores Ossie models that teams already keep next to their code.

## 2. Spec version

Ossie's `main` (commit `8437d59`, 2026-11-06) defines `0.2.0.dev0`: one model per document at the
root. The only release is `0.1.1` (December 2025), which wraps models in a `semantic_model` array.

- EKOS **writes** `0.2.0.dev0`. The official JSON Schema pins `version` to that constant.
- EKOS **reads** both. A `0.1.1` document is migrated the way the spec's "Migrating earlier document
  shapes" says: each array element becomes one model. EKOS never silently picks only the first.
  Root `dialects`/`vendors` declarations are dropped, as the spec says.

The schema is vendored in `crates/ossie/schema/ossie-schema.json` (Apache-2.0) and every export test
validates against it.

## 3. Export — `ekos export ossie`

Built by the pure crate `ekos-ossie` (`build::from_ledger`). The CLI and the MCP tool only load
objects and write text.

| EKOS | Ossie |
|---|---|
| `Table` | dataset. `source` = `database.owner.name`, `schema.name`, or `name`; one field per column (`expression.sql` = the column, quoted when it is not a plain identifier), `datatype`, `description` |
| `Table.primary_key` / per-column `primary_key` | `primary_key` |
| `Table.unique_keys` / per-column `unique` | `unique_keys` |
| `Custom("View")` | dataset with `source` = the view, fields from `columns`, no `datatype` |
| `ForeignKey` relationship (`fk_desc`, composite allowed) | relationship, only when both ends are exported and every column is a field |
| `EnumMeaning` (by `--status`) | field `ai_context.instructions` (`value 'O' means "open"`) |
| `BusinessConcept` (by `--status`) | dataset `ai_context.instructions`; glossary terms → `synonyms` |
| `OssieMetric` ingested from a source (§4) linked to the table | the dataset's metric |

**Anti-invention.** EKOS never writes a metric it did not observe in an Ossie source. A business
concept is a predicate, not an aggregation, so it is never turned into a `COUNT(*) WHERE …`.

**The `EKOS` vendor extension.** Every element carries one
`{vendor_name: "EKOS", data: "<json>"}`:

- on the model: `{"export_status", "ekos_version", "project"}`;
- on a dataset or field: `{"ekos_id", "status": "observed", "origin", "dialect", "evidence"}`, plus
  the review entries it was built from (`concepts` / `enum_meanings`, each `{ekos_id, name|value,
  label|description, status}`);
- on a re-exported metric: `{"ekos_id", "status": "observed", "origin": "ossie:<path>"}`.

The output is deterministic. Elements are sorted by name, the document has no timestamps, and
duplicate dataset names get a numeric suffix.

Flags: `--status confirmed|hypothesis|all` (default `confirmed`, for review items only — tables are
observed facts), `--out`, `--name`, `--json`, `--database <name>`, `--source <name>`.

`ekos-ossie::check` repeats the structural checks of Ossie's `validation/validate.py`, except the
sqlglot SQL parse:

- unique names;
- keys and relationship columns are declared fields;
- arity matches;
- field names and dataset metric names do not collide.

## 4. Ingest — Ossie files as a knowledge source

This follows the dbt analyzer (RFC 0117). There is no plugin crate.

**Discovery.** `recover` walks the observe bases and takes:

- `*.ossie.{yaml,yml,json}`;
- any other `*.yaml|yml|json` whose top level sniffs as Ossie: `version` `0.1.*`/`0.2.*` together
  with `datasets` or `semantic_model`.

**Raw-content entry point.** The walk is one. Each file is read through
`redaction::read_redacted` (RFC 0043), which refuses excluded files and redacts the content before
anything is persisted.

`OssieAnalyzerPass` emits these objects. All three are structurally keyed, so identity never merges
them:

| Kind | Key | Properties |
|---|---|---|
| `Custom("OssieModel")` | file, model | `name`, `description`, `version`, `ai_context` |
| `Custom("OssieDataset")` | file, model, dataset | `source`, `fields[]` (name, expression, datatype, is_time, description, synonyms), `primary_key`, `unique_keys`, `relationships[]`, `synonyms` |
| `Custom("OssieMetric")` | file, model, scope, name | `expression`, `dialects`, `datatype`, `description`, `synonyms`, `dataset` (when dataset-scoped) |

`Contains` edges link `File` → model → dataset → metric. A model-scoped metric hangs off its model.

**Linking at `ekos commit`** (`semantic::ossie_links`, after `procedure_lineage`):

- A dataset's `source` is resolved to a `Table` or `View`. The last dotted part is matched with
  `procedure_lineage::NameIndex`'s unique-name rule, and a miss or an ambiguous name links nothing.
- A resolved dataset gets a `DependsOn` edge to its table.
- A field whose expression is a plain column the table does not have becomes a
  `Custom("ConflictingEvidence")` item (RFC 0172).
- So does a declared `datatype` that disagrees with the column's SQL type, compared after
  normalisation.

## 5. Review round trip — `ekos import ossie <file>`

This is human-only, like `ekos import linkml`.

1. The file is parsed; either spec version is accepted.
2. `export_status` is read from the model-level `EKOS` extension, and the baseline is rebuilt with
   the export builder.
3. A pure `plan(baseline, edited)` matches entries by `ekos_id`, never by name.

| Edit | Result |
|---|---|
| a `concepts`/`enum_meanings` entry's `status` set to `confirmed` / `rejected` | `Decision::Confirm` / `Reject` |
| its `name` / `description` / `label` changed | `Decision::Edit` |
| a field's description, datatype, a new field, a new metric | warning: an observed fact; change the source or add an Ossie file to the sources |
| an unknown `ekos_id` | error |

All decisions are applied before any write; if any one fails, nothing is written. The flags are
`--dry-run`, `--json` and `--as`, and the write context is `stage: "semantics-review:import-ossie"`.

`import ossie` is listed in:

- `review_guard::HUMAN_ONLY`;
- the catalogue as `review`.

A source-scan test keeps it out of `commands/mcp.rs`.

## 6. MCP and console

- **MCP:** `ekos_ossie_export` (`status`, `database`, `format: yaml|json`) is read-only and reads
  only through the store, so it is in `SCOPED_TOOLS` (and in the console's `mcp_policy`).
- **Console:** the Semantics tab gains an Apache Ossie page, next to LinkML:
  - download YAML or JSON (`read`);
  - upload a file to dry-run it (`read`);
  - apply the decisions (`review`).

  All of these run catalogue commands built by `home.command`.

## 7. Not in this RFC

- Ossie ontology and mapping documents.
- Using ingested metrics in the text-to-SQL agent's context.
- Registering `EKOS` in Ossie's `extensions/vendors.md` upstream.
