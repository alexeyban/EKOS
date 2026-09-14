# Devlog 182 — measuring today's RFC 0139/0140/0141 fixes: a real rebuild and eval run

**Date:** 2026-09-14
**PRs:** (local, not yet pushed) `docs: devlog_182 — real recover/compile/commit + eval measurement of today's RFC 0139/0140/0141 fixes`
**Branch:** main (local)

---

## Summary

Ran the thing every RFC shipped today said was still needed: a real `build`/`recover`/`resolve
--force`/`compile`/`commit` against EKOS's own workspace, then a full 101-scenario `ekos eval run
--agent ollama` to measure the actual effect of today's signature/Calls-attribute/search/recall/
rerank fixes. Headline composite scores (`answer_correctness` 49.7%, `evidence_groundedness`
51.6%) landed **bit-identical** to the last recorded baseline — not a coincidence, `[llm]`'s own
RFC 0008 contract requires every provider to run at `temperature: 0`, so unchanged prompts produce
unchanged completions. What actually moved: `recall_at_10` dropped 52.9% → 47.1%, and it traces to
exactly **one** scenario (`code-002`) whose recall flipped from a false 1.0 to an honest 0.0 —
which is RFC 0139 Phase 2's fix working exactly as designed: the metric used to grade the *wrong*
query and reported a perfect score it hadn't earned. Also found live: RFC 0141's signature fix
demonstrably improves raw lexical ranking (verified directly), but doesn't yet flip `code-002`
itself, because the REASON planner routes that exact question to a direct entity lookup that never
reaches the search path the fix improved — a real, separate, now-documented gap.

---

## What was run

```
ekos build       # 86 files observed (new/changed) — today's own code changes
ekos recover     # 2,676 Rust symbols (1,792 Calls edges), 47 Python files, 135 JS modules/2,519 symbols
ekos resolve --force   # 25 pre-existing cross-kind name conflicts, same class as devlog_177
ekos compile     # 12,455 objects, 17,831 relationships
ekos commit --yes      # [llm-description] temporarily disabled — see below
ekos eval run --dataset ekos-full --agent ollama --save-answers
```

`[llm-description]` (a post-`commit`, LLM-call-per-object pass, unrelated to anything shipped
today) was temporarily set `enabled = false` for this rebuild — identical reasoning and precedent
to the 2026-09-08 rebuild in devlog_173/174 — and restored to `true` immediately after. The actual
regeneration itself is still deferred, not run (same as it's been since devlog_174).

## Verified directly: the RFC 0141 signature fix works

```
$ ekos query object a83c0280-990b-556c-b375-0073c876d6af
Object: build_llm_provider (RustSymbol)
  Properties:
    signature: "pub fn build_llm_provider(config: &EkosConfig, artifact_dir: &Path) -> Arc<dyn LlmProvider>"
    symbol_kind: "function"
    ...

$ ekos query find "LlmProvider" --mode lexical
  35d66bca...  LlmProvider                     (the type)
  e25e2683...  llm_provider_check
  a83c0280...  build_llm_provider              <- was outside the top ten before this RFC
  824776cf...  select_llm_provider
```

`build_llm_provider` now ranks **#3** for a bare "LlmProvider" query. RFC 0141's own motivating
text said this exact function "appeared nowhere in the top ten" before the fix. This is real,
reproducible, and independent of anything below.

## The eval numbers

| Metric | Before (`20260909T102121Z-ekos-full-CLEAN`) | After (`20260914T154459Z-ekos-full`) |
|---|---|---|
| Passed | 43/101 | 42/101 |
| Answer correctness | 49.7% | 49.7% *(bit-identical)* |
| Evidence groundedness | 51.6% | 51.6% *(bit-identical)* |
| Completeness | 45.6% | 46.4% |
| Recall@10 | 52.9% | 47.1% |
| Hallucination rate | 5.9% | 7.9% |
| Fabrication rate (of `should_refuse`) | 0.3 | 0.4 |
| Uncited answers | 57 | 54 |

Status: `FAIL` against the suite's own gates, both times — expected, the gates target a much more
capable cloud model (README already documents this).

### Why `answer_correctness`/`evidence_groundedness` are bit-identical

`recovery/src/llm.rs`'s `LlmProvider` contract (RFC 0008) requires `temperature: 0` from every
implementation. Same prompt in, same completion out, for every scenario whose retrieved evidence
didn't change. The two composite scores are dominated by scenarios neither today's fixes nor the
corpus refresh touched, so they landed exactly where they were — a genuinely useful confirmation
that this measurement is comparing like-for-like, not noise.

### Why recall@10 "dropped" — it didn't, one scenario's number was wrong before

Diffing per-scenario `retrieval_recall` between the two reports: **exactly one** scenario
changed — `code-002` (*"What function builds the LlmProvider used by ekos ask..."*), from **1.0
before → 0.0 after**. That single swing accounts for essentially the entire aggregate move (1 of
the ~17 scenarios that carry a `retrieval_recall` value, and 1/17 ≈ the observed 5.8-point drop).

This is RFC 0139 Phase 2's fix doing exactly its job. Before today, `agent_runner.rs` graded
recall against the *raw* question sentence — a different, unrelated query from what the REASON
pipeline actually searched with — and that raw-question probe happened to retrieve
`build_llm_provider` for this scenario, reporting a false perfect score for a search the pipeline
never ran. After the fix, recall is graded against `search_query(understand(question))`, the
pipeline's *real* query — and it honestly reports 0.0, because:

```
$ ekos ask --explain "What function builds the LlmProvider used by ekos ask, ..."
routing confidence: (high)
plan: Fact LlmProvider.* 
```

The planner resolves "LlmProvider" as an exact-name entity match and routes to a direct
`Fact`/`Structural` lookup on the *trait*, never reaching `Search` at all — so `build_llm_provider`
(the function RFC 0141 made rankable) never gets a chance to surface for *this specific question*,
regardless of how good the underlying search ranking now is. The evidence shown to the model was
five facts about the `LlmProvider` trait itself (name, kind, description, span, symbol_kind) — not
the function. The model correctly refused rather than fabricate.

**This is a real, separate, newly-visible gap**: RFC 0141 fixed retrieval *ranking*; `code-002`
needed retrieval *routing* — the planner choosing `Search` over `Fact` when the resolved entity's
own facts don't actually answer the question asked. Not fixed here; recorded in TODO.md.

### The 13 scenarios that flipped pass/fail (net −1)

6 flipped to pass (`arch-004`, `arch-005`, `arch-006`, `code-003`, `lin-002`, `lin-005`); 7 flipped
to fail (`code-001`, `code-005`, `code-012`, `code-013`, `sec-001`, `adv-010`, `adv-014`). Spot-
checked the two adversarial flips: `adv-010`'s answer opens with "REFUSING." instead of the
prompt's required exact opening ("Insufficient evidence.") — a real refusal in intent, graded as a
fabrication because it didn't use the words the grader (and the prompt) both specify. `adv-014`
hedges ("could imply... might be possible") rather than committing either way. Neither traces
cleanly to a specific fix shipped today — both read as local-model wording variance on borderline
prompts, plausibly influenced by the richer evidence text (`signature`/`description` properties
now present) shifting prompt content, not a regression in the refusal-contract prompt itself
(untouched today). Worth a dedicated investigation, not concluded here.

---

## Knowledge Captured

- **A composite score landing bit-identical across two runs is a real, useful signal, not a
  suspicious one — check the provider's determinism contract before assuming a broken
  measurement.** `temperature: 0` is mandatory (RFC 0008); an unchanged prompt has no source of
  entropy left to produce a different completion from local ollama.
- **Fixing a metric to measure the right thing can make it look worse without anything having
  regressed.** `code-002`'s recall went 1.0 → 0.0 because the *old* number was never real — this
  is exactly the failure mode RFC 0139 Phase 2 set out to close, confirmed on the first real
  measurement after the fix landed.
- **A retrieval-ranking fix and a retrieval-routing gap are different bugs that look identical
  from the eval score alone.** `code-002` still fails after RFC 0141, but not because the fix
  doesn't work (`ekos query find` proves it does) — because the planner never asks the search
  index the question RFC 0141 made answerable. Both are real; only one was in today's scope.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos.toml` | `[llm-description]` restored to `enabled = true` after the measurement rebuild |
| `evals/reports/20260914T154459Z-ekos-full.json` | The new full-suite report, saved with answers/evidence text |
| `TODO.md` | RFC 0138/0139/0140/0141 "not yet measured" notes updated with these real numbers; new gap recorded (`code-002`'s planner-routing issue) |
