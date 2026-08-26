# Devlog 125 — RFC 0107: MCP Exposure of Architecture Query Tools

**Date:** 2026-08-26
**PRs:** RFC 0107
**Branch:** main (direct)

---

## Summary

Continuing RFC 0068's §62 Phase 2 build-out: two new read-only MCP tools —
`ekos_architecture_evaluate` and `ekos_architecture_drift` — giving an AI agent working through
`ekos mcp serve` the same real, deterministic architecture signal `ekos architecture investigate`
already produces, without needing to shell out or run a build.

---

## RFC 0107 — MCP Exposure of Architecture Query Tools

### Problem / motivation

RFC 0068 §62's own tracking named "MCP exposure of architecture query/investigation tools" as a
real, still-open item — `crates/cli/src/commands/mcp.rs` already has ten real tools, but nothing
architecture-specific. `evaluate_architecture` (RFC 0065 Phase 3) and `drift_from_history`
(RFC 0068 §31-32) already exist as pure, already-tested functions in `ekos-recovery`; the gap was
purely a missing MCP wiring, not missing logic.

### What was built

| Component | Change |
|---|---|
| `ekos_architecture_evaluate` | New MCP tool — real completeness/evidence-coverage score |
| `ekos_architecture_drift` | New MCP tool — documentation drift findings |

### Implementation details worth remembering

- **Both `EvaluationReport` and `DriftFinding` already derived `serde::Serialize`** — the MCP
  handler for `ekos_architecture_evaluate` is a single `serde_json::to_value(report)?` call, no
  manual field mapping needed. Worth checking for an existing `Serialize` derive before writing a
  manual JSON-construction block; it's easy to reach for `json!({...})` out of habit even when the
  type already round-trips for free.
- **Deliberately did not expose `ekos architecture investigate`'s orchestration loop itself.** It
  runs `build → recover → compile → commit` (a real write sequence, potentially with LLM calls) —
  fundamentally different in kind from every other tool in this server, which are all read-only
  except the one deliberate exception (`ekos_identity_review`, and even that only ever
  confirms/rejects an already-proposed candidate, never runs a pipeline). Exposing `investigate`
  over MCP would be a real, separate design question (cost confirmation over a JSON-RPC tool call?
  progress streaming for a multi-minute operation with no protocol support for it today?) — not
  something to fold into "expose the query tools" without its own explicit scoping.

### Decisions (alternatives considered, why this choice)

- **Reused `detect_drift`'s exact logic inline rather than refactoring `architecture.rs` to export
  a shared function.** The logic is ~10 lines and both call sites (the CLI command and this new MCP
  tool) already had direct access to the same `KnowledgeStore` methods (`all_objects`,
  `object_history`) — a shared extraction would have added an indirection layer for a very small
  amount of genuinely duplicated code. Revisit if a third call site appears.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/mcp.rs` | Two new tool definitions + dispatch arms; `tools/list` exhaustive-name test updated; 4 new tests |
| `ekos/docs/rfcs/0107-mcp-architecture-tools.md` | New RFC |
