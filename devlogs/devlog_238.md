# Devlog 238 — RFC 0170 Phase 3: wider sources for business semantics

**Date:** 2026-10-03
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

RFC 0170's semantics now come from more than views and routines: PL/pgSQL `IF NEW.col …`
conditions (resolved to the trigger's table), Pentaho `FilterRows` conditions, standalone analyst
`SELECT`s, and dbt `schema.yml` (descriptions and `not_null`/`unique`/`accepted_values`/
`relationships` tests). Seed rows cite their own lines. An opt-in LLM step adds a plain-language
summary to undocumented concepts under strict rules: every sentence must cite evidence, hedged
sentences are dropped, and the text is never the definition.

---

## What was built

| Component | Change |
|---|---|
| `kir/predicates.rs` | `Clause::Condition`; `NEW_ROW`/`OLD_ROW` placeholder relations; ops `is_not_true`/`is_not_false` (devlog_235) |
| `recovery/sql_predicates.rs` | `condition_predicates` (only `NEW.`/`OLD.` resolve). 1 test |
| `recovery/plpgsql_analyzer.rs` | Conditions' predicates on statements; `NEW.x` survives the locals filter; logic v5 |
| `recovery/pentaho_analyzer.rs` | `filter_predicates` from the XML condition tree; pass v2. 2 tests |
| `recovery/sql_transform_analyzer.rs` | `top_level_query_predicates` → the graph's `Filter` node; pass v2. 1 test |
| `recovery/dbt_analyzer.rs` | `column_json`: description/type/tests, `macro_target`; pass v2. 1 test |
| `recovery/sql_analyzer.rs` | Per-row seed lines; pass v3. 1 test |
| `recovery/semantics_llm.rs` | New: `describe_concepts`, `keep_cited` (cite-or-drop, hedges dropped). 2 tests |
| `semantic/business_semantics.rs` | Trigger-table resolution, `TransformNode` carriers, dbt domains/FKs/constraints, space-form legends. 3 tests |
| `semantic/semantics_review.rs` | `review_reason` carried forward (a `needs_review` item re-ran into a rewrite). Test tightened |
| `compiler-core/config.rs` | `[semantics] llm-definitions`, `llm-max-definitions` |
| `cli/commands/semantics.rs`, `commit.rs` | Async commit step; `describe_with_llm` (spend prompt, code meanings as evidence); MCP `ai_summary` |
| `cli/commands/export.rs`, `web/ui` | `ekos_ai_summary` annotation; panel shows `llm definition` |

---

## Implementation details worth remembering

- **Conditions resolve only through triggers.** A routine's `NEW.status = 3` names the row of
  whatever trigger runs it; synthesis maps `$new`/`$old` to that trigger's table when every
  trigger running the routine fires on one table, else leaves it unresolved.
- **Pentaho is read structurally**, not from the display string: `IN LIST` values split on `;`,
  `STARTS WITH x` → `LIKE 'x%'`, typed values (`Integer` unquoted, `Boolean` → `true`/`false`), `OR`
  anywhere in a group makes every part a branch.
- **Standalone `SELECT`s only.** The SQL transform analyzer also lowers views and routines; their
  predicates come from `view_analyzer`/`plpgsql_analyzer` already, so only `Statement::Query`
  graphs carry them — otherwise every view filter would count as two carriers.
- **dbt `relationships` is a foreign key** for meaning purposes: a dbt model column referencing a
  seeded lookup gets the lookup's labels.

## Decisions

- **LLM text is never the definition.** It lives in `llm_definition`, is excluded from the review
  signature (no review churn), and is exported as `ekos_ai_summary`, not `description`.
- **Hedges are dropped.** A sentence with "likely"/"probably"/"may be" is a guess even when it cites
  something; dropping it beats shipping a confident-looking guess.

---

## Knowledge Captured

- **A citation requirement does not stop a model from guessing.** Local `llama3` cited real lines
  and still called `category = 'Q'` accounts "likely Quality accounts". What fixed it: give the
  model the meaning EKOS already knows (the column comment's `Q=Equity`) as numbered evidence, and
  discard hedged sentences. Second run: "equity accounts".
- **A `needs_review` item must carry its reason forward**, or the next unchanged commit rewrites it
  without one. `review_reason` is now a review field; the test asserts an unchanged re-run
  reproduces the flagged version exactly.
- **Recovery output changes reopen reviews, by design.** Improving an extractor changes a concept's
  evidence set, so a concept confirmed before the change becomes `needs_review` — seen live on
  `AccTransApproved` after this phase's extractors added sites.
- **Every pass whose output shape changed needs a version bump** — Pentaho, dbt, SQL transform and
  SQL analyzers all hash only their inputs.

---

## Verification

- LedgerSMB (fresh recover): 534 predicate sites (373 resolved), 40 concepts, 208 coded values,
  re-commit 0 after the `review_reason` fix. Starter-set eval unchanged (concept recall 0.68, label
  accuracy 0.95, evidence validity 416/416).
- Analytics demo dbt project: 91 dbt `ConstraintCandidate`s, 27 coded values in 7 columns, the five
  `account_category` codes explained from the dbt description; 5 analyst-query sites (2 resolved —
  the others name tables the repo does not define).
- LLM: local Ollama `llama3`, 8 concepts described, 0 errors; a cached re-run asks nothing and writes
  nothing (14 s vs 75 s).
- `cargo test --workspace` (157), clippy, fmt; LedgerSMB corpus ratchet; UI vitest 73, tsc, build.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/kir/src/predicates.rs` | Condition clause, placeholders |
| `ekos/crates/recovery/src/{sql_predicates,plpgsql_analyzer,pentaho_analyzer,sql_transform_analyzer,dbt_analyzer,sql_analyzer,semantics_llm,lib}.rs` | New sources, LLM text |
| `ekos/crates/semantic/src/{business_semantics,semantics_review}.rs` | Synthesis, review fix |
| `ekos/crates/compiler-core/src/config.rs` | LLM flags |
| `ekos/crates/cli/src/commands/{semantics,commit,export}.rs` | Async step, LLM, export |
| `web/ui/src/pages/SemanticsItemPanel.tsx` | Shows the AI text |
| `ekos/docs/rfcs/0170-business-semantics-linkml.md`, `README.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md`, `TODO.md` | Documented |
