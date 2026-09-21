# Public-communication guardrail check — session memory (RFC 0151, Phase 8)

**No article, post or announcement has been drafted.** The plan requires eval numbers first, and the
only numbers that exist are a deterministic proxy. This file is the checklist any future draft must
pass, with the current status of each claim it might want to make.

| Candidate claim | Shipped? | Measured? | Allowed wording today |
|---|---|---|---|
| EKOS has an opt-in, redacted, capped session-note inbox | yes (tests) | n/a | "EKOS can store agent notes locally, opt-in." |
| Notes are anchored to compiled objects and flag when the object changes | yes (tests; real-pipeline demo) | demo only | "…flags a note when the anchored table's columns change." |
| Session memory beats native compaction | **not shown in general** | 7 notes: 0.92 vs 0.92, no advantage. 121 notes, same budget: scoped 1.00 vs 0.16 — but unscoped 0.16, i.e. no better than no memory | **Do not claim generally.** May say: *at 121 notes a scoped lookup answered what a same-budget summary could not.* Must add that the fixture's facts are arbitrary numbers, maximally hostile to summarisation. |
| An unscoped session-start brief is useful at volume | **no — measured negative** | 121 notes: 0.16, identical to no memory | Say so plainly; the win is retrieval with a scope, not a brief. |
| Stale notes are flagged in the answer, not just the brief | yes | 5 of 6 eligible answers flagged; was 0 of 24 before the fix | Claimable for haiku; no sonnet data in the corrected run. |
| Memory cannot be poisoned | **false** | – | State the residual risk: pattern redaction, append-only ledger, notes are unverified data. |
| Agents cannot confirm their own notes | yes (no MCP promotion tool; guard test) | test | "Only a person can promote a note." |
| Works inside Claude Code hooks | `SessionStart` injection yes (one live canary run) | one run, one prompt | "A SessionStart hook can load a session brief; timeout/failure behaviour not tested." |
| Fingerprint staleness is accurate | partial | 7,701 flips / 9,004 raw changes on one ledger; no ground truth | Do not quote a false-flag rate. |

Framing rule: complementary to native memory and generic memory services, never "better than".
Every number in a draft must trace to a committed report under `docs/evals/`.
