# Devlog 62 — The first published benchmark number

**Date:** 2026-08-20
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

EKOS had never published a benchmark number, despite every comparable tool in this space
(codegraph, codebase-memory-mcp, codemap, CoreStory, GitNexus, Code Grapher, KiroGraph, Graphify,
Aegis) leading with one — tokens saved, ingestion time, query latency. This session built and
published a first, real, reproducible one against the same real repo (`analytics/`, Plausible
Analytics) the full-loop case study already uses: two real questions, answered from raw source
(three honest grep tiers) vs. from the compiled ledger (real MCP calls), both sides counted with a
standard tokenizer, not a hand-rolled estimate. Headline result: 67.5-93.4% fewer tokens than
realistic/naive raw-source search — with one case grep wins, published rather than hidden.

---

## Methodology

Two real questions against the real, unmodified `analytics/` checkout already used throughout
devlog_59-61:

1. **"What columns does the `imported_browsers` table have?"** — a single-object lookup.
2. **"What tables exist in the Postgres schema?"** — an enumeration/aggregation.

For each, the raw-source cost was measured at three tiers, all real commands against the real
repo, not synthetic:
- **Best-case**: the agent already knows the exact file and line — `grep -A15` directly on the
  known `CREATE TABLE` statement.
- **Realistic**: the agent doesn't know which of the real matching files has the answer, so it
  greps the whole repo with context lines — the way an agentic coding tool actually behaves. 32
  real files matched `imported_browsers` across this repo; all 32 were included.
- **Naive**: the agent found the one correct file (via the realistic tier) and reads it whole.

The EKOS-side cost included the same "doesn't know where to look yet" step: a real
`ekos_search` MCP call to find the object, plus a real `ekos_state` call to fetch its full
compiled state — not just the final answer counted in isolation, which would have been an unfair
comparison favoring EKOS. For the enumeration question, a single `ekos_ekl` structured query was
the whole EKOS-side cost, since there was no object-id lookup step needed.

Both sides were tokenized with `tiktoken`'s `cl100k_base` encoding (a standard, publicly
documented reference tokenizer used across the industry for GPT-4-class token estimates) — chosen
specifically to avoid a hand-rolled `chars/4` or `words*1.3` heuristic that a skeptic could
reasonably dismiss as made up.

## Results

| Question | Raw-source tier | Tokens | EKOS | Tokens | Reduction |
|---|---|---:|---|---:|---:|
| Q1: `imported_browsers` columns | best-case grep | 122 | search+state | 1,186 | **EKOS costs 9.7× more** |
| Q1: `imported_browsers` columns | realistic grep (32 real hits) | 10,357 | search+state | 1,186 | 88.5% fewer |
| Q1: `imported_browsers` columns | naive (whole file) | 3,651 | search+state | 1,186 | 67.5% fewer |
| Q2: enumerate Postgres tables | naive (whole 2,738-line schema) | 18,995 | ekl query | 1,258 | 93.4% fewer |

The best-case grep result is the honest headline of this whole exercise: **if the agent already
knows exactly where the answer is, nothing beats a direct grep — there was nothing left to
discover.** That's not a realistic starting condition for "what does this table contain?" against
a repo the agent hasn't already memorized (32 real files in this repo mention
`imported_browsers`; none of them are the schema file's own name), which is why the realistic and
naive tiers are the number worth leading with, not the best-case one. Publishing only the
best-case comparison would have been the easy, flattering number and a materially misleading one.

## Published as

`docs/presentations/token-benchmark.html` — same slide format and "real, unedited, every claim
linked to a real transcript" convention every other deck in this repo follows, including the one
slide (§04) dedicated entirely to the case where grep wins. Backed by every raw command, every
real MCP JSON-RPC response, and the exact `count-tokens.py` script used, all under
`docs/presentations/examples/token-benchmark/` — reproducible by anyone with the same public repo
and `pip install tiktoken`.

Linked from three places competitors' benchmark claims are typically seen first: the site hero
(above the fold, `docs/index.html`), the "Proven, not promised" stat-grid (same page, further
down), and `README.md`'s intro paragraph — not buried only in the deck listing.

## Explicitly not claimed

- **No comparison against any named competitor's own published numbers.** None of
  codegraph/codebase-memory-mcp/codemap/CoreStory/GitNexus/Code Grapher/KiroGraph/Graphify/Aegis's
  actual benchmark methodology or results were reproduced or verified — this deck measures EKOS
  against raw source, not against another tool's claim.
- **No latency/wall-clock-per-query number** — this pass measured token cost only.
- **No claim of statistical breadth** — two questions, one repo. More questions and more repos is
  exactly what "rough" is meant to signal, and is natural follow-on work.

---

## Knowledge Captured

- **A benchmark that only shows its best case is marketing, not evidence — and this project's own
  credibility depends on not doing that**, consistent with every other "honest, not hidden"
  deck already in this repo. The single most persuasive slide in the new deck is arguably §04 (the
  case grep wins), not the two "EKOS wins" slides — a benchmark that survives showing its own worst
  result is a stronger claim than one that doesn't.
- **Fair token-accounting requires charging both sides for "finding" the answer, not just
  "having" it.** An early draft of this benchmark counted only `ekos_state`'s response (581
  tokens) against grep's exploration cost, which would have overstated EKOS's advantage — the
  agent doesn't know the object's id any more than it knows which file has the schema. Adding the
  real `ekos_search` lookup step (605 tokens) to the EKOS side was the fix, and is the reason the
  final number is 1,186 tokens, not 581.
- **`tiktoken` (cl100k_base) is available in this environment and is a meaningfully more credible
  choice than a hand-rolled token estimate for any claim meant to be scrutinized** — worth
  reaching for by default whenever a token-cost number is being published, not just this once.

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/presentations/token-benchmark.html` | New deck: the first published benchmark, real repo, real tokenizer, one honest exception |
| `docs/presentations/examples/token-benchmark/*` | Real commands, grep outputs, MCP JSON-RPC responses, and the token-counting script backing every number in the deck |
| `docs/presentations.html`, `docs/index.html` | New deck listing entry; hero-level and "Proven, not promised" stat-grid now lead with the benchmark number |
| `README.md` | New benchmark callout in the intro, above `## About` |
| `TODO.md` | New tracked, done item for the benchmark; two stale `2,045-file` references (superseded by the corrected `2,022`) fixed in passing |
