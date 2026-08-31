# RFC 0116 — Top-level `ekos status` CLI alias

**Status:** Accepted (per user direction — small, additive)
**Author:** EKOS team
**Created:** 2026-08-31
**Implemented:** 2026-08-31

---

## Motivation

While testing RFC 0115's new MCP TCP transport connected to a VS Code AI chat, the chat assistant
recommended running `ekos status` directly in a shell. It failed:

```
error: unrecognized subcommand 'status'
```

Root cause: `ekos_status` (RFC 0013) is an MCP-only tool name, reachable exclusively through
`ekos mcp serve`'s JSON-RPC `tools/call` — never a CLI subcommand. The closest real CLI equivalent,
`ekos ledger status`, is nested under the `ledger` subcommand group, not top-level. The naming
convention `ekos_status` (underscore) closely resembles the plausible top-level CLI form
`ekos status` (space), which is exactly what got guessed and recommended.

Rather than only documenting the mismatch away, this RFC adds the top-level form so that natural
guess — typed by a human or produced by an AI chat pattern-matching a tool name into a shell
command — actually works.

## Design

`ekos status [--storage]` is a straight alias: it calls the exact same
`ekos_ledger::status`-backed function `ekos ledger status` already calls
(`crates/cli/src/commands/ledger.rs::status`), with the same `--storage` flag. No new business
logic, no new output shape — `ekos status` and `ekos ledger status` produce byte-identical output
for the same workspace. `ekos ledger status` is unchanged and stays for backward compatibility;
this is additive only.

## Non-Goals

- **No relationship-count parity with the MCP `ekos_status` tool.** The MCP tool returns
  `entries`/`objects`/`relationships`; `ledger::status` (and thus both CLI forms) reports
  `entries`/`objects` only, matching its pre-existing behavior. Bringing the CLI and MCP tool to
  exact parity is separate, explicitly-declined scope.
- **No removal of `ekos ledger status`.** Both forms coexist; the top-level one is sugar, not a
  replacement.
- **No broader audit of other MCP-tool-name-vs-CLI-command mismatches.** This RFC fixes the one
  instance that was actually hit live; documenting or aliasing every other tool name
  (`ekos_search`, `ekos_diff`, etc.) is out of scope here.

## Files Changed

| File | Change |
|---|---|
| `crates/cli/src/bin/ekos.rs` | New top-level `Commands::Status { storage: bool }`, dispatched to the existing `ledger::status` |
| `README.md` | Notes `ekos status` as the top-level equivalent of `ekos ledger status` |
