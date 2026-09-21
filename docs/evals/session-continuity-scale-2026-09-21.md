# session-continuity eval at scale — 2026-09-21 (RFC 0151)

Supersedes the 7-note run in `session-continuity-2026-09-21.md`, which was too small to
discriminate. Raw answers: `session-continuity-scale-2026-09-21.json`.

Reproduce: `python3 demo/session-memory/live_eval.py <ekos-binary> <workdir> --runs 2 --models
haiku --notes 120 --tables 40 --budget 400 --questions 16` — 191 metered `claude -p` calls.

## Setup

121 notes (120 anchored across 40 tables, plus one prompt-injection note) committed to a real
built ledger. 19 questions: 16 whose answer is a specific value in exactly one note, 3 negative
controls whose answer is in no note. **Every condition is held to the same 400-token budget** and
gets its context through the same channel, with the same instruction and grader.

`ekos, no scope` is a session-start brief with nothing declared. `ekos, scoped` passes the
question's table as `--scope`. `compaction` is a real model-written summary of all 121 notes
capped at the shared budget — a proxy, since native `/compact` cannot be driven from `-p`.

## Results (haiku, 2 runs, mean ± sd)

| condition | ctx tok | correct | refused | stale served | over-hedge | leak |
|---|---|---|---|---|---|---|
| none | 0 | 0.16 ±0.00 | 1.00 ±0.00 | — | — | 0.00 |
| compaction (model-written, same budget) | 310 | 0.16 ±0.00 | 1.00 ±0.00 | n/a (answered nothing) | n/a | 0.00 |
| ekos, **no scope** | 352 | 0.16 ±0.00 | 1.00 ±0.00 | n/a (answered nothing) | n/a | 0.00 |
| ekos, scoped | 353 | **1.00 ±0.00** | 0.16 ±0.00 | 1.00 ±0.00 | 0.00 ±0.00 | 0.00 |
| ekos, scoped, anchors changed | 384 | **1.00 ±0.00** | 0.16 ±0.00 | **0.17 ±0.00** | 0.00 ±0.00 | 0.00 |

## Reading these numbers

**The staleness fix works.** This was the point of the run. In the 7-note eval the `[CHANGED]`
marker produced a flag in **0 of 24** answers. Here, once the anchored tables changed, 5 of 6
eligible answers both answered *and* said the note may be out of date — *"The tbl_11 loader
retries 2153 times before paging someone, though this note may be stale since the tbl_11 columns
have changed since the note was written."* `stale_served` 1.00 → 0.17. The one that did not flag
was `tbl_02`, identically in both runs. Over-hedging on unchanged tables stayed at 0.00, so the
flag did not generalise into blanket hedging.

**At 121 notes, summarisation retained none of the specific facts.** Compaction refused all 38
factual questions; its 0.16 is the 3 negative controls it refused correctly. Its summary
generalised — *"retry thresholds: each table has a configurable loader retry count (1,100–9,800
retries)"* — which is a reasonable compression and useless for "how many times does tbl_11 retry".

**But the whole win comes from scope, not from memory.** The unscoped brief scored 0.16 —
*identical to having no memory at all*. At 121 notes a 400-token brief holds ~12 of them, and
without a scope the ranking has no idea which 12 matter. This is a **retrieval** result, not a
session-brief result: it says a targeted lookup beats a summary, which is closer to what
`ekos_session_recall` does than to what a session-start brief does.

**`stale_served` = 0 is not a virtue for the first three rows.** They answered nothing, so they
served nothing stale. The metric only carries meaning for a condition that actually answers.

## Limits — read before quoting any of this

- **The fixture is adversarial to summarisation by construction.** Each fact is an arbitrary
  4-digit number, i.e. maximally incompressible and individually required. Real session notes are
  redundant and structured, so a real summary would fare better than 0/16. This measures the
  regime where facts are numerous, specific and individually needed — a real regime, but not the
  only one, and the opposite regime (7 notes) showed no advantage at all.
- **haiku only, 2 runs.** No sonnet comparison in this run; the earlier contaminated run suggested
  over-hedging is model-dependent (0.40 haiku vs 0.00 sonnet there).
- **`--scope` ranks, it does not filter.** Every scoped brief still carried unrelated `CHANGED`
  lines (12/12 in the first run). That is correct for a session brief but adds noise.
- A first run of this eval (572 calls) is **discarded**: a fixture bug keyed the note template on
  the note index rather than the row, so all 3 notes for a table shared one template with
  different values. Every question then had 3 contradictory answers and any answer listing them
  all scored correct. Both the EKOS and compaction columns were contaminated. Fixed in
  `make_fixture`; the discarded run's staleness, over-hedge and leak findings were unaffected by
  it, and its 0.40 haiku over-hedge is worth remembering.

## Verdict

The RFC 0151 go/no-go rule asks whether session memory beats the baseline on correctness **and**
stale-fact-served. On this fixture, scoped: yes on both. Unscoped: no on either.

Honest claim: *at 121 notes, a scoped lookup into anchored session memory answered questions a
same-budget summary could not, and flagged 5 of 6 answers whose anchor had moved.* Not claimable:
that session memory beats native compaction generally, that an unscoped session brief is useful at
this volume, or anything about models other than haiku.
