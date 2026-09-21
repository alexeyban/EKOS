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

---

# Live run (added 2026-09-21) — supersedes the proxy GO

Reproduce: `python3 demo/session-memory/live_eval.py <ekos-binary> <workdir> --runs 3 --model haiku`
(real `claude -p` calls, metered). Raw answers: `session-continuity-live-2026-09-21.json`.

## Setup

Same 7 notes and 8 questions, on a real built ledger (`ekos build … commit`, four tables). Every
condition gets its context through the same channel (`--append-system-prompt`), the same instruction
("answer only from the earlier-session context, else `NONE`"), the same grader, model `haiku`,
tools disabled, 3 runs. Conditions: **none**; **compaction** = a *real model-written* 4-line summary of
the same notes (a proxy for `/compact` — native `/compact` cannot be driven from `-p`); **ekos** =
`ekos session brief`; **ekos_changed** = the brief after `customers` and `orders` gained columns and
the ledger was rebuilt. 96 calls per run.

## Results (clean run; mean ± sd over 3 runs)

| condition | correct | stale-fact served (customers/orders questions) | injected-note leak |
|---|---|---|---|
| none | 0.25 ±0.00 | n/a | 0.00 |
| compaction (model-written summary) | 0.92 ±0.06 | 0.83 ±0.24 | 0.00 |
| ekos | 0.92 ±0.06 | n/a (nothing had changed) | 0.00 |
| ekos, anchors changed | 0.88 ±0.00 | 0.67 ±0.24 | 0.00 |

"Stale-fact served" = the question is about a changed table, the model answered rather than said `NONE`,
and its answer contains none of *chang / outdated / stale / orphan / moved / no longer*.

## What this shows — and does not

- **No correctness advantage.** 0.92 vs 0.92 (0.88 after the change). At 7 notes the compaction summary
  keeps every fact in four lines. My proxy's "1.00 vs 0.62" compared EKOS against a deliberately weak
  model (last 4 notes, truncated); a real summary is not that weak.
- **Staleness: not established.** 0.67 vs 0.83 is within run-to-run spread (n = 3 runs × 2 questions).
  The brief did carry `[CHANGED]` and the model sometimes hedged or answered `NONE` on changed notes,
  but this run cannot separate that from noise.
- **Injected note: no leak in any condition**, including compaction (the summariser flagged the injection
  and dropped it). The proxy's "compaction leaks by construction" was wrong in practice.
- Under the plan's own rule (beat the baseline on correctness **and** stale-fact-served) this is a
  **NO-GO at this scale and model**. It does not show session memory is worse; it shows this fixture
  cannot show it is better.

## Things that went wrong in the harness (disclosed)

1. **Run 1 was contaminated and is not used.** `claude -p` loaded my MCP servers even with tools
   disabled; some answers were "grant access to the Serena tools". Fixed with `--strict-mcp-config`
   and re-run (this is the reported run). Run 1 had shown a striking 0.17 stale-served for EKOS — that
   number is an artefact and should not be quoted.
2. **The first staleness grader was biased toward EKOS.** It counted "unconfirmed/unverified" as a
   staleness flag, but the T0 tier label makes the model say "unconfirmed" whether or not anything
   changed. Regraded offline with the stricter word list above (loose grader gave 0.50 for
   ekos_changed; strict gives 0.67). The strict figure is reported.
3. Grading is substring/`NONE` matching, so paraphrase can score as wrong (one compaction answer
   "deduplicated by hash, same email can appear multiple times" was judged incorrect, arguably fairly).

## What would test the actual hypothesis

Compaction's weakness is volume and drift, not a 7-note fixture: rerun with dozens to hundreds of notes
against a fixed summary budget, more than one model (sonnet), more questions per table, and enough
repeats to give the staleness metric a usable confidence interval. Until then the only claims the data
supports are the mechanical ones (notes are anchored, staleness is flagged in the brief, no promotion by
agents), not that this beats native compaction.
