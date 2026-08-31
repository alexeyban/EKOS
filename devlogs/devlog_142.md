# Devlog 142 — RFC 0116: top-level `ekos status` CLI alias

**Date:** 2026-08-31
**PRs:** the commit landed with this devlog
**Branch:** main (direct)

---

## Summary

Testing RFC 0115's new MCP TCP transport connected to a VS Code AI chat surfaced a small but real
usability gap: the chat recommended running `ekos status` directly in a shell, which failed with
`error: unrecognized subcommand 'status'`. Added a top-level `ekos status [--storage]` CLI alias
for the existing `ekos ledger status` so that natural guess actually works.

---

## PR — RFC 0116: top-level `ekos status` alias

### Problem / motivation

`ekos_status` (RFC 0013) is an MCP-only tool name — reachable exclusively through `ekos mcp
serve`'s JSON-RPC `tools/call`, never a CLI subcommand. The closest real CLI equivalent, `ekos
ledger status`, is nested under the `ledger` subcommand group, not top-level. The naming
convention (`ekos_status`, underscore) closely resembles the plausible top-level CLI form (`ekos
status`, space) — exactly what a VS Code AI chat pattern-matched into a shell command
recommendation, and exactly the kind of thing a human skimming the MCP tool grid could also guess.

### What was built

A straight top-level alias: `Commands::Status { storage: bool }` in
`crates/cli/src/bin/ekos.rs`, dispatched to the exact same
`ekos::commands::ledger::status(&config, &cwd, storage)` function `ekos ledger status` already
calls. No new business logic, no changed output shape — verified byte-identical output between
`ekos status --storage` and `ekos ledger status --storage` against this repo's own real, populated
`.ekos/` workspace (20,793 entries, 5,533 objects).

### Decisions (alternatives considered, why this choice)

- **Alias, not a rename/move**: `ekos ledger status` stays exactly as it was — additive only, no
  backward-compatibility break.
- **No relationship-count parity with the MCP `ekos_status` tool**: the MCP tool returns
  `entries`/`objects`/`relationships`; the CLI form (both `ledger status` and the new top-level
  alias) still reports `entries`/`objects` only. Explicitly declined as separate scope — the user
  chose "add the alias" only, not "make CLI and MCP report identical numbers."
- **No broader audit of every other MCP-tool-name-vs-CLI-command mismatch** (`ekos_search`,
  `ekos_diff`, etc.) — this fixes the one instance actually hit live, not a general sweep.

---

## Knowledge Captured

- **MCP tool names and CLI subcommand names live in two separate namespaces with no automatic
  relationship** — `crates/cli/src/commands/mcp.rs`'s `tools_call` dispatch and
  `crates/cli/src/bin/ekos.rs`'s `Commands` enum are entirely independent, and clap gives no
  hint when a tool-shaped name isn't a real subcommand beyond the generic "unrecognized
  subcommand" error. An AI chat client connected over MCP has no way to know which of its
  available tool names might also plausibly work (or not) as a raw shell command — this is a
  standing minor confusion risk for any future MCP tool added without a matching top-level CLI
  form, worth remembering next time a new `ekos_*` tool ships.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0116-top-level-status-alias.md` | New RFC |
| `ekos/crates/cli/src/bin/ekos.rs` | New top-level `Commands::Status { storage }`, dispatched to the existing `ledger::status` |
| `README.md` | Notes `ekos status` as the shorter top-level equivalent |
| `docs/generated/ekos-self-documentation.html` | Storage-shrinking paragraph mentions the new alias |
