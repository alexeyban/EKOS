# Devlog 186 — REASON planner: routing and entity resolution (79 → 87/101)

**Date:** 2026-09-15
**PRs:** (local, not pushed) `fix(runtime): planner routing + entity resolution`, plus this docs commit
**Branch:** main (local)

---

## Summary

After devlog_185's knowledge-side work, the remaining eval failures were overwhelmingly a **planner** problem, not a
knowledge problem. The facts were in the ledger, but the question was routed to the wrong operation or anchored on the wrong
entity. Five small, deterministic fixes in `ekos-runtime`'s query understanding moved the same Zen
`deepseek-v4-flash` run from **79/101 to 87/101**, groundedness 86.8% → 95.6%, with no new regressions and no
LLM or ledger change. The fresh model calls for the whole measurement cost ≈ $0.02.

---

## PR — planner routing + entity resolution

### Problem / motivation
The 22 failures in `zen-final-8k`, read one by one:
- **Structural misrouting (5).** A relation phrase buried in a descriptive clause triggered a graph walk: a
  "what function builds X *used by* Y" question became *Graph Dependents of X*, and the answer was never searched.
  Same shape: lin-003, lin-004, lin-008, hist-012.
- **Wrong entity (4).** A `::` path was never extracted as a mention, so its fragment `ekos` resolved exactly to the
  `ekos` crate (dep-004). "recovery crate" resolved `crate` to `crate::ArmSet` via Jaro-Winkler prefix bonus (dep-001).
  "Evidence record" resolved the generic noun `record` to an unrelated symbol (lin-004).
- **Fact-attribute hijack (2).** Any "return/returned" keyword routed to a `returns` fact of whatever entity was
  guessed — the `ekos` crate for arch-017, `redact` for sec-008.
- **Uncitable lookup (1).** Once `::` paths resolved, `sql_analyzer::SqlAnalyzerPass` hit a bare import-path
  `RustModule` stub (name + kind, no evidence).

### What was built
| Fix | Where | Effect |
|---|---|---|
| Structural cue must open the question (≤ 3 words in); `what does … depend on` → Dependencies; `depend on` added | `retrieval.rs::classify_intent` | descriptive clauses no longer trigger graph walks |
| `::` paths are mentions; keyword fragments of a qualified mention are not candidates | `extract_mentions`, `understand` | `ekos_common::redaction` no longer becomes `ekos` |
| `GENERIC_NOUNS` / `QUESTION_VERBS` never resolve as bare keywords | `understand` | no `crate`→`crate::ArmSet`, `depends`→`fastapi.Depends` |
| Match rules: exact 1.0; `path::` family 0.9 (prefer `::self`); last segment 0.9; fuzzy only at length ratio ≥ 0.6 | `resolve_entities` | "recovery crate" → `ekos-recovery` |
| Fact-attribute route only for a named mention or a ≤ 5-word exact query | `reason.rs::plan` | "orders columns" still works; long questions search |
| `Lookup` = `Compose[Fact "*", Search(5)]` | `reason.rs::plan` | a stub entity still brings citable evidence |

### Decisions
- **Position rule, not a parser.** "Opens the question" (≤ 3 words) separates `which crates depend on X` from
  `… section says it depends on RFC 0015` with one line and no grammar model; the eval's structural scenarios all
  still route structural.
- **Kept fuzzy matching, bounded it.** Jaro-Winkler's prefix bonus was the bug, not fuzzy matching itself; a length
  ratio guard keeps `userservice`→`UserService` working.
- **Search added to every Lookup** rather than special-casing Rust import stubs — cheaper to reason about; the
  existing "orders" lookup test was updated to the new shape.

---

## Measurement

Same model and config as devlog_185 (`ekos.zen.toml`, `[ai] max-tokens = 8192`), same rebuilt ledger.

| Category | Baseline (old ledger) | devlog_185 final | Now |
|---|---|---|---|
| adversarial | 15/18 | 15/18 | 15/18 |
| architecture | 12/20 | 15/20 | **18/20** |
| code | 12/15 | 13/15 | 13/15 |
| dependencies | 10/12 | 9/12 | 10/12 |
| history | 8/12 | 10/12 | **11/12** |
| lineage | 5/12 | 8/12 | **10/12** |
| security | 8/12 | 9/12 | **10/12** |
| **Total** | **70** | **79** | **87** |

| Metric | devlog_185 final | Now |
|---|---|---|
| Answer correctness | 77.8% | 85.7% |
| Evidence groundedness | 86.8% | 95.6% |
| Hallucinated | 3 | 3 |

Flipped to pass: `arch-001, arch-017, arch-019, dep-001, hist-012, lin-003, lin-004, sec-008`. Report: `evals/reports/zen-planner2/`.

Still failing (14): adversarial premise rejections not phrased as refusals (`adv-004/011/015`), retrieval-only
scenarios (`arch-009`, `hist-007` — recall graded, no answer), `code-002`/`lin-008`/`dep-004` (answer not in top
evidence), `code-004` (the edition lives in `ekos/Cargo.toml`'s `[workspace.package]`, not surfaced), `dep-005`,
`arch-020`/`lin-007`/`sec-002`/`sec-010` (wording/expected-keyword misses).

---

## Knowledge Captured

- **Jaro-Winkler's prefix bonus makes short generic words match long qualified names** (`crate` ≈ `crate::ArmSet`,
  `ekos` ≈ `ekos_common::redaction`). Always pair it with a length-ratio guard.
- **Rust `RustModule` objects are import paths, one per imported item** (`ekos_common::redaction::redact`,
  `…::self`); there is usually no object named exactly the module path a human writes. Resolve a module path as a
  `path::` family.
- **A structural phrase anywhere in a question is a weak signal**; its position is a strong one.
- **Planner fixes are cheap to measure**: unchanged prompts replay from the LLM cache, so a full 101-scenario run
  after a routing change made only 36 fresh calls.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/crates/runtime/src/retrieval.rs` | cue-position routing, `::` mentions, generic-noun/verb stoplists, segment/path-family/length-guarded resolution, tests |
| `ekos/crates/runtime/src/reason.rs` | fact-attribute gating, `Lookup` = Fact + Search, test update |
