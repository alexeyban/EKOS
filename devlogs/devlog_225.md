# Devlog 225 — MCP tools refuse argument names they do not declare

**Date:** 2026-09-28
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

The last open "trust" item from the devlog_222 audit. An MCP tool given an argument name it does not
know, such as `max_depth` to `ekos_impact` whose parameter is `max_hops`, used to run with its
default and say nothing. The agent then reasoned over an answer it believed matched its request.
Every `tools/call` is now checked against that tool's own `inputSchema`. An unknown name is an error
that lists the accepted names, and non-object `arguments` are refused.

---

## What was built

| Piece | Detail |
|---|---|
| `check_argument_names` | Runs first in `call_tool`, for every tool, extensions included; a schema with no `properties` is not checked |
| `every_argument_a_handler_reads_is_declared` | Source-scan guard: every key a handler reads (`args.get`, `required_str`, `bounded_arg`, `required_id`) across the 18 dispatch arms and 4 delegated handlers is declared in that tool's schema. It runs with every gated tool switched on, so the ClickHouse and session tools are covered. |
| Console `impact` route | `max_hops` limit 50 → 20, to match `ekos_impact`'s cap from devlog_222; the route now returns 422 instead of forwarding a call the tool would refuse |

## Why strict checking was safe to turn on

Before writing any enforcement, a per-tool scan compared every key each handler reads with that
tool's declared properties. There were zero mismatches, and every key the web console sends was
declared too. The guard test now makes that property permanent: a handler that starts reading
an undeclared key fails CI before the strict check can reject a real call.

**Mutation-checked:** renaming `max_hops` to `max_hopz` in the impact handler made the guard fail
with `["ekos_impact.max_hopz"]`. The file was restored afterwards.

---

## Knowledge Captured

- **A source-scan guard needs a floor on what it matches.** The first per-arm scan used an
  indentation-exact regex that matched zero arms and so printed "no problems". The Rust guard
  asserts it found at least 18 arms, so a scan that silently stops matching fails instead of
  passing.
- **Gated tools hide from `tools/list`.** `ekos_clickhouse_query` only appears with
  `enable_mcp_query`; the guard first panicked on it. A guard over "all tools" has to switch every
  gate on.
- **Changing a limit on one side needs the other side checked.** devlog_222 capped `ekos_impact` at
  20 hops, but the console API still accepted 50. Nothing broke only because the UI sends 5.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/mcp.rs` | `check_argument_names`; 2 tests (refusal + source-scan guard) |
| `web/api/app/routes/graph.py` | impact `max_hops` limit 50 → 20 |
| `web/api/tests/test_graph_unit.py` | 422 above the tool's bound, 200 at it |
| `README.md`, `docs/generated/ekos-self-documentation.html` | Unknown-argument behaviour |
| `TODO.md`, `CLAUDE.md` | Item ticked; devlog pointer |
