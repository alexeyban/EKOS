# Devlog 263 — MCP gateway on the console (RFC 0182)

**Date:** 2026-10-13
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

Phase E of the production plan. An agent now reaches a project through the console at
`/mcp/<tenant>/<project>` with an API token. The gateway filters tools by role, attaches the
caller's read scope to every call, drops any scope the client sends, and records every call.
The supervisor no longer starts every project's MCP server at console start: a server starts on
first use, stops when idle, and a cap limits how many run. No Rust code changed.

---

## RFC 0182

### Problem / motivation

`ekos mcp serve` knows nothing of tenants, roles or scopes. RFC 0180's API tokens worked only
against the console's HTTP API, which no MCP client speaks. And every registered project held a
server process for the console's whole life; one held 13 GB of a 16 GB machine.

### What was built

| Part | Change | Where |
|---|---|---|
| Gateway | `POST /mcp/{tenant}/{project}`: `initialize`, `ping`, `tools/list`, `tools/call`, batches, SSE on `Accept`, `202` for notifications, `405` for `GET` / `DELETE` | `web/api/app/routes/mcp_gateway.py` (new) |
| Tool policy | `action_for(tool)`: review, operate or read; `SCOPED_TOOLS` | `web/api/app/mcp_policy.py` (new) |
| Raw forwarding | `call_tool_raw`: the server's result unchanged; the call built from name and arguments only | `mcp_client.py` |
| Servers on demand | `acquire`, `reap_idle`, the cap, `last_used` | `supervisor.py` |
| Settings | `mcp_idle_seconds` (900), `mcp_max_servers` (8) | `settings.py` |
| Mounts | `main.MOUNTS`; the route walk covers `/mcp` | `main.py`, `tests/test_authz_routes.py` |
| UI | `idle` for a stopped server; the MCP URL beside the tokens | `pages/Workspaces.tsx`, `Admin.tsx` |

### Implementation details worth remembering

- **The gateway answers `initialize` itself.** The connection to the project's server is one
  shared, already-initialised client; an agent's handshake is with the gateway.
- **A notification is accepted and not forwarded**, a `tools/call` sent as one included. A call
  without an id cannot be answered, so it is not run.
- **The refusal of a tool is a tool result** (`isError: true`), not a JSON-RPC error: MCP
  clients show the first to the model and treat the second as a broken server.
- **`_meta` inside `arguments` is passed as an argument.** It is the tool's to reject. Only
  `params._meta` is where a scope lives, and that the gateway writes itself.
- **A restarted server keeps its `last_used`.** The monitor's replacement handle otherwise
  starts at zero and the reaper stops it at once.
- **Eviction is decided under the lock and carried out after it.** Tearing a server down takes
  seconds; the handle is removed from the table first so nothing else picks it.
- **`GET` on the gateway path answered 404, not 405,** because only `POST` was registered and
  this FastAPI does not derive a 405 from an included router. Two explicit routes answer 405.

### Decisions

- **Audit every call, reads included, without arguments.** A read through the gateway is an
  agent's action on someone's behalf. A query can hold what the caller is looking for, so it is
  not kept.
- **No server at console start.** Starting up to the cap would pick arbitrary projects. The
  first use pays a cold start instead.
- **The Python copy of the scoped-tool list is checked against the Rust source** rather than
  fetched from the server. The gateway must decide before any server is running.
- **`ekos_session_note` needs `operate`.** It writes, though only to the inbox.

---

## Verification

| Check | Result |
|---|---|
| Gateway (`test_mcp_gateway.py`) | 14 pass: protocol, 401 / 404, role filtering, restricted tokens, a client-sent scope dropped, audit, the tool list against the Rust source |
| Supervisor (`test_supervisor_lifecycle.py`) | 5 pass: nothing at start, idle stop and restart, cap evicts the least recently used |
| Live (`test_projects_live.py`) | an API token lists tools and searches through the gateway against the real binary; a deny rule hides its objects on the next call; a self-sent scope changes nothing |
| Whole console API suite | 277 pass (257 before) |
| Console UI | 92 pass |

Not verified: a real MCP client (VS Code, Claude Desktop, ChatGPT) against the gateway. The
tests speak the protocol over HTTP as RFC 0143 defines it.

---

## Knowledge Captured

- **A partial route match is not a 405 here.** Register the methods you want refused.
- **Fixtures imported from another test module trip `F811`** on every test that names them. A
  file-level `# ruff: noqa: F811` with the reason is cleaner than one per test.
- **`cargo fmt` reflows constant tables**, so a test that greps Rust source for `("name",` on
  one line breaks. Match the block, then the name.
- **Idle shutdown needs a notion of "used" that restarts do not reset** and that only real use
  advances; listing a workspace's status must not count.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0182-mcp-gateway.md` | New |
| `web/api/app/routes/mcp_gateway.py`, `mcp_policy.py` | New |
| `web/api/app/supervisor.py` | On demand, idle reaper, cap |
| `web/api/app/{mcp_client,deps,main,settings}.py` | Raw call, `acquire`, mounts, two settings |
| `web/api/tests/test_mcp_gateway.py`, `test_supervisor_lifecycle.py` | New |
| `web/api/tests/test_authz_routes.py`, `test_projects_live.py` | `/mcp` in the walk; live gateway test |
| `web/ui/src/pages/Workspaces.tsx`, `Workspaces.test.tsx`, `Admin.tsx` | `idle`; MCP URL |
| `deploy/.env.example` | Idle and cap settings |
| `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html`, `ekos/docs/rfcs/0181-read-scope.md` | Documented |
