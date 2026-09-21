# session-continuity eval v0 — 2026-09-21 (RFC 0151, Phase 5)

Reproduce: `ekos session eval --runs 5` (deterministic, no LLM, no network).

## What this is — and is not

A **deterministic proxy** for a two-session workflow. "Session B" is an extractive answerer over
whatever context each condition provides, so the numbers measure the *memory layer* (what survives,
what is flagged stale, what leaks), not any model's behaviour. The **compaction baseline is a stated
model** — keep the last 4 notes, truncated to 60 chars, no anchors/rationale, no staleness signal —
not a measurement of native `/compact`. A headless `claude -p` comparison against the real baseline
was **not run** (it is a metered call and needs an explicit go-ahead). The GO below is conditional.

## Fixture

7 session-A notes (4 anchored to tables, 2 unanchored, 1 prompt-injection attempt) and 8 questions
(6 answerable, 2 negative controls whose answer is in no session). After session A the tables
`customers` and `orders` change. `customers` is included because the compaction model still holds its
note; where the baseline has already forgotten a note it cannot serve it stale.

## Results (5 runs, fresh ledger each; mean ± sd)

| condition | correct | grounded | cite cov. | stale served | poison leak | brief tok | µs/q |
|---|---|---|---|---|---|---|---|
| no memory | 0.25 ±0.00 | 0.00 ±0.00 | 0.00 ±0.00 | 0.00 ±0.00 | 0.00 ±0.00 | 0 | 0 |
| compaction baseline (model) | 0.62 ±0.00 | 0.00 ±0.00 | 0.00 ±0.00 | 0.50 ±0.00 | 1.00 ±0.00 | 56 | 44 |
| EKOS session memory | 1.00 ±0.00 | 0.75 ±0.00 | 1.00 ±0.00 | 0.00 ±0.00 | 0.00 ±0.00 | 317 | 2269 |
| EKOS, anchors then changed | 1.00 ±0.00 | 0.75 ±0.00 | 1.00 ±0.00 | 0.00 ±0.00 | 0.00 ±0.00 | 337 | 2357 |

Zero spread is expected: the harness is deterministic. It shows reproducibility, not statistical power.

## Metric notes

- **correct**: the answer contains the ground-truth phrase, or a negative control is refused.
- **grounded**: answered *and* correct *and* cited (uncited answers are penalised, not excluded).
- **stale served**: a note about a *changed* table answered with no staleness flag.
- **poison leak**: a prompt-injection note reaches context outside the untrusted envelope or labelled
  above `T0`. For the compaction model this is 1.0 *by construction* (no tiering, no envelope); that is
  a structural property of the model, not an observed failure of `/compact`.
- **The metric can fail**: `a_deliberately_broken_retrieval_makes_the_score_drop` (a retriever that
  always returns note 0) cuts correctness from 1.00 to about 0.12.

## Findings the eval produced (fixed, not tuned away)

1. Recall had no stopword handling: "the" alone made both negative controls match a note, so the
   explicit "no relevant session memory" result never fired (EKOS correctness 0.50 vs baseline 0.62).
2. Recall had no stemming: "run" matched the injected note as strongly as "reconciliation … runs".
   Both are fixed in `ekos-session/src/read.rs`.
3. The first fixture changed `orders`/`payments`, whose notes the compaction model had already
   forgotten, so it could not serve them stale and the stale comparison was 0 vs 0 (NO-GO). The
   fixture, not the system, was the flaw; it now changes a table the baseline still remembers.

## Fingerprint noise on a real ledger (RFC Open Question 3)

`ekos session fingerprint-noise` on this repo's own ledger (40,280 consecutive object-version pairs):

| | before tuning | after tuning |
|---|---|---|
| pairs where any property differs | 9,004 | 9,004 |
| pairs where the fingerprint flips | 8,208 | 7,701 |

Largest remaining causes: `Section.excerpt` (10,260 — documents genuinely edited), `RustSymbol.signature`
(2,095 — real signature changes), `Section.heading`. Tuning removed line-range and derived-metadata
(`doc_type`, `rfc_*`, `size_bytes`) flips. **Limitation:** there is no ground truth for "should have
flipped", so this measures suppression of non-projection churn, not a false-flag rate. No `Table` key
appears in the top causes: tables in this ledger rarely change.

## GO / NO-GO

correctness EKOS 1.00 vs baseline 0.62; stale-fact-served EKOS 0.00 vs baseline 0.50 → GO

**Decision: GO to Phase 6 on the proxy result, conditional on a live `claude -p` run before any
public claim.** Nothing here supports "session memory beats native compaction in practice"; it
supports that the memory layer has the properties the design needs.
