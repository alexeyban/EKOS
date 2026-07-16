# Devlog 15 — Agent-session test + `ekos_dependents` / `ekos_diff` MCP tools

**Date:** 2026-07-16
**PRs:** —
**Branch:** main

---

## Summary

Follow-up to devlog 14, driven by a comparison of the EKOS MCP server against Serena (the
LSP-based code-navigation MCP). Added an agent-session integration test that scripts the exact
chained tool flow Claude Code runs, verified end-to-end with a real headless `claude -p` session,
and implemented the two scenarios the comparison surfaced as EKOS's unique value: impact analysis
(`ekos_dependents`) and knowledge change detection (`ekos_diff`). RFC 0013 amended.

---

## Part 1 — Agent-session integration test (`crates/cli/tests/mcp_session.rs`)

A 7-turn scripted session over a SQL fixture where each turn consumes the previous turn's output,
exactly as an agent does: handshake → `ekos_status` → `ekos_ekl` table discovery → id chained into
`ekos_neighborhood` (FK traversal) → `ekos_state` (asserts evidence points back at the defining
SQL) → `ekos_search` → `ekos_dependents` (impact) → `ekos_diff` (change window). If tool shapes,
id round-tripping, or evidence attachment break, this fails before a user notices.

Also verified live: a headless `claude -p` session restricted to the `ekos` MCP tools correctly
reported ledger size, listed all 24 tables, traversed the `orders` FKs — and spontaneously noticed
that two schemas coexist in the workspace (Northwind-style vs lowercase e-commerce).

## Part 2 — Serena comparison (what it taught us)

Serena answers symbol-level questions about one live codebase ("where is this defined, who calls
it"); EKOS answers estate-level questions with evidence ("what exists, how is it related, prove
it, what changed"). Complements, not competitors — the agent flow is EKOS-first (locate the
concept across 44 projects), Serena-second (open the file, edit the symbol). The comparison
surfaced the two scenarios below as things *no* code-navigation MCP can serve.

## Part 3 — New tools

| Tool | Wraps | Answers |
|---|---|---|
| `ekos_dependents(id)` | new `Runtime::relationships_for` (thin ledger delegate, per the RFC 0005 "CLI goes through Runtime" contract), split by edge direction | "What breaks if this changes?" — incoming edges as `dependents`, outgoing as `dependencies`, resolved to name/kind + relationship properties |
| `ekos_diff(from, to?)` | `diff_ledger`; `LedgerDiff` gained a `touched: Vec<String>` field (unique logical ids, sorted) so callers can resolve names | "What knowledge changed since T?" — resolved to Object/Relationship entries, capped at 200 listed with `changed_total` for the rest |

Verified against the real 44-project ledger: `ekos_dependents(orders)` → dependents
`order_items` + `payments`, dependency `customers`; `ekos_diff` since this afternoon's commit →
5,161 changed / 21,967 unchanged, reconciling exactly with devlog 14's commit numbers.

## Knowledge Captured

- **`RelationshipKind` now has the same `Display` contract as `ObjectKind`** (Custom passthrough,
  Debug fallback) — it was Debug-rendered in the CLI before, which would have leaked
  `Custom("x")` formatting into MCP output. Anything user- or agent-visible should render via
  Display, and both kind enums now do.
- **Tool output should resolve ids to names before the agent sees them.** `diff_ledger`'s raw
  entry ids were useless to an agent; the `touched` logical ids + name resolution (with a listing
  cap for full-rebuild windows) is what makes the tool consumable in one hop.
- A headless `claude -p ... --allowedTools "mcp__ekos__*"` run is a cheap, genuine end-to-end
  test of the full agent → MCP → ledger path — worth keeping in the verification repertoire.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/tests/mcp_session.rs` | New 7-turn agent-session integration test |
| `ekos/crates/cli/src/commands/mcp.rs` | `ekos_dependents` + `ekos_diff` tools; 3 new unit tests (12 total) |
| `ekos/crates/runtime/src/lib.rs` | `Runtime::relationships_for` delegate + test |
| `ekos/crates/ledger/src/lib.rs` | `LedgerDiff.touched` (unique logical ids, sorted) |
| `ekos/crates/kir/src/lib.rs` | `Display` for `RelationshipKind` (mirrors `ObjectKind`) |
| `docs/rfcs/0013-mcp-server.md` | Tools table amended with the two new tools |
| `README.md` | MCP tool list updated |
