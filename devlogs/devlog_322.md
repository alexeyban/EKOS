# Devlog 322 — Apache Ossie interchange: export, ingest, review round trip (RFC 0245)

**Date:** 2026-11-07
**PRs:** — (worked on main)
**Branch:** main

---

## Summary

The user asked for import, export and integration between EKOS and Apache Ossie. Ossie
(incubating since 2026-06-22) is the former Open Semantic Interchange: a vendor-neutral YAML/JSON
format for semantic models — datasets, fields, keys, relationships, metrics, `ai_context`.
Snowflake, dbt, ThoughtSpot, Dremio and Power BI read and write it.

The user chose:

- **Import means both** ingesting Ossie files from the sources as knowledge, and a human-only
  review round trip like `ekos import linkml`.
- **Versions:** write the current flat `0.2.0.dev0`, read `0.1.1` too.
- **Scope:** an MCP tool, a console view, and validation against the official schema.

All of it shipped. Exports pass the official schema in the tests and Ossie's own `validate.py`,
including its sqlglot SQL parse. On a Northwind project, ingest found exactly the two
disagreements planted in an Ossie file, and the review round trip recorded confirm, edit and
reject decisions.

---

## RFC 0245 — Apache Ossie interchange

### Problem / motivation
EKOS already compiled what an Ossie model holds: tables, typed columns, keys, foreign keys, author
comments, and RFC 0170 meanings with a review status. It could not hand them to the BI and
semantic-layer tools that speak Ossie, and it ignored the Ossie models teams keep beside their code.

### What was built
| Component | Where |
|---|---|
| Spec types (`0.2.0.dev0`), reader for both versions + `sniff`, SQL → `DataType`, `from_ledger` builder, structural checks, `fk_desc` column parser, kind constants | new crate `crates/ossie` (`ekos-ossie`) |
| Official `ossie-schema.json` (apache/ossie@8437d59, Apache-2.0) + official examples as fixtures | `crates/ossie/schema/`, `crates/ossie/tests/fixtures/` |
| Tables record whole `primary_key` and `unique_keys`, composite ones included | `recovery/src/sql_analyzer.rs` (`v7`), `oracle_catalog_analyzer.rs` (`oracle-catalog/5`) |
| `ekos export ossie` (`--status`, `--database`, `--source`, `--json`, `--out`) | `cli/src/commands/export.rs` (`ossie_model`, shared with MCP) |
| Ingest: discovery + redaction | `cli/src/commands/recover.rs::register_ossie_passes` |
| `OssieAnalyzerPass`: keyed `OssieModel` / `OssieDataset` / `OssieMetric` | `recovery/src/ossie_analyzer.rs` |
| Commit: dataset → table `DependsOn`; Ossie-vs-schema conflicts | `semantic/src/ossie_links.rs`, `commit.rs`, `commands/conflicts.rs` |
| `<state>/ossie/current.json` — the files the latest recover found | `export.rs::{current_ossie_files, write_current_ossie_files}` |
| `ekos import ossie` (human-only), shared apply/report with LinkML | `cli/src/commands/import.rs` |
| MCP `ekos_ossie_export` (scoped, read-only, `match` + `max_datasets`) | `commands/mcp.rs`, `web/api/app/mcp_policy.py` |
| Console: `/semantics/ossie`, `/ossie/yaml`, `/ossie/validate`, `/ossie/import`; Semantics → *Apache Ossie* view | `web/api/app/routes/semantics.py`, `semantics_write.py`, `readproc.py`, `web/ui/src/pages/Ossie.tsx` |
| Registries: three keyed custom kinds, identity "ossie" namespace, docs-gen entity pages, catalogue rows, `HUMAN_ONLY` | `kir/custom_kinds.rs`, `identity/lib.rs`, `docs-gen/lib.rs`, `catalogue.rs`, `review_guard.rs` |
| `synonyms` indexed for search (`CONTENT_VERSION` 3) | `kir/src/lib.rs::indexed_content`, `ledger/src/search.rs` |

### Implementation details worth remembering
- **`ekos-ossie` depends only on `kir` and serde.** `semantic` depends on it, for the SQL-type
  families and kind names, not the other way round.
  - A first version depended on `semantic` for RFC 0170's constants. That made the commit-time
    linker unable to use the type mapping.
  - The constants are now duplicated in `ossie::kinds`. A test in `ossie_links` holds them equal
    to `business_semantics::{CONCEPT, ENUM_MEANING}`.
- **Only reviewable things are decisions.** The import reads decisions only from the `EKOS`
  extensions' `concepts` (`name`, `description`, `status`, `note`) and `enum_meanings` (`label`,
  `status`, `note`). Any other edit compares unequal after extensions and `ai_context` are stripped,
  and becomes a warning.
- **The baseline must be rebuilt the same way.** The model's `EKOS` extension records
  `export_status` and any `--database` / `--source` selection, so the import rebuilds the identical
  baseline.
- **When an ingested metric is carried.** Only when every field of the linked Ossie dataset is a
  plain column under its own name and exists on the table. Otherwise the metric's references might
  name a field EKOS's dataset does not have, and the export would be invalid.
- **The datatype check is per family.** A disagreement needs different families: number,
  datetime, date, time, string, boolean. Boolean beside a number is not one (MySQL
  `tinyint(1)`, Oracle `NUMBER(1)`).
- **The missing-column check is only for complete column lists:** DDL with a dialect, or a live
  catalog. A dbt model documents only some columns.

### Decisions
- **Export `0.2.0.dev0`, although the spec marks it unstable.** The official schema on `main`
  pins it. `0.1.1` reads fine, but its keys named source columns, which 0.2 forbids. The 0.1.1
  TPC-DS example's `primary_key` names `ss_ticket_number`, which is not a field, and the checker
  says so rather than passing it.
- **A business concept is never turned into a metric.** A concept is a predicate, not an
  aggregation. It goes into `ai_context.instructions` with its status in words.
- **Glossary terms are not dataset synonyms.** They name the concept, not the table.

---

## Knowledge Captured

- **An export left in the project was ingested back as a source on the next `recover`.** This is
  the self-import loop the glossary reader hit once. A model whose model-level `EKOS` extension has
  `export_status` is now skipped (`ossie_analyzer::is_ekos_export`).
- **A deleted per-file input's objects outlive the file, in the CKM as well as the ledger.**
  Measured: deleting the stray export left its 17 objects in `ckm/model.json`. This is general to
  per-file analyzers, not new.
  - For Ossie, `recover` writes `<state>/ossie/current.json`.
  - Linking, conflicts, export and MCP filter by it (`ossie_links::is_current`).
  - Any new per-file source whose objects feed a whole-ledger step needs the same.
- **`NameIndex::resolve("db.schema.t")` does not find a table recorded as `public.t`.** A
  qualified lookup only tries the exact key, then the bare tail as an *exact* name.
  - `ossie_links::resolve` tries the full name first.
  - Then it tries the last part alone, which uses the tail map.
- **`python -I` hides user site-packages.** So the official validator needs its own environment:
  `uv run --no-project --with pyyaml --with jsonschema --with sqlglot python -I validate.py`.
- **vitest in the sandbox cannot create `node_modules/.vite/vitest`.** Run `npx vitest run --no-cache`.
- **The `jsonschema` crate needs `default-features = false`.** Without it, it pulls in reqwest
  and rustls for remote `$ref`s. The Ossie schema refers only inside itself.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/crates/ossie/**` | new crate: model, reader, builder, checks, datatype, fk, kinds; vendored schema + fixtures; tests (incl. opt-in official validator) |
| `ekos/crates/recovery/src/ossie_analyzer.rs` | new: Ossie files → keyed KIR objects; skips EKOS's own exports |
| `ekos/crates/semantic/src/ossie_links.rs` | new: dataset → table links, Ossie-vs-schema conflicts, `is_current` |
| `ekos/crates/recovery/src/sql_analyzer.rs`, `oracle_catalog_analyzer.rs` | table-level `primary_key` / `unique_keys`; versions bumped |
| `ekos/crates/cli/src/commands/{export,import,recover,commit,conflicts,mcp,catalogue,review_guard,semantics}.rs`, `app.rs` | `export ossie`, `import ossie`, ingest, commit wiring, MCP tool, registrations, guards |
| `ekos/crates/kir/src/{custom_kinds,lib}.rs`, `identity/src/lib.rs`, `docs-gen/src/lib.rs` | registry rows, synonyms indexed, identity namespace, entity pages |
| `ekos/crates/ledger/src/{search,fact_ledger}.rs` | `CONTENT_VERSION` 3 |
| `web/api/app/{readproc,semantics_write,mcp_policy}.py`, `routes/semantics.py`, tests | Ossie routes, allowlist, import helper, scoped tool |
| `web/ui/src/pages/{Ossie.tsx,Ossie.render.test.tsx,Semantics.tsx,Linkml.tsx}`, `api/types.ts` | Apache Ossie view; `PlanView` shared |
| `ekos/docs/rfcs/0245-apache-ossie-interchange.md` | the RFC |
| `CLAUDE.md`, `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | docs |
