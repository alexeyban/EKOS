# Public-communication guardrail check — session memory (RFC 0151, Phase 8)

**No article, post or announcement has been drafted.** The plan requires eval numbers first, and the
only numbers that exist are a deterministic proxy. This file is the checklist any future draft must
pass, with the current status of each claim it might want to make.

| Candidate claim | Shipped? | Measured? | Allowed wording today |
|---|---|---|---|
| EKOS has an opt-in, redacted, capped session-note inbox | yes (tests) | n/a | "EKOS can store agent notes locally, opt-in." |
| Notes are anchored to compiled objects and flag when the object changes | yes (tests; real-pipeline demo) | demo only | "…flags a note when the anchored table's columns change." |
| Session memory beats native compaction | **not shown** | live run (haiku, 7 notes, 3 runs): correctness 0.92 vs 0.92; staleness difference within noise; a NO-GO under the plan's rule | **Do not claim.** May say only that anchored notes are flagged as changed in the brief. |
| Memory cannot be poisoned | **false** | – | State the residual risk: pattern redaction, append-only ledger, notes are unverified data. |
| Agents cannot confirm their own notes | yes (no MCP promotion tool; guard test) | test | "Only a person can promote a note." |
| Works inside Claude Code hooks | `SessionStart` injection yes (one live canary run) | one run, one prompt | "A SessionStart hook can load a session brief; timeout/failure behaviour not tested." |
| Fingerprint staleness is accurate | partial | 7,701 flips / 9,004 raw changes on one ledger; no ground truth | Do not quote a false-flag rate. |

Framing rule: complementary to native memory and generic memory services, never "better than".
Every number in a draft must trace to a committed report under `docs/evals/`.
