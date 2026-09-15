# Devlog 183 — Phase 1 of the eval-improvement plan: E-wording, C-cite, B1, B2, D fixes

**Date:** 2026-09-15
**PRs:** (local, not yet pushed) `fix(runtime,evals): Phase 1 eval fixes — refusal wording, citation parsing, evidence excerpts, retrieval term dilution, grading defects`, `chore: devlog_183, README + TODO + capabilities doc update`
**Branch:** main (local)

---

## Summary

Implemented and measured Phase 1 of the plan from "what's next to improve the 101-scenario eval
suite" — five root causes found by a multi-round research pass over devlog_182's real 42/101
baseline, ranked by yield-per-effort rather than discovery order: a refusal-wording mismatch
(E-wording), broken citation-block parsing (C-cite), evidence claims that rendered only an
object's name and discarded its retrieved text (B1), BM25 term dilution in the retrieval-mode
runner (B2), and four grading/ruler defects that would otherwise have inflated the measured gain
(D1/D3/D4, plus a D5 audit that found no bug). Measured via seven category-scoped
`ekos eval run --agent ollama --save-answers` runs (a full single-process 101-scenario run was
attempted twice and OOM-killed by the host — see Knowledge Captured) against the
`20260914T154459Z-ekos-full` baseline: **42/101 → 53/101 (+11)**, groundedness 51.6% → 65.9%,
recall@10 47.1% → 64.7%, hallucination count 8 → 7 (slightly better, not worse). Two real,
understood side effects are documented rather than hidden: a genuine fabrication-vs-refusal
trade-off in 4 adversarial scenarios, and a citation-format drift in 3 scenarios that cost
`history` its only net regression.

---

## What was built

| Fix | Files | Scenarios touched | Real yield |
|---|---|---|---|
| E-wording — bracketed prompt headers so the model can't echo one as its own answer opener, widened `DEFAULT_REFUSAL_PHRASES` | `runtime/src/ai.rs`, `evals/src/evaluators/groundedness.rs` | 5 named + general | part of the 22 gross flips below |
| C-cite — citation parsing tolerates a bare claim position and an `"evidence <id>"`-wrapped uuid | `runtime/src/ai.rs` (`extract_citations`, `resolve_citation_ref`) | 13 named | part of the 22 gross flips |
| B1 — `Search`/`Graph` claims now carry a bounded excerpt of the object's own `excerpt`/`description`, not just its name | `runtime/src/reason.rs` (`entity_item_with_excerpt`, `excerpt_of`) | 21 named | **the largest single lever** — see Recall@10 jump |
| B2 — strip corpus-generic nouns (`function`/`pass`/`store`) from `mode: retrieval` queries before searching | `evals/src/runners/retrieval_runner.rs` | 3 named (code-006/015, arch-016) | all 3 confirmed fixed via direct `ekos query find` checks before implementing |
| D1 — a refusal never counts as stating a fact, even one that echoes the fact's own keyword | `evals/src/evaluators/answer.rs`, `groundedness.rs` (`looks_like_a_refusal`) | 5 named | prevents a false-positive inflation of the numbers above |
| D3 — added real, ledger-verified `expected_facts` to 3 previously-ungraded dependency scenarios | `evals/datasets/dependencies.yaml` | dep-004/006/007 | makes their content actually checked, not just their query-type routing |
| D4 — attribution no longer counts a fact name that only appears inside a claim's bracketed `[location]`/`(evidence id)` suffix as "shown" | `evals/src/evaluators/mod.rs` (`claim_prose_only`) | arch-015, code-014 named | fixes a mislabelled generation/retrieval split |
| D5 — audited `code-009`/`lin-009`'s suspected name-mismatch bugs | none (verified, not fixed) | — | **both turned out to be real, already-correct exact matches** — see Knowledge Captured |

## Implementation details worth remembering

- **`entity_item` split into a shared `entity_item_inner` plus two thin wrappers** (`entity_item`,
  `entity_item_with_excerpt`) rather than adding a boolean flag to every call site — only the
  `Search`/`Graph` arms in `exec_node` opt into excerpt enrichment; `Fact` claims already carry
  their real value and don't need it.
- **`CLAIM_EXCERPT_MAX_CHARS = 280`**, deliberately smaller than RFC 0140 §3's
  `MAX_SOURCE_TEXT_LINES` (400 lines) — this is the always-on cheap path applied to every hit, not
  the opt-in per-question expensive tier.
- **Citation-index resolution needed the claim's position, not just its source id.** `known_evidence`
  (a `HashSet<KirId>`) can't tell a bare `"2"` in `cited_evidence` apart from a malformed uuid, so
  `extract_citations` gained a third parameter, `claim_order: &[Option<KirId>]`, built from
  `evidence.items.iter().map(|i| i.source)` at the one call site that has real claims
  (`reason_with_history`); `ask_with_history`/`ask_stream_with_history` (0 scenarios in the eval
  suite) pass `&[]` since they have no numbered claim list to resolve against.
- **`retrieval_runner.rs`'s generic-term list is deliberately separate from `ai.rs`'s
  `QUESTION_STOPWORDS`.** The two lists solve different problems: `QUESTION_STOPWORDS` strips
  closed-class function words from a natural-language sentence; `RETRIEVAL_GENERIC_TERMS` strips
  real content words ("function", "pass", "store") that happen to be corpus-frequent enough to
  dilute a short, hand-picked `mode: retrieval` query. Folding them together would have changed how
  `understand()`'s keyword-fallback entity resolution behaves — out of scope for this fix.
- **D5's audit reversed a documented suspicion instead of confirming it.** The plan (based on an
  earlier research pass) suspected `code-009`'s `expected_objects: ["redact"]` and `lin-009`'s
  `expected_objects: ["ObservationArtifact"]` were bad expectations, colliding with a longer
  qualified name in the ledger. Direct `ekos query object`/`ekos query find` checks showed both
  bare names are real, distinct, correctly-typed objects (`redact` is the actual `RustSymbol`
  function; `ekos_common::redaction::redact` is a separate `RustModule` namespacing object) —
  the dataset was already correct. Recorded here so a future pass doesn't re-open this without
  re-checking first.

## Decisions (alternatives considered, why this choice)

- **Renamed the prompt's paragraph headers to a bracketed style (`[Guidance on citing]`) rather
  than just widening the refusal-phrase list alone.** Removing the root cause (a plausible
  sentence-opener the model could echo) seemed more durable than only patching the grader's phrase
  list. Measured trade-off: this also appears to have loosened the model's adherence to the exact
  `{"cited_evidence": [...]}` JSON format in 3 scenarios (see below) — a real cost, weighed against
  4 adversarial scenarios it fixed cleanly (adv-010/011/016/017). Net positive but not free.
- **Ran the suite category-by-category in the foreground instead of one full background run.** A
  single-process full 101-scenario background run was attempted twice and killed both times by the
  host's low-memory condition (`free -h` showed 181Mi free, 22Gi of 73Gi swap in use). Each
  category is small enough to finish comfortably inside a single foreground call's timeout, and a
  fresh process per category releases memory before the next one starts — this worked cleanly for
  all 7 categories with no further kills, at the cost of manually aggregating 7 separate report
  files afterward instead of reading one.
- **Did not chase the two measured side effects further in this pass.** Both are real and
  documented, not hidden, but pursuing either (retuning the prompt's bracket style, or excluding
  weak/partial-overlap hits from excerpt enrichment) is exactly the kind of one-more-tweak
  iteration this project's own `devlog_172` lesson warns against chasing without a fresh, focused
  measurement cycle. Reported to the user as a known trade-off; net effect (+11 pass, hallucination
  count down not up) was accepted as-is.

---

## The 22 gross flips to pass / 11 gross flips to fail (net +11)

Diffed every new per-category report against `20260914T154459Z-ekos-full` scenario-by-scenario:

**Flipped to pass (22):** `code-001, code-005, code-006, code-009, code-011, code-012, code-015`
(C-cite + B2), `arch-002, arch-007, arch-016` (B1 + B2 + C-cite), `dep-007, dep-009` (C-cite),
`lin-007` (D-adjacent), `hist-001` (D1), `sec-001, sec-003, sec-009, sec-011` (C-cite + E-wording +
D1), `adv-010, adv-011, adv-016, adv-017` (E-wording).

**Flipped to fail (11):** `code-003, code-008` (citation-format drift / a name-precision miss on
"kir" vs "kirobject" tokenization — see Knowledge Captured), `arch-006, arch-014` (not yet
individually diagnosed — both were borderline passes at baseline), `lin-002`, `hist-003, hist-009`
(citation-format drift, same shape as `code-003`), `adv-007, adv-013, adv-015, adv-018` (a real
fabrication-vs-refusal trade-off — see Knowledge Captured).

### Per-category before → after

| Category | Before | After | Δ |
|---|---|---|---|
| architecture | 8/20 | 9/20 | +1 |
| code | 3/15 | 8/15 | +5 |
| dependencies | 8/12 | 10/12 | +2 |
| lineage | 5/12 | 5/12 | 0 |
| history | 4/12 | 3/12 | **−1** |
| security | 3/12 | 7/12 | +4 |
| adversarial | 11/18 | 11/18 | 0 (fully recomposed — see below) |
| **Total** | **42/101** | **53/101** | **+11** |

### Aggregate metrics

| Metric | Before | After |
|---|---|---|
| Answer correctness | 49.7% | 50.1% |
| Evidence groundedness | 51.6% | **65.9%** |
| Completeness | 46.4% | 50.1% |
| Recall@10 | 47.1% | **64.7%** |
| Hallucination count | 8/101 | 7/101 |

Groundedness and recall moved the most, as expected — C-cite is squarely a groundedness fix, and
B2 (plus B1's richer evidence) is squarely a recall fix. Answer correctness barely moved, which is
also expected: these fixes target citation/evidence-rendering mechanics, not the model's raw
factual recall.

---

## Knowledge Captured

- **A single background `ekos eval run` process for the full 101-scenario suite got OOM-killed
  twice on this host** (`free -h`: 181Mi free of 15Gi, 22Gi/73Gi swap in use) — this is a real,
  recurring environmental constraint on this machine, not a fluke. **Workaround: run
  `--category <name>` scoped, sequentially, in the foreground.** Each of the 7 categories (12-20
  scenarios) completes well inside a single foreground call, and starting a fresh process per
  category means memory is released before the next one starts. Costs manual aggregation of 7
  report files afterward (no built-in merge) but is fully reliable.
- **Ollama's disk cache survives a process being OOM-killed mid-run.** The first killed background
  run had already made real LLM calls (under the already-updated Phase 1 code) for a chunk of the
  `architecture` category before dying; the subsequent foreground `--category architecture` run
  showed `Cache hits: 18/18` — the killed run's partial progress wasn't wasted, it was persisted to
  `.ekos/llm-cache/` and simply replayed. Don't assume a killed background eval run produced zero
  usable output; check cache-hit counts on the retry before assuming a from-scratch cost.
- **Richer evidence for a loosely-related ("weak"/partial-overlap) claim can make a model more
  willing to fabricate, not less.** Before B1, a weak hit's claim was just a bare name — too thin
  to build a fabricated narrative from. After B1, the same weak hit now carries up to 280 chars of
  real prose, and for 4 adversarial false-premise questions (`adv-007/013/015/018`) the model used
  that richer-but-irrelevant text to construct a plausible-sounding wrong answer instead of
  refusing. This is a genuine, measured side effect of enriching *all* Search/Graph claims
  uniformly — a future pass might consider excluding `weak: true` hits from excerpt enrichment
  specifically, trading some legitimate-answer recall for adversarial safety. Not done here;
  recorded as an open option.
- **Prompt header style can bleed into unrelated output formatting.** Switching
  `REASON_SYSTEM_PROMPT`'s paragraph headers from ALL-CAPS (`CITING.`) to a bracketed style
  (`[Guidance on citing]`) coincided with 3 scenarios (`code-003`, `hist-003`, `hist-009`) where the
  model wrote `Cited evidence: [...]` in prose instead of the required `{"cited_evidence": [...]}`
  JSON object — despite the instruction text itself being unchanged in substance. A prompt's
  incidental *stylistic* choices (not just its explicit instructions) appear to influence how
  strictly a small local model follows an unrelated formatting rule elsewhere in the same response.
  Worth keeping in mind before making further stylistic changes to this prompt.
- **The two D5-suspected dataset bugs were not real** — see "Implementation details" above.
  Verifying against the live ledger before editing a dataset entry caught this before an unneeded
  (and actually name-confusing) edit was made.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/runtime/src/ai.rs` | `REASON_SYSTEM_PROMPT` header rewrite (E-wording), `extract_citations`/`resolve_citation_ref` claim-position + prefix tolerance (C-cite), `REASON_PROMPT_VERSION` bumped to v3, 3 new tests |
| `ekos/crates/runtime/src/reason.rs` | `entity_item_with_excerpt`/`excerpt_of`/`CLAIM_EXCERPT_MAX_CHARS` (B1), wired into `Search`/`Graph` arms of `exec_node`, 7 new tests |
| `ekos/crates/evals/src/evaluators/groundedness.rs` | Widened `DEFAULT_REFUSAL_PHRASES`, exposed `looks_like_a_refusal` (E-wording, D1) |
| `ekos/crates/evals/src/evaluators/answer.rs` | `matched_count` treats a refusal as zero matches (D1), 2 new tests |
| `ekos/crates/evals/src/evaluators/mod.rs` | `claim_prose_only` strips location/evidence-id suffixes before attribution (D4), 4 new tests |
| `ekos/crates/evals/src/runners/retrieval_runner.rs` | `strip_generic_terms`/`RETRIEVAL_GENERIC_TERMS` (B2), 4 new tests |
| `evals/datasets/dependencies.yaml` | Real, ledger-verified `expected_facts` added to `dep-004`/`dep-006`/`dep-007` (D3) |
| `evals/reports/2026091{4,5}T*-ekos-full.json` | 7 new category-scoped reports with saved answers, this measurement |
