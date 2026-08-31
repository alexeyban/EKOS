# Devlog 143 — RFC 0117: dbt project metadata analyzer

**Date:** 2026-08-31
**PRs:** the commit landed with this devlog
**Branch:** main (direct)

---

## Summary

Testing the new MCP TCP transport against a real Databricks/dbt project surfaced a real capability
gap: EKOS extracted no `Table` objects at all for that project's dbt-managed tables. `sql_analyzer`
only sees literal `CREATE TABLE` DDL; `sql_transform_analyzer` turns `SELECT`/view SQL into
Transformation IR lineage steps, not `Table` entities; `dbt-gen` is the reverse direction (EKOS →
dbt rendering). RFC 0117 adds `DbtAnalyzerPass`: static extraction of real `Table` objects from a
dbt project's own checked-in files — never a live warehouse connection, never `manifest.json` —
live-verified end to end against a real medallion-architecture Databricks project.

---

## PR — RFC 0117: dbt project metadata analyzer

### Problem / motivation

dbt can point at any warehouse (Databricks, Snowflake, Postgres, ...), so the user's explicit
direction was: the analyzer must work from dbt's own metadata, not database introspection.
Investigating the real project confirmed a second, non-obvious constraint: `dbt/target/` (where
`manifest.json`/`catalog.json` would live, dbt's own richest, most-resolved metadata) is gitignored
— a build artifact, not checked-in source of truth. So the only stable input is the actual
committed project: Jinja-templated `.sql` model files plus `schema.yml`/`sources.yml`-shaped YAML.

### What was built

| Component | What it does |
|---|---|
| `crates/recovery/src/dbt_analyzer.rs` — `DbtAnalyzerPass` | New `CompilerPass`: builds `Table` objects for every `models/**/*.sql` file and every declared `sources[].tables[]` YAML entry, plus `DependsOn` edges from regex-extracted `ref()`/`source()` macro calls |
| `crates/cli/src/commands/recover.rs` | New discovery block: walks `observe_paths` for a `dbt_project.yml` marker, collects sibling `models/**/*.sql` and `*.yml`/`*.yaml`, registers one pass per discovered dbt project |

### Implementation details worth remembering

**File existence is the primary signal for models; YAML is only enrichment.** A `.sql` file under
`models/**/` *is* a dbt model regardless of whether any `schema.yml` documents it — YAML only adds
description/columns on top of a model that already exists. Getting this backwards (treating YAML
`models:` entries as the existence signal) would have silently dropped every undocumented model,
and this real project has plenty of those. Sources are the opposite: they have no `.sql` file at
all (dbt only references a pre-existing table), so YAML is their only existence signal.

**`ObjectKind::Table`, not a new `Custom("DbtModel")` — a deliberate identity-resolution choice.**
Investigated the identity crate before deciding: `Custom(_)` kinds go through `DefaultResolver`'s
blanket kind-exclusion list (`crates/identity/src/lib.rs:402`) specifically because they're usually
self-identified by a structural key (file path, module name) where same-kind name-similarity
scoring is dangerous — that's the exact bug class behind every kind already on that list
(`Section`/`RustModule`/`Crate`/etc., per this repo's own CLAUDE.md). `Table` doesn't have that
problem: it already has real column-Jaccard structural scoring instead of the naive `1.0` fallback.
A dbt model *is* a real table, so if a DDL-based analyzer ever independently discovers the same
warehouse table by name, letting `DefaultResolver` fuse them via genuine column overlap is the
desired behavior. This mirrors an existing precedent exactly: `python_analyzer.rs`'s SQLAlchemy-
ORM-to-`Table` promotion (RFC 0091) uses `ObjectKind::Table` with its own id namespace
(`"python-orm-table:"`) so it never collides with a same-named DDL table's id, while both stay
mergeable by real identity resolution. This analyzer does the same with `"dbt-table:{project}:"`.

**Dependency edges use the built-in `RelationshipKind::DependsOn`, not the Transformation IR's
`Custom("FeedsInto")`.** `FeedsInto` is step-to-step lineage within one transformation (what
`sql_transform_analyzer`/`pentaho_analyzer` already use); this is whole-table-to-whole-table
dependency — the same kind RFC 0094's `concentration_risks` pass already scans the whole graph for.
Confirmed live: once `silver_customer` had real dependents recorded, `concentration_risks` picked
it up as a real risk in the very same `ekos compile` run, with no changes needed on that side.

**Regex macro-call extraction, not Jinja evaluation or SQL parsing.** Model bodies are Jinja
templates (`{{ config(...) }}`, `{% set %}`, `{{ ref(...) }}`) that a real SQL parser chokes on
outright (confirmed: `sql_analyzer`/`sql_transform_analyzer` already fail with "Expected: an SQL
statement, found: {" on every one of these files, a pre-existing, harmless warning this pass runs
alongside without needing to fix). Extracting just the `ref('x')`/`source('s','t')` macro calls via
regex is enough for table-level lineage without needing a Jinja engine. Repeated calls to the same
target within one model (a real, common pattern — the same upstream table referenced from more than
one CTE) are deduplicated to one `DependsOn` edge, not one per occurrence.

**Honestly skip what can't be resolved, twice over.** Two independent "don't fabricate" points,
verified live: (1) a model with no YAML documentation gets no `columns` property at all — not an
empty array, not guessed columns — confirmed via `silver_customer` (has YAML, gets real declared
columns) vs. models with no matching YAML entry (get none); (2) a `ref()`/`source()` call that
doesn't resolve against the same dbt project's known tables (e.g. a cross-package reference into
`dbt_packages/`, itself gitignored) is skipped with a debug-level trace, never turned into a
fabricated edge — covered by a dedicated test mirroring the existing SQLAlchemy same-file-only
honesty test's naming convention.

### Verification

Live end-to-end run against a real Databricks/dbt project (medallion bronze/silver/gold/semantic
layers, ~40 models): `ekos recover && ekos compile && ekos commit`, then confirmed via `ekos query
find`/`ekos query object`/`ekos query neighbourhood`:
- `silver_customer` — real `Table`, `dbt_kind: "model"`, real declared columns
  (`customer_id`/`is_active`) matching `_silver_models.yml` exactly.
- `bronze_actor` — real `Table`, `dbt_kind: "source"`, `dbt_source: "bronze"`, no backing `.sql`
  file, as designed.
- `sem_customer_context` — real `materialized: "incremental"` extracted from its `config()` block;
  neighbourhood query showed 4 real `DependsOn` edges matching the file's actual `ref()`/`source()`
  calls exactly, including one on line 48 of a 90-line file (`source('semantic', 'business_rule')`)
  that wasn't visible in the excerpt inspected during design — confirming the regex scan finds every
  occurrence in the full file, not just the obvious top-of-file ones.

Unit tests (9, all passing) cover: undocumented models still becoming tables, YAML column/
description merging, sources with no `.sql` file, macro-call resolution to real edges, dedup of
repeated refs, the two honesty cases above, malformed-YAML tolerance, and deterministic/
project-namespaced ids. Full workspace gate (`build`/`test`/`clippy -D warnings`/`fmt --check`
across `ekos/`, plus `tests/integration` and `benchmark` builds) clean.

---

## Knowledge Captured

- **`dbt/target/` (where `manifest.json`/`catalog.json` live) is a build artifact, not source of
  truth** — confirmed gitignored on a real project. Any future EKOS work touching dbt should assume
  it's absent and read the project's own `.sql`/`.yml` files directly, the same lesson this
  analyzer is built on.
- **`ObjectKind::Table` sidesteps the `Custom(_)` identity-resolution exclusion-list bug class
  entirely** — that list (`crates/identity/src/lib.rs:402`) only matters for `Custom(_)` kinds;
  `Table` already has real structural (column) scoring. Worth remembering next time a new analyzer
  considers whether to mint a new `Custom(_)` kind vs. reuse a built-in one with its own id
  namespace — the built-in is often both simpler and safer when the thing really is that kind of
  real-world entity.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0117-dbt-metadata-analyzer.md` | New RFC |
| `ekos/crates/recovery/src/dbt_analyzer.rs` | New `DbtAnalyzerPass`, 9 unit tests |
| `ekos/crates/recovery/src/lib.rs` | Export `DbtAnalyzerPass` |
| `ekos/crates/recovery/Cargo.toml` | Added `regex.workspace = true` |
| `ekos/crates/cli/src/commands/recover.rs` | New `dbt_project.yml`-gated discovery + registration block |
| `CLAUDE.md` | `recovery` crate row mentions `dbt_analyzer` |
| `README.md` | New "dbt metadata extraction (RFC 0117)" section |
| `docs/generated/ekos-self-documentation.html` | Knowledge recovery section (§03) documents the new analyzer |
| `TODO.md` | RFC 0117 marked landed; "Additional connectors on demand" bucket's dbt placeholder noted as covered |
