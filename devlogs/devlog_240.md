# Devlog 240 — RFC 0170: CTE and dbt lineage, Python enums, SQL in Perl, gold-set tooling

**Date:** 2026-10-03
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

The four limits devlog_239 listed are closed, three in code and one in tooling. Filters written
through CTEs and derived tables are restated on their base columns, and dbt model columns are traced
back across models to the source column — the demo's dbt sites go from 6 to **20 of 20** resolved.
Python `Enum` classes are constant groups. SQL inside Perl strings is parsed (LedgerSMB: 303
strings, 82 literal sites). The expert gold set is not something EKOS can write: `ekos semantics
gold-template` gives an expert a blank, structure-only file to fill in blind, and `eval` now says so
plainly when a gold set was not.

---

## What was built

| Component | Change |
|---|---|
| `recovery/sql_predicates.rs` | `ColMap`/`chase`/`projection_map`, `learn_derived`, `through_derived`; `output_lineage`; `PREDICATES_VERSION`. 3 tests |
| `recovery/dbt_analyzer.rs` | `parse_model`, `model_lineage` → `column_lineage` on models; v4 |
| `recovery/python_analyzer.rs` | `enum_members` → `constants` (with `group`) on `Enum` class symbols; v5. 1 test |
| `recovery/perl_sql.rs` | New: `sql_strings` (q/qq any delimiter, quotes, heredocs, POD/comment-aware), `neutralise`, `perl_sql_predicates`. 2 tests |
| `recovery/perl_analyzer.rs` | Predicates on the innermost `sub`; v3. 1 test |
| `recovery/view_analyzer.rs`, `plpgsql_analyzer.rs`, `sql_transform_analyzer.rs` | Cache keys include `PREDICATES_VERSION` |
| `semantic/business_semantics.rs` | Model-lineage chase; dbt models exempt from the declared-column check; explicit constant groups + `name` matching; `PerlSymbol`/`PerlPackage` carriers. 2 tests |
| `cli/semantics.rs`, `app.rs` | `gold-template`; `GoldMeta` + eval caveat. 1 test |
| `ekos/docs/rfcs/0170-ledgersmb-starter-gold.yaml` | `meta` block (not expert, not blind) |
| `recovery/tests/semantic_traces_ledgersmb.rs` | Perl SQL ratchet (≥ 100 strings) |

---

## Implementation details worth remembering

- **A CTE is a column map, not a relation.** Output name → `(base relation, column)` for plain
  column references; `SELECT *` from one relation passes everything through; computed outputs are
  absent, so a filter on them keeps no relation instead of a wrong one.
- **dbt lineage is the same map, saved.** A model's `column_lineage` lets synthesis follow
  `mart → stg → source` up to 8 hops; the declared-column check is skipped for dbt models because
  `schema.yml` documents only some columns.
- **`perl_var`.** Interpolated variables become an identifier, so they can never become literals;
  one interpolated inside quotes (`'$x'`) becomes the string `'perl_var'`, and those sites are
  dropped.

## Decisions

- **No Python module constants.** In the demo they are paths and settings; reading them would add
  noise, not codes. `Enum` classes are explicit about being code sets.
- **No gold set authored here.** A gold set written by the session that built the extractor
  measures agreement with itself. The template shows structure only, and `eval` prints a caveat
  unless `meta.written_before_seeing_output: true`.

---

## Knowledge Captured

- **Rust `\` line continuations strip leading whitespace.** The gold template's indented `meta:`
  keys came out flush-left — still valid YAML, but filling it in would have set top-level keys the
  eval ignores. Use a raw string for anything indentation-sensitive, and test that a filled-in
  template round-trips.
- **"Must declare the column" is wrong for partially documented sources.** dbt models list only the
  columns someone documented; the check silently dropped 14 of 20 demo sites.
- **Extractor versioning belongs in one place.** Five passes record predicates; a shared
  `PREDICATES_VERSION` in their cache keys (with a test for literal pass versions) replaces five
  hand bumps.

---

## Verification

- Demo: 20 of 20 dbt sites resolved, 6 concepts (`MartOrderBacklog` → `oe.closed IS FALSE` through
  two models), re-commit 0.
- LedgerSMB (`lib/` observed): 649 sites (458 resolved), 47 concepts, re-commit 0; gold template
  lists 184 tables; the starter-set eval now opens with the not-an-expert caveat.
- `cargo test --workspace` (157), clippy, fmt, LedgerSMB ratchets (SQL + Perl).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/{sql_predicates,dbt_analyzer,python_analyzer,perl_sql,perl_analyzer,view_analyzer,plpgsql_analyzer,sql_transform_analyzer,lib}.rs` | Lineage, new sources, versioning |
| `ekos/crates/recovery/tests/semantic_traces_ledgersmb.rs` | Perl ratchet |
| `ekos/crates/semantic/src/business_semantics.rs` | Lineage chase, constant groups |
| `ekos/crates/cli/src/{commands/semantics.rs,app.rs}` | Gold template, caveat |
| `ekos/docs/rfcs/0170-business-semantics-linkml.md`, `0170-ledgersmb-starter-gold.yaml` | RFC, meta |
| `README.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md`, `TODO.md` | Documented |
