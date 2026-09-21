# Spike — Agent Session Memory, Phase 0 findings (RFC 0151)

Date: 2026-09-21. Source of the plan: `todo-agent-session-memory.md`. Every statement below was
read from the code at this commit unless marked **UNVERIFIED**.

## 1. Dependency numbering in the TODO does not match this repo

The TODO names "RFC 0136 (`KirRelationship::new()` id determinism)" and "RFC 0137
(`source_artifact_id` audit trail)". In this repo `ekos/docs/rfcs/0136-*` is *Web Console Phase 6 —
graph v2*, and 0137 is unrelated. The work the TODO means shipped as **RFC 0135 Part C**
(`KirRelationship::deterministic`) and **RFC 0135 Part B** (`WriteContext` + `audit_trail`, devlog_160
and the per-object `source_artifact_ids` follow-up). Both are already landed, so the Phase 2/Phase 4
"blocked by" notes are cleared. New RFC number: **0151** (highest existing was 0150; `docs/rfcs/`
tops out at 0024).

## 2. Existing claim machinery (`Custom("Claim")`)

- Registered in `ekos/crates/kir/src/custom_kinds.rs:113`, `structurally_keyed: true`, keyed by the
  `(subject, predicate, object)` triple (RFC 0065). Its properties today are architecture-specific:
  `predicate: "has_role"`, plus `review_status` / `reviewed_at` written by `architecture_review`.
- `KirRelationship` already has `valid_from` / `valid_until` (`kir/src/lib.rs:369`, RFC 0047).
- `EventKind` has a `Custom(String)` escape hatch (`kir/src/lib.rs:488`), so `ClaimStatusChanged`
  needs **no** new top-level KIR primitive.

**Decision.** Session notes reuse `Custom("Claim")` with `claim_type: "session_note"`, but the
identity key is the inbox `entry_id`, not a triple, so `Session` and `DeadEnd` are new registry rows
(`structurally_keyed: true`). No new KIR primitive.

## 3. Existing memory artifacts

`.claude/skills/memory` (markdown notes under `memory/`, refreshed by re-running the pipeline) and the
`memory-keeper` subagent are a *file-note* memory. Session memory is a different layer: typed,
anchored, staleness-checked claims. **Decision: supersede nothing in P1–P5**; reconcile in P6 when the
`session-memory` skill lands (the file-note skill keeps cross-project lessons, session memory keeps
per-codebase claims tied to objects).

## 4. Persistence path for review decisions

`identity_review` and `architecture_review` (`cli/src/commands/mcp.rs:1642`, `:1697`) open a **fresh
writable store** (`open_store`), re-append the reviewed object/relationship with a `status` /
`review_status` property and `reviewed_at`, then append a `KirEvent`. No separate lock — the writer
takes the ledger's own cross-process `write.lock` (RFC 0104), and a second writer gets
`LedgerError::Locked` immediately at open.

**Decision.** `ClaimStatusChanged` reuses this exact pattern: re-append the claim with the new
status + append `EventKind::Custom("ClaimStatusChanged")`. Human-only promotion (`ekos session review`)
is a CLI command with **no** MCP twin.

## 5. Lock behaviour for incremental commit

`FactLedger` fails fast with `Locked` when another writable process holds `write.lock`; read-only
opens never take it. So `ekos session commit` cannot run alongside `ekos mcp serve` *only if* serve
holds a writable handle — serve uses read-only stores except for the two review tools, which open a
short-lived handle. **Policy (to implement in P2):** bounded retry with backoff (5 attempts, 200 ms →
3 s), and on exhaustion leave notes `pending` and report it; never surface `Locked` to the agent. The
concurrent-writer repro is a P2 test, not verified here.

## 6. Claude Code hook behaviour — checked against docs, not live

Checked 2026-09-21 against the published hooks reference: `SessionStart`/`SessionEnd` exist; the
`additionalContext` JSON shape is documented; `timeout` is in seconds (default 600 for commands);
`transcript_path`/`session_id`/`cwd` are provided; exit code 2 blocks only tool/prompt events. **No
`PreCompact` event is documented** — the RFC's PreCompact capture idea is dropped. **Live check (2026-09-21):**
a `SessionStart` hook emitting `additionalContext` with a canary token made `claude -p` answer with the
token; the control without the hook answered `NONE`. Still unverified: the failure semantics of a
`SessionStart` hook that times out or exits non-zero, and `SessionEnd`.

## 7. Decision gates (owner unavailable; defaults chosen, reversible)

- **Open Question 1 (ledger vs sidecar for claim text): ledger.** Because the ledger is append-only and
  redaction is a prevention control (RFC 0043), the mitigation is redaction twice (at inbox write and
  again at observation) plus short capped text. The risk register's trigger stands: any confirmed leak
  switches to the sidecar alternative before more data is written.
- **Open Question 2 (scope): per-workspace**, one inbox per workspace under `.ekos/session/inbox`.
