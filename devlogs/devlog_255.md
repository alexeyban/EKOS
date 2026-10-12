# Devlog 255 — Refusal prompt v5 and a grader that read a refusal out of the third paragraph

**Date:** 2026-10-12
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

RFC 0177 left three refusal defects on `deepseek-v4.1-flash`: two correct denials that used neither fixed opening, and one real question answered `Premise not supported.` The reason prompt now ties each opening to the outcome. All 20 refusal scenarios pass, and the suite is at 98 of 101.

The run also showed a grader defect: a complete, cited answer scored 0 because it contained "no such". Fixed as ruler v7; both cloud runs were re-graded under it.

---

## Prompt v5

### Problem / motivation

| Scenario | v4 answer | Cause |
|---|---|---|
| `adv-001`, `sec-003` | A correct denial in the model's own words | The refusal was limited to one sentence. A model with context to give did not use the form |
| `arch-015` | `Premise not supported.` on a real question | Nothing limited the opening to a question that depends on the unsupported part |

### What was built

| Change | Where |
|---|---|
| "It is not in the claims" is a refusal and takes the refusal opening; cited context may follow | `runtime/src/ai.rs::REASON_SYSTEM_PROMPT` |
| A name that appears in no claim is not described by a similar one | same |
| The premise opening only when the question cannot be answered without the unsupported part | same |
| A question that asks whether something is so gets yes or no | same |
| Prompt version `ai-runtime-reason-v5` | same |
| Test: each opening appears once, alone on its line | `evals/src/evaluators/groundedness.rs` |

### Measured

`deepseek-v4.1-flash`, same suite and ledger, both graded under ruler v7:

| | Prompt v4 | Prompt v5 |
|---|---|---|
| Scenarios passed | 96 / 101 | 98 / 101 |
| Refusal scenarios passed | 18 / 20 | 20 / 20 |
| Answer correctness | 97.4% | 94.7% |
| Evidence groundedness | 95.6% | 100.0% |
| Answered but uncited | 2 | 0 |
| Refusals counted | 19 | 24 |

Passing now: `adv-001`, `sec-003`, `sec-002`, `dep-001`, `dep-009` (the last by ruler v7).

Failing now:

| Scenario | Cause |
|---|---|
| `code-004` | The edition is not in the evidence. v5 says so under `Insufficient evidence.`; v4 said so in its own words and passed on the keyword `2024` |
| `lin-004` | Same evidence; the answer says "span", the expected word is "location" |
| `arch-015` | Still `Premise not supported.` The scenario says the simulation crate stays separate from a ledger primitive. The sources say it is separate from the compiler pipeline and writes to the ledger |

Local `llama3`, 30 adversarial and security scenarios, wording before the yes/no sentence: refusal scenarios 13 → 18 of 20. `adv-007` and `adv-014` still invent an answer. `sec-002` was answered correctly under `Premise not supported.`, which is what the yes/no sentence was added for; the cloud run passes it.

The full local run with the final wording was started and stopped for lack of memory about 20 scenarios in. It writes its report at the end, so nothing was kept.

### Decisions

- **No scenario was edited.** `arch-015` has a wrong premise and the model is right to reject it. It is recorded as an open item, not fixed in the same change that is being measured.
- **The yes/no sentence went in from one local failure, unmeasured locally.** The cloud run is its only measurement.

---

## Ruler v7

`sec-010` was a full answer with five citations. Its third paragraph says "The claims show no such allowlisting." `no such` is in the refusal phrase list, the answer evaluator searched the whole text, and a refusal states no fact: answer score 0.

Where a scenario expects an answer, the grader now takes only an answer that begins as a refusal: a phrase in the first 300 characters (`groundedness::opens_with_a_refusal`). 300 leaves room for a small model that repeats the question first. The `should_refuse` path still searches the whole answer.

Effect on saved reports: one scenario each in the two cloud runs (`dep-009`, `sec-010`), none in the 2026-10-11 local run.

---

## Knowledge Captured

- **A refusal form that forbids context will be skipped by a model that has context.** Letting cited detail follow the fixed opening moved both denials into the form.
- **A pass can be an accident of the keyword.** `code-004` passed under v4 with an answer that said the edition is not stated. Read the answer before counting a lost pass as a regression.
- **A phrase list searched over a whole answer grades long answers worse than short ones.** The longer the answer, the likelier it holds "no such" or "not mentioned" somewhere.
- **The Ollama "probably truncated" warning fires on every call here.** `likely_truncated` is `prompt + num_predict >= num_ctx`, and `[ai] max-tokens = 8192` equals the window. A 3,300-token prompt is reported as truncated. Read `prompt_tokens` in the line, not the message.
- **A background run on this machine can be stopped for memory.** `llama3` holds about 6 GB of 15. `ekos eval run` keeps results in memory until the end, so a stopped run leaves nothing.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/runtime/src/ai.rs` | Reason prompt v5 |
| `ekos/crates/evals/src/evaluators/groundedness.rs` | `opens_with_a_refusal`, three tests |
| `ekos/crates/evals/src/evaluators/answer.rs`, `mod.rs` | Use it where an answer is expected |
| `ekos/crates/evals/src/report.rs` | Ruler v7 |
| `ekos/docs/rfcs/0177-eval-followup-retrieval-refusals.md` | Amendment |
| `devlogs/devlog_254.md` | Truncation sentence corrected |
| `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Documented |
