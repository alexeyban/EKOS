# Devlog 197 — Agent session memory: Phases 2–8 (RFC 0151)

**Date:** 2026-09-21
**PRs:** none (uncommitted on `main`)
**Branch:** main (local)

---

## Summary
Built the rest of RFC 0151 on top of devlog_196's inbox: ledger commit, anchors + staleness, `recall`/
`brief`, human-only lifecycle, an eval harness, the agent-facing MCP tools, transcript capture + LLM
extraction proposals, and the hardening/docs pass. The full loop runs on the real pipeline
(`demo/session-memory/run.sh`, transcript committed). Two things are deliberately **not** done:
live Claude Code hook verification and a live-model eval — the Phase 5 GO is on a deterministic proxy.

---

## Phase 2 — commit
`ekos session commit` = observe (re-redact + content-hash the pending batch) → pure `map_entries`
(`Session`, `SessionClaim`, evidence, `ObservedIn`/`AnchoredTo`, `DeadEnd` events, UUIDv5 ids, entry
timestamps — no clock) → append with `WriteContext { stage: "session-commit" }`. Existing claims
(same `entry_id`) are never overwritten, so a reviewed claim survives a re-commit. Lock policy:
open-with-backoff (0/0.2/0.4/0.8/1.6/3s); on exhaustion notes stay pending — never an error.

### Decisions
- **`SessionClaim`, not `Claim`.** `architecture_evaluator`, identity and `ekos_architecture_review`
  read `Custom("Claim")`; sharing it would shift architecture-confidence scores.
- **Isolation by filter, not by store.** `Runtime::find_objects`/`retrieve` drop `Session`/`SessionClaim`
  (name prefix `session-` as a pre-filter, real kind as the decision).

## Phase 3/4 — read path, staleness, lifecycle
Verdicts `fresh|changed|orphaned|unanchored` from a per-kind fingerprint pinned on `AnchoredTo`.
`lifecycle.rs` has no agent actor; a test scans `mcp.rs` for it. Confirm/reject/supersede re-append the
claim and add a `ClaimStatusChanged` event; nothing is deleted.

## Phase 5 — eval (proxy)
`ekos session eval`. Proxy GO: correctness 1.00 vs 0.62, stale-served 0.00 vs 0.50. Report:
`docs/evals/session-continuity-2026-09-21.md`.

## Phase 6/7 — MCP + capture
`ekos_session_note` (opens no ledger handle, tested), `_recall`, `_brief`, all gated on
`[session-memory] enabled`. Capture slices are redacted, content-addressed, retained N days; extraction
requires a quoted span that exists in the slice, strict JSON, cached by slice checksum, output `T0`.

## Phase 8
Threat model + residual risks in the RFC; user guide, comms checklist (no draft), demo.

---

## Knowledge Captured
- **The eval found three real bugs before it found a result.** (1) no stopwords: "the" matched every
  note so negative controls never refused; (2) no stemming: "run" tied "reconciliation … runs" with an
  injected note; (3) the first fixture changed tables the compaction baseline had already forgotten, so
  its stale-served rate was 0 by forgetting — a fixture flaw. Fixed in the system/fixture, not the metric.
- **Fingerprint noise is dominated by derived metadata.** On this repo's ledger (40,280 version pairs),
  `Section.line_start/line_end/doc_type/rfc_*` and `File.size_bytes` flipped fingerprints without a real
  change; narrowing projections cut flips 8,208 → 7,701. The rest is genuine edits (`Section.excerpt`,
  `RustSymbol.signature`). There is no ground truth, so this is not a false-flag rate.
- **`cargo fmt` breaks `str.replace` patches.** A scripted edit written against pre-format code silently
  no-oped on the formatted file (the noise breakdown printed nothing). Check the replacement landed.
- **A global `sed` on a test file rewrote two unrelated tests** (left unused imports). Scope sed edits
  to the function you mean, and read `git diff` afterwards.
- `KirObject.properties` is a `HashMap`: compare KIR via `serde_json::Value`, not serialized strings.
- The TODO named "RFC 0136/0137" dependencies; here they are RFC 0135 Parts C/B. Check numbers against
  the repo, not the plan.
- **Live hook check (same day):** a `SessionStart` hook printing `ekos session brief --format claude-hook` made `claude -p --model haiku` return the canary token from a pending note; the no-hook control returned `NONE`. `claude -p` warns "no stdin data received in 3s" unless stdin is redirected. Not exercised: hook timeout/failure, `SessionEnd`.

## Files Changed
| File | Change summary |
|---|---|
| `ekos/crates/session/src/{anchor,map,commit,read,lifecycle,capture,eval}.rs`, `tests/pipeline.rs` | new modules + 9 integration tests |
| `ekos/crates/cli/src/commands/{session,mcp}.rs`, `app.rs` | session commands, 3 MCP tools, tests |
| `ekos/crates/runtime/src/lib.rs` | hide session kinds from default retrieval |
| `ekos/crates/kir/src/custom_kinds.rs` | `Session`, `SessionClaim` rows + constants |
| `ekos/crates/compiler-core/src/config.rs` | retention + extraction settings |
| `SECURITY.md`, `CLAUDE.md`, `README.md`, `docs/rfcs/0013-*`, `docs/generated/ekos-self-documentation.html` | amendments |
| `ekos/docs/rfcs/0151-*`, `docs/session-memory.md`, `docs/evals/…`, `docs/integrations/…`, `docs/session-memory-comms-checklist.md`, `.claude/skills/session-memory/SKILL.md`, `demo/session-memory/` | RFC, guides, eval, skill, demo |
