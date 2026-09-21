# Devlog 196 — Agent session memory: Phase 0 audit + Phase 1 inbox (RFC 0151)

**Date:** 2026-09-21
**PRs:** none yet (uncommitted on `main`)
**Branch:** main (local)

---

## Summary
Started the agent-session-memory plan (`todo-agent-session-memory.md`). Wrote RFC 0151 and a Phase 0
spike, then shipped Phase 1: a new `ekos-session` crate (capped, redacted, append-only JSONL inbox)
plus `ekos session note` / `ekos session status`. Nothing touches the ledger yet.

---

## Phase 0 — audit

### Problem / motivation
The plan had `[verify P0]` tags and named dependencies by RFC numbers that don't exist here.

### What was built
`docs/spikes/session-memory-findings.md`, `ekos/docs/rfcs/0151-agent-session-memory.md`.

### Decisions
- The plan's "RFC 0136/0137" are **RFC 0135 Parts C/B** in this repo (0136 is web-console graph v2);
  both already landed, so P2/P4 are not blocked.
- Reuse `Custom("Claim")` (`claim_type: "session_note"`); `Session`/`DeadEnd` become new registry rows in
  P2. `ClaimStatusChanged` = `EventKind::Custom`, persisted like `architecture_review`.
- Open Q1 → ledger (redaction twice + short capped text; any confirmed leak switches to sidecar).
  Open Q2 → per-workspace. Owner not consulted; both are reversible.
- Hook behaviour is **UNVERIFIED** (no live scratch session run). Blocks P6 only.

## Phase 1 — inbox

### What was built
| Component | Notes |
|---|---|
| `crates/session/src/inbox.rs` | `Inbox::append` — redact → validate → cap → dedupe → `O_APPEND` |
| `[session-memory]` config | kebab-case like every other section; `enabled = false` default |
| `commands/session.rs` | `note`, `status`; refuses to write while disabled |

### Implementation details worth remembering
- `Redactor` is a fallible trait so a broken redactor drops the note; the real `redact()` is infallible.
- A note that is *only* `[REDACTED:…]` markers is dropped.
- `entry_id` = SHA-256 of (session, kind, redacted text, rationale, anchors), so resends are idempotent.
- A torn last line (crash mid-append) is skipped on read and the next append starts a new line.
- Files `0600`, dir `0700`; inbox dir and anchors are canonicalised and refused if they leave the
  workspace (`..`, absolute-elsewhere, symlink out).
- `pending_commit` = entries − a `.committed` counter file (written by P2's commit; 0 until then).

---

## Knowledge Captured
- `EkosConfig` is `deny_unknown_fields`-strict in places; a new section needs both the struct field and
  the `Default` impl entry or it won't compile.
- Dropped entries are counted in a `<session>.dropped` sidecar, not in the JSONL, so a flood can't grow the log.

## Files Changed
| File | Change summary |
|---|---|
| `ekos/crates/session/**` | new crate, 15 tests |
| `ekos/crates/cli/src/commands/session.rs`, `app.rs`, `mod.rs`, `Cargo.toml` | `ekos session` command, 3 tests |
| `ekos/crates/compiler-core/src/config.rs` | `SessionMemoryConfig` |
| `ekos/Cargo.toml` | workspace member + dep |
| `ekos/docs/rfcs/0151-…`, `docs/spikes/session-memory-findings.md`, `TODO.md` | RFC, spike, plan |
