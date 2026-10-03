# Devlog 234 — RFC 0170: business semantics from technical traces, exported as LinkML

**Date:** 2026-10-03
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

The plan "EKOS × LinkML: Recovering the Semantic Layer from Technical Traces" is now RFC 0170, and
its Phase 1 (MVP) is built. Recovery records every column-vs-literal predicate in views, routines and
`CHECK` constraints, normalized, plus key columns and lookup-table seed rows. With
`[semantics] enabled = true`, `ekos commit` synthesizes **hypotheses** — `BusinessConcept`,
`EnumMeaning`, `ConstraintCandidate`, `SemanticGap`, `RationaleLink` — each with `path:line`
evidence, deterministically and with no LLM. `ekos semantics list|show|gaps|eval` read them and
`ekos export linkml` writes a draft LinkML schema that passes `linkml-lint` with 0 errors.

LedgerSMB (SQL + git, no LLM): **39 concepts, 209 coded values in 56 columns (173 explained), 60
constraints, 8 gaps, 97 rationale links**; a re-commit writes 0 new ledger entries. Against a
starter gold set — not expert-written — concept recall 0.73 and label accuracy 0.95.

---

## What was built

| Component | Change |
|---|---|
| `kir/src/predicates.rs` | New: `PredicateSite`/`Clause`/`canonical_text`, the one schema recovery writes and synthesis reads |
| `recovery/src/sql_predicates.rs` | New: AST walk → normalized predicates (WHERE/HAVING/JOIN ON/CASE/CHECK), alias scopes, `subquery` flag, `CASE` labels. 8 tests |
| `recovery/src/plpgsql_footprint.rs` | `Footprint.predicates` |
| `recovery/src/view_analyzer.rs`, `plpgsql_analyzer.rs` | `predicates` on `View`, `ProcedureStatement`, `LANGUAGE sql` `Procedure`; routine parameters/variables filtered; logic versions bumped |
| `recovery/src/sql_analyzer.rs` | Columns gain `not_null`/`primary_key`/`unique`; `check_constraints`; `seed_rows`; column comments record path + line |
| `recovery/tests/semantic_traces_ledgersmb.rs` | New corpus ratchet (`EKOS_LEDGERSMB_DIR`): ≥ 420 sites, ≥ 300 resolved, ≥ 18 seeded tables, no `in_*` parameter as a column |
| `semantic/src/business_semantics.rs` | New: `synthesize` + `RationaleSource` trait. 10 tests |
| `kir/src/custom_kinds.rs` | `EnumMeaning`, `ConstraintCandidate`, `SemanticGap`, `RationaleLink` rows |
| `compiler-core/src/config.rs` | `[semantics]`: `enabled`, `rationale`, `min-sites`, `max-enum-values` |
| `cli/src/commands/semantics.rs` | New: commit step, `GitBlame` (`git blame --porcelain -w`), `list`/`show`/`gaps`/`eval`, `.ekos/semantics/current.json`. 4 tests |
| `cli/src/commands/export.rs` | New: `ekos export linkml`. 4 tests |
| `cli/src/commands/commit.rs`, `app.rs`, `mod.rs` | Wiring + summary line |
| `ekos/docs/rfcs/0170-*.md`, `0170-ledgersmb-starter-gold.yaml` | RFC + starter gold set |

---

## Implementation details worth remembering

- **Normalization.** `=` → `in`, `<>` → `not_in`, a literal on the left is flipped, `NOT col` and a
  bare boolean column become `is_false`/`is_true`, and an `OR` of equalities on one column merges
  into one `IN`. Values are sorted and de-duplicated, so `status IN (3,1)`, `status = 1 OR 3 =
  status` all canonicalize to `t.status IN (1, 3)`.
- **Scope, never a guess.** A column resolves through the enclosing `SELECT`'s aliases or the
  `UPDATE`/`DELETE` target; an unqualified column resolves only when one relation is in scope.
  Otherwise the site keeps `scope: [relations]` and synthesis resolves it when exactly one of those
  tables declares the column.
- **Synthesis at `commit`**, after `procedure_lineage`: a predicate in one file names a table created
  in another, and git blame needs the workspace — `SemanticCompilerPass` has no repository access.
- **Meaning sources**, in priority: column-comment legend (`A=asset,L=liability`, ≥ 2 pairs, 0.9),
  lookup seed via FK — or the lookup table's own key (0.8), `CASE` label (0.5). A column that
  references a seeded lookup gets every seeded value, compared in code or not.
- **`.ekos/semantics/current.json`.** The ledger is append-only, so an item whose traces disappeared
  is still in it. The manifest lists what the latest run derived; read commands filter by it.
  Phase 2 turns this into a real `needs_review` status.

## Decisions

- **No `PredicateSite` objects.** Sites ride on the object they occur in (view, statement); thousands
  of tiny objects would add nothing a property does not.
- **Opt-in.** The plan says to communicate this as an experiment until eval numbers from an expert
  gold set exist.
- **Default export filter `confirmed`.** Phase 1 has nothing confirmed, so the default exports
  nothing and says to pass `--status hypothesis` — hypotheses never leave as facts by accident.
- **Row keys are not codes.** A single-column primary key or unique column compared to a literal
  picks one row. Excluding them took the LedgerSMB gap report from 19 (mostly `defaults.setting_key`)
  to 8 real questions.

---

## Knowledge Captured

- **`ObjectKind::Custom(x)` where `x` names a built-in variant does not round-trip.** `Custom` is
  `#[serde(untagged)]`: `Custom("BusinessConcept")` serializes to `"BusinessConcept"` and
  deserializes as `ObjectKind::BusinessConcept`. All 45 concepts were in the ledger, visible to EKL,
  and invisible to every `ObjectKind::Custom(k) if k == …` match. Before adding a `Custom` kind,
  check it is not already a built-in variant name (`BusinessConcept`, `BusinessRule`, `Column`,
  `Dataset`, `Pipeline`, `Model`, `Agent`, …); the new test round-trips every RFC 0170 kind.
- **PL/pgSQL parameters parse as columns.** Inside a routine's SQL, `in_from_date IS NULL` is an
  `Identifier`, and with one table in scope it "resolves" to that table. Filter by the routine's
  parameter and declared-variable names before treating anything as a column.
- **`git blame` without `-w` cites whitespace commits.** LedgerSMB has "Remove trailing spaces and
  tabs" commits on many lines; `-w` skips them. Blame is still "last changed", not "introduced".
- **sqlparser 0.53 `Expr::Value` has no span**; only `Ident`s do. A multi-row `VALUES` list can only
  be cited at its `INSERT` line — the evidence-validity check reads a window after it.
- **A self-written gold set is not an evaluation.** The plan requires an expert to write it before
  seeing output. The starter set exercises the format and gives smoke numbers; it was written after
  the first run and says so in its header.

---

## Verification

- Unit tests: 8 extractor, 10 synthesis (normalization, alias scope, CASE labels, subquery flag,
  comment legends, seeds through FK and own key, row-key exclusion, rationale, serde round-trip,
  determinism), 8 CLI (porcelain parsing, eval scoring, type mapping, `LIKE` → regex, names).
- LedgerSMB end to end on a fresh local clone (19,542 commits), no LLM key: numbers above; second and
  third commits write 0 new semantics entries.
- LinkML 1.x in a scratch venv: `linkml-lint` 0 errors (437 warnings: `recommended` for undocumented
  columns, `standard_naming` for numeric codes); `gen-json-schema` and `gen-pydantic` succeed.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -D warnings`, `cargo fmt --check`.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/kir/src/predicates.rs`, `lib.rs`, `custom_kinds.rs` | Shared predicate schema; 4 registry rows |
| `ekos/crates/recovery/src/sql_predicates.rs` | New extractor |
| `ekos/crates/recovery/src/plpgsql_footprint.rs`, `plpgsql_analyzer.rs`, `view_analyzer.rs`, `sql_analyzer.rs`, `lib.rs` | Predicates, constraints, seeds, key columns |
| `ekos/crates/recovery/tests/semantic_traces_ledgersmb.rs` | New corpus ratchet |
| `ekos/crates/semantic/src/business_semantics.rs`, `lib.rs` | New synthesis |
| `ekos/crates/compiler-core/src/config.rs` | `[semantics]` |
| `ekos/crates/cli/src/commands/semantics.rs`, `export.rs`, `commit.rs`, `mod.rs`, `app.rs` | Commands + commit step |
| `ekos/docs/rfcs/0170-business-semantics-linkml.md`, `0170-ledgersmb-starter-gold.yaml` | RFC + starter gold set |
| `README.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md`, `TODO.md` | Capability documented |
