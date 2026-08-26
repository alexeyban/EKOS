# Devlog 127 — RFC 0109: Architecture Claim Human Review

**Date:** 2026-08-26
**PRs:** RFC 0109
**Branch:** main (direct)

---

## Summary

Resumed RFC 0068's §62 Phase 2 remaining work. Built the "Human Review" workflow item —
`ekos_architecture_review`, a second write-capable MCP tool mirroring RFC 0029's
`ekos_identity_review` exactly, letting a human (or the `identity-reviewer`-style agent pattern)
confirm or reject an LLM-classified crate role `Claim` before it's treated as ground truth. Found
and designed around a real correctness trap before writing any implementation code: a naive
"stamp every claim unconfirmed at creation" design would have silently reverted every human review
decision back to unconfirmed on the very next `commit` re-run.

---

## RFC 0109 — Architecture Claim Human Review

### Problem / motivation

`ArchitectureReasoningPass` writes every role `Claim` with `claim_type: "inference"` — honest that
it's LLM-derived — but nothing downstream (`evaluate_architecture`, rendered docs, RFC 0108's
architecture diff) ever distinguished a reviewed claim from an unreviewed one. RFC 0068 names
"Human Review" throughout its target package; RFC 0029 already solved the identical problem for
cross-system identity candidates, and `TODO.md`'s own tracking had already named it as the closest
real precedent to extend.

### What was built

| Component | Change |
|---|---|
| `commit.rs::preserve_claim_review_status` | Carries a claim's real review status forward across re-commits when its role value is unchanged |
| `ekos_architecture_review` | New MCP tool — confirm/reject a role `Claim`, mirrors `ekos_identity_review` |

### Implementation details worth remembering

- **The real design trap, found before any code was written**: `ArchitectureReasoningPass` is
  deliberately ledger-free (an established precedent this session already knew from
  `architecture_drift.rs`'s own doc comment). If it stamped every claim with a literal
  `review_status: "unconfirmed"` property at construction, that property becomes part of the
  object's content signature (RFC 0015) — and since the *same deterministic claim id* gets
  re-derived on every `recover`/`commit` cycle, a claim a human had already confirmed would compare
  as "different content" against the pass's always-unconfirmed fresh version on the very next
  re-run, silently reverting the review. The fix has two parts working together: the reasoning pass
  never writes `review_status` at all (read by absence, everywhere); `commit.rs` — the one place
  that already does real ledger-aware enrichment before appending, matching `commit_rollups`/
  `commit_data_lineage`'s own precedent — carries a real review status forward from the ledger's
  current version when the role value hasn't changed. When nothing genuinely changed, `append_object`
  finds no real content difference and skips writing a version at all — a reviewed claim stays
  completely untouched across repeated re-commits, not just eventually-consistent.
- **This was caught by design reasoning, then proven by both a unit test and a real, live
  end-to-end run** — not just claimed. `a_reviewed_claim_keeps_its_status_and_writes_no_new_version_
  when_value_is_unchanged` proves it directly against `FactLedger`; separately, a real scratch
  workspace was taken through `ekos architecture investigate` (real Ollama LLM call,
  `qwen2.5:1.5b`, classifying a real crate as "CLI entry point"), reviewed via the real
  `ekos mcp serve` binary over stdio, then re-run through a real `recover`/`compile`/`commit` cycle
  — `commit`'s own summary line confirmed it: "Objects skipped: 7 (already in ledger)," the
  reviewed claim among them, `review_status: "confirmed"` still intact afterward.
- **A changed role value deliberately does *not* inherit the old review status.** A crate
  reclassified from "shared utility" to "core library" is a genuinely different assertion — the old
  human confirmation was never about the new one. Verified by a dedicated test.

### Decisions (alternatives considered, why this choice)

- **MCP-only, no new CLI subcommand.** Matches `ekos_identity_review`'s own precedent exactly
  (checked `IdentityCommands` in `bin/ekos.rs` directly — identity review has never had a CLI
  equivalent). Reviewing is an interactive, per-candidate decision; MCP is where that decision
  actually gets made.
- **Not changing how `evaluate_architecture`/rendered docs/RFC 0108's architecture diff consume
  `review_status` yet.** They still read every claim the same way, reviewed or not — a real,
  separately-scoped follow-on (e.g. a lower weight for unconfirmed claims, or a rendered
  "unreviewed" badge), matching RFC 0029's own v1 scope, which likewise didn't immediately change
  how confirmed `SameAs` relationships were consumed elsewhere.

---

## Knowledge Captured

- **A property that only gets written by a review action (never by the pass that creates the
  object) needs the *consuming* pipeline stage to actively preserve it across re-derivation, or
  it silently disappears the next time the object is naturally re-derived.** This is a general
  shape, not specific to claims — any "human annotation layered on top of a deterministically
  re-derived object" feature in this codebase needs the same `commit.rs`-level preservation step,
  found here by reasoning through the actual re-commit semantics before writing code, not by
  hitting the bug in production first.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/commit.rs` | `preserve_claim_review_status`; wired into the object-write loop; 4 new tests |
| `ekos/crates/cli/src/commands/mcp.rs` | `ekos_architecture_review` tool definition + `architecture_review` handler; 5 new tests |
| `ekos/docs/rfcs/0109-architecture-claim-human-review.md` | New RFC |
