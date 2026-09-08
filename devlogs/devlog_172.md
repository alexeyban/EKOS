# Devlog 172 — RFC 0139 continued: an honest ruler, and closing the fabrication regression

**Date:** 2026-09-08
**Commits:** `9c7252f`, `3dd2ba4`, `2abb959`, `38adfce`, `e0ad385`, `e24393d`, `ebd26b2`, `502d164`, `22fe559`, `43f352c`
**Branch:** main (local)

---

## Summary

devlog_171 left two things open: a grading ruler that was measurably unfair in both directions, and
a fabrication regression (10 → 15 of 101) introduced by RFC 0139 §3.1's search relaxation. Both are
now resolved.

The ruler went through three versions and the honest baseline it produced is **worse** than the one
originally published — `48/101` was inflated; the same answers score **31/101** under a ruler that
stops awarding credit it hadn't earned. The fabrication regression took **six attempts**; five
failed the same way, and the sixth worked by doing the opposite of all of them.

Final state against the original system, all measured under the same ruler:

| | passed | answer correctness | groundedness | fabricated |
|---|---|---|---|---|
| original | 31/101 | 36.2% | 39.6% | 10 |
| **shipped** | **39/101** | **42.5%** | **44.0%** | **3** |

---

## `ekos eval regrade` — the audit trail (`9c7252f`)

### Problem

Changing how answers are graded makes new numbers non-comparable to old ones. Without a way to
re-score *old answers under a new ruler*, a grading change and a real improvement are
indistinguishable in a trend table — and the temptation is to read whichever interpretation
flatters the work.

### What was built

`ekos eval regrade <report.json>` rebuilds each scenario's `ScenarioRun` from the transcripts
Phase 0 saved and re-runs the current evaluators — **offline, zero LLM calls**. Verified to
reproduce R0 exactly (48/101, 37.6/78.3/36.8/65.0) before being trusted for anything.

That property is itself a test: a regrade that drifts would invalidate every v1-vs-v2 comparison
built on it.

---

## The ruler: v2, v3, v4 (`3dd2ba4`, `2abb959`, `e0ad385`)

Every number below is over **identical saved answers**, so each delta is grading and nothing else.

| ruler | passed | answer | groundedness | recall@10 |
|---|---|---|---|---|
| v1 (RFC 0138 as shipped) | 48/101 | 37.6% | 78.3% | 65.0% |
| v2 — normalisation + `any_of` | 48/101 | 37.1% | 78.3% | 65.0% |
| v3 — composite + groundedness | 31/101 | 37.1% | **39.6%** | 65.0% |
| v4 — dataset hygiene | 31/101 | 36.2% | 39.6% | 59.1% |

**v2 went *down*, and that inverted the RFC's premise.** §2.1 was written to fix false negatives,
and it fixed two (`sec-001` "redacted" vs `"redaction"`, `lin-005` "tombstones" vs `"tombstone"`).
But it also removed **three false positives** substring matching had been awarding:

| scenario | key | matched inside | the answer |
|---|---|---|---|
| `arch-006` | `runtime` | "Ai**Runtime**" | "AiRuntime.kind = RustSymbol" — wrong question |
| `code-008` | `kir` | "**Kir**Object" | names the wrong crate |
| `arch-002` | `compile` | "ekos-**compile**r-core" | nonsense |

The old ruler was not merely too strict; it was **also too loose, in the direction that flatters the
system**, and part of the published 37.6% was credit for wrong answers.

**v3 is where the pass rate collapsed**, and it is a correction, not a regression. Groundedness was
`valid / cited` — the same formula as citation *precision*. At zero citations that is 0/0, so it
returned `None`, and the scenario left the denominator instead of scoring zero. 59 of 101 answers
cited nothing, so 78.3% was a mean over 46 scenarios. Also fixed: `completeness` re-used `answer`'s
match count and both fed one unweighted mean, so a single missed keyword cost *two of three* slots.

**v4 removed three checks that could not fail.** `adv-014` accepted a bare `"not"` as proof of
refusal; `arch-010`/`lin-001`/`code-013` keyed on words appearing in 55-59% of *all* answers
regardless of correctness.

---

## The fabrication regression: six attempts (`ebd26b2`, `502d164`, `22fe559`, `43f352c`)

§3.1's relaxation raised answer correctness but pushed fabrications 10 → 15: relaxing the query
makes evidence sets non-empty for questions about things that do not exist.

| # | approach | adversarial | cost |
|---|---|---|---|
| 1 | mark relaxed search hits weak, refuse when all-weak | 15 (no change) | guard fired 2× in 101 |
| 2 | + mark planner-added neighbourhoods weak | 0 fabrications | `code` 72.7% → **18.2%** |
| 3 | + term-coverage grading (§3.7) | 0 fabrications | `code` 72.7% → **18.2%** again |
| 4 | §3.0 entity gate + all-weak refusal | 8 | −13.3pp answer, 17 legit refusals |
| 5 | entity gate + empty-only refusal | 14 | none — but barely helped |
| 6 | **+ refusal contract in the prompt** | **3** | 9 legit self-refusals |

Attempts 2, 3 and 4 all look like wins on their adversarial column alone. Each was reverted only
because the *other* direction was checked.

**What finally worked** was the opposite of the first five: stop making the system refuse on the
model's behalf, and tell the model the rubric. The v1 prompt said "if the evidence does not answer
the question, say so explicitly" while the evaluator graded refusals against a fixed list of 22
phrases — the model was marked against a rubric it had never been shown.

---

## Knowledge Captured

- **A metric defined as a ratio silently becomes "not applicable" at zero, and that is where the
  interesting failures live.** `valid/cited` is undefined at zero citations, so groundedness
  returned `None` and the scenario left the denominator. Precision is undefined at zero;
  groundedness is *zero*. Same arithmetic, different question. The dangerous property: misses left
  the denominator instead of entering it, so **the worse the model behaved, the better the score
  looked**.
- **`None`-means-not-applicable is safe for scenario *shape* and unsafe for run *outcome*.** A
  scenario with no `expected_objects` genuinely cannot be scored on recall@10 — a static dataset
  property. `cited == 0` is something the model did. Applying the same convention to both is the
  category error that hid this for a full release.
- **A ruler can be wrong in both directions at once.** The stated problem was strictness; the
  measured reality included three answers scoring 1.0 for matching a key *inside a longer
  identifier*. Fixing only the direction you expected leaves the other half.
- **Check the direction a metric doesn't measure before believing a fix.** Three separate changes
  reached a perfect adversarial score by refusing a third of legitimate questions. The adversarial
  column alone would have shipped every one of them.
- **Encoding a disproven rule into a prompt reproduces the failure exactly.** The first refusal
  contract told the model "if every claim is a weak match, refuse" — the same all-weak rule already
  rejected twice in code. It self-refused 59 of 83 legitimate questions. Same rule, same failure,
  new location.
- **Replace a judgement call with a checkable criterion.** "Refuse when the claims genuinely contain
  no answer" → 31 self-refusals. "Refuse only when no claim names or describes the thing asked
  about" → 9. Same intent, and the second is something a model can actually evaluate.
- **Diagnose before assuming a mechanism.** `AI001` jumped 4 → 72 and truncation was the obvious
  culprit; outputs had in fact got *shorter* (median 34 vs 106 tokens, none near the 1024 cap).
  Refusals legitimately cite nothing.
- **Tune a threshold from the corpus, not from taste.** Gated-neighbourhood sizes were sharply
  bimodal — 31 of 36 were 46-47 (the `ekos` hub, i.e. the whole crate graph), 5 were mid-size. A cap
  of 12 gated the 5 for no benefit; two thirds of the evidence budget targets the hub exactly.
- **`ekos eval run` exits non-zero when the *quality gate* fails**, so a shell loop using
  `&& echo OK || echo FAILED` reports every run as failed while writing perfectly good reports.
  Check for the output file, not the exit code.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/evals/src/regrade.rs` | New — offline re-grading, the ruler audit trail |
| `ekos/crates/evals/src/evaluators/normalize.rs` | New — deterministic token/word-ending folding |
| `ekos/crates/evals/src/evaluators/{answer,groundedness,retrieval,mod}.rs` | Ruler v2/v3: alternates, honest groundedness, weighted composite |
| `ekos/crates/evals/src/schema.rs` | `ExpectedFact` (`untagged`), two dataset guards |
| `ekos/crates/evals/src/report.rs` | `RULER_VERSION` 1→4, honest denominators |
| `ekos/crates/ledger/src/search.rs` | `ScoredHit` + per-term coverage |
| `ekos/crates/ledger/src/{retrieval,fact_ledger}.rs` | `WEAK_COVERAGE`, post-fusion annotation |
| `ekos/crates/runtime/src/reason.rs` | §3.0 entity gate (`RSN007`), `supporting` plan flag |
| `ekos/crates/runtime/src/ai.rs` | REASON prompt v2 + refusal contract, configurable |
| `evals/datasets/*.yaml` | Degenerate/near-free-pass keys replaced |
| `ekos/docs/rfcs/0139-answer-quality.md`, `TODO.md` | Measured results throughout |
