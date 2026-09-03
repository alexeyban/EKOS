# Devlog 151 — RFC 0128: Web Console Phase 0 (part 2) — TCP auth, Python MCP client, `web/` skeleton

**Date:** 2026-09-03
**PRs:** commits on branch `rfc/0128-web-console-phase-0` → `main`
**Branch:** `rfc/0128-web-console-phase-0` → `main`

---

## Summary

RFC 0127 (Web Console) landed its Rust Phase 0 contracts last session (`devlog_150`: `ekos graph
export`, `ekos status --json`, `ekos_graph_export`). RFC 0128 finishes Phase 0 — the parts that
introduce Python and Node to a Rust-only repo:

- **R4** — optional bearer-token auth on `ekos mcp serve --tcp`.
- **The Python asyncio NDJSON/TCP MCP client** — the console's only read path into a workspace.
- **The `web/` skeleton** — FastAPI app factory + a handful of real endpoints proving the MCP
  wiring end to end, a Vite + React + TypeScript shell, and a one-command `docker-compose.yml`.

The statistics dashboard, config UX, job runner, scheduler, and graph views are **not** here —
they are RFC 0127 Phases 1–7, each authored just-in-time (RFC 0129+). The `web/` modules for them
exist as stubs (`runner.py`, `scheduler.py`, `commands.py`, `config_io.py`) so the layout is real.

End to end was verified against this repo's own `.ekos/`: `/api/workspaces/self/graph` returns a
real `GraphExport` through the Python client talking to a token-authed `ekos mcp serve --tcp`
(counts: 20 793 ledger entries, 5 533 objects, 8 364 relationships).

---

## PR — RFC 0128 (the RFC)

`ekos/docs/rfcs/0128-web-console-phase-0.md` — "Phase 0 (rest of)". Status Accepted. Defers RFC
0127 Phases 1–7 to their own RFCs and lists three open questions carried forward (per-workspace
MCP-server lifecycle vs. the job-runner supervisor; console auth beyond a static token; whether
generated-TS drift deserves a CI gate).

---

## PR — R4: bearer-token auth on `ekos mcp serve --tcp`

### Problem / motivation

RFC 0115 shipped `--tcp` with no authentication — its only mitigation is loopback binding. The web
console keeps that posture (the MCP server is never published; FastAPI is the only reachable
surface), and R4 is defence in depth so a second unrelated local process cannot connect casually.

### What was built

| Component | Role |
|---|---|
| `--tcp-token-file <FILE>` on `McpCommands::Serve` (`bin/ekos.rs`) | reads the token (whitespace-trimmed) from the file, or from `EKOS_MCP_TOKEN` if the flag is absent; flag wins |
| `serve_messages(.., require_token: Option<&str>)` (`commands/mcp.rs`) | when `Some`, the first non-blank line must be an `initialize` whose `params._meta.token` matches; else one `-32001 unauthorized` line and close |
| `authorize_initialize(line, expected)` | parses the first line, checks `method == "initialize"` and `params._meta.token` via `ct_eq` |
| `ct_eq(a, b)` | hand-rolled constant-time compare: length check, then XOR-accumulate over the bytes. No new dependency (`subtle` is only a transitive dep of rustls; promoting it for eight lines isn't worth it) |

- stdio is **never** gated — it's already a private pipe owned by the spawning host. Both the
  stdio path and the token-less TCP path pass `require_token: None`.
- `--tcp` with no token configured is byte-for-byte RFC 0115 behaviour. Back-compat guarantee,
  not an oversight — existing loopback setups keep working untouched.
- The token length leaks through the length check; the token itself is sent in cleartext over a
  plaintext socket. This defends against a casual local process, **not** a wire attacker. TLS
  stays out of scope, consistent with RFC 0115 and RFC 0113.

### Tests (5 new, `commands::mcp::tests`)

`ct_eq_is_length_and_content_sensitive`, `authed_connection_proceeds_when_the_initialize_token_matches`,
`authed_connection_is_rejected_when_the_token_is_wrong_or_absent`,
`authed_connection_is_rejected_when_the_first_message_is_not_initialize`,
`tcp_transport_enforces_the_bearer_token` (real socket: right token connects, wrong token gets
`-32001` and the socket closes).

---

## PR — the Python MCP client + `web/` skeleton + CI

### What was built

| Area | Contents |
|---|---|
| `web/api/app/mcp_client.py` | `EkosMcpClient` (asyncio, ~150 lines, no MCP SDK — the transport is raw NDJSON/TCP) + `ClientPool` (one client per workspace id, connected lazily) |
| `web/api/app/main.py` | `create_app()` factory: CORS for the Vite origin, routers, static mount of `ui/dist` when present, `ClientPool` on `app.state` via lifespan |
| `web/api/app/settings.py` | pydantic-settings, `EKOS_CONSOLE_` prefix: `CONSOLE_TOKEN`, `WORKSPACES_JSON` (JSON array of `{id,name,path,mcp_host,mcp_port}`), `DEV_ORIGIN`; `EKOS_MCP_TOKEN` (no prefix) forwarded to the MCP handshake |
| `web/api/app/routes/{meta,workspaces,graph}.py` | `/api/health` (public), `/api/workspaces`, `/api/workspaces/{id}/{stats,graph,search}` — the last three proxied to `ekos_status` / `ekos_graph_export` / `ekos_search` |
| `web/api/app/deps.py` | `require_console_token` (FastAPI dependency, `secrets.compare_digest`), `mcp_for_workspace` (resolves the workspace + hands back a pooled client) |
| `web/api/app/{runner,scheduler,commands,config_io}.py` | **stubs** — Phase 3 / 4 / 1 / 2 respectively; `COMMAND_ALLOWLIST: list[Command] = []` + the dataclass shape only |
| `web/ui/` | Vite + React 18 + TS shell, one page: fetch `/api/health` + `/api/workspaces`, render a status card + workspace list. House palette tokens copied into `src/theme.css`. `npm run gen:api` wired but `src/api/types.ts` is a hand-stub until the API runs |
| `web/docker-compose.yml` | `api` (built from `./api`, port 8000) + `ui` (`node:20-slim`, `vite dev --host`, port 5173). `ekos` release binary + workspace bind-mounted into `api`. The console does **not** run the MCP server from Compose — the operator starts one by hand for the skeleton |
| `.github/workflows/ci.yml` | new `web` job: build `ekos` (release), `uv sync` + `ruff check` + `ruff format --check` + `pytest` for `web/api` (with `EKOS_BIN` set), `npm ci` + `tsc --noEmit` + `vite build` for `web/ui` |
| `.gitignore` | `web/` build artifacts (`.venv`, `node_modules`, `dist`, `__pycache__`, …) |

### Implementation details worth remembering

- **`EkosMcpClient` is not concurrency-safe per connection.** One `_io_lock` serializes requests
  so concurrent callers don't race to read each other's response line; `_request` also skips any
  line whose `id` doesn't match (notifications, out-of-band). Give each caller its own client, or
  serialize — `ClientPool` gives each workspace one client and every request to that workspace
  goes through its `_io_lock`.
- **`call_tool` double-decodes.** This server returns tool output as
  `{"content":[{"type":"text","text":"<json>"}], "isError": bool}` (`mcp.rs::tool_ok`). The
  `text` payload is *itself* a JSON document — the client parses `content[0].text` and returns
  that. `isError: true` → `McpToolError` with the text as the message; a JSON-RPC `error` object
  → `McpError`. A `text` that isn't JSON is returned as-is rather than raising.
- **One reconnect retry.** `_request_with_retry` catches `ConnectionError` / `IncompleteReadError`
  / `OSError`, does `aclose()` + `connect()` (which re-runs the `initialize` + token handshake),
  and retries once. A second failure propagates.
- **Handshake:** `connect()` sends `initialize` with `protocolVersion: "2025-06-18"` and, when a
  token is set, `params._meta.token`; then the `notifications/initialized` notification (no id, no
  response).
- **The settings env var is `EKOS_CONSOLE_WORKSPACES_JSON`, not `EKOS_CONSOLE_WORKSPACES`** — the
  field is `workspaces_json` and pydantic-settings appends the field name to the prefix verbatim.
  A stray comment in `settings.py` said `EKOS_CONSOLE_WORKSPACES`; fixed this session. `docker-
  compose.yml`, `README.md`, `App.tsx`, and the tests all already had it right.
- **Console token vs. MCP token are unrelated.** `CONSOLE_TOKEN` (browser → FastAPI, checked with
  `secrets.compare_digest` on every router except `meta`) and `EKOS_MCP_TOKEN` (FastAPI → MCP
  server, R4) are two independent secrets.
- **`/stats` uses the `ekos_status` MCP tool, not R2's `ekos status --json`.** The richer R2
  payload (storage breakdown, evidence count, `last_write`) needs a subprocess call, which
  arrives with the Phase 1 job runner. The skeleton's `/stats` returns
  entries/objects/relationships from the tool.

### Tests

`web/api/tests/`: 5 unit (`test_mcp_client_unit.py` — framing, id monotonicity, handshake,
`call_tool` unwrapping, error mapping — against an in-memory duplex stream), 3 API
(`test_api.py` — health is public, a missing/wrong console token is 401, workspaces list), 2 live
(`test_mcp_client_live.py` — starts a real `ekos mcp serve --tcp 127.0.0.1:0 --tcp-token-file`,
asserts `list_tools()` has `ekos_graph_export`, calls it and gets a graph, asserts a wrong token
is rejected). The live tests skip unless `$EKOS_BIN` points at a built binary; CI builds it and
sets the var.

### End-to-end check (RFC §5, not CI-asserted)

Against this repo's own `.ekos/` (fact-segment backend):

```
ekos mcp serve --workspace <repo> --tcp 127.0.0.1:7331 --tcp-token-file <token>   # by hand
uvicorn --factory app.main:create_app --port 8099
  EKOS_CONSOLE_CONSOLE_TOKEN=…  EKOS_MCP_TOKEN=…  EKOS_CONSOLE_WORKSPACES_JSON='[{…}]'
```

| Request | Result |
|---|---|
| `GET /api/health` (no auth) | `{"status":"ok",…}` |
| `GET /api/workspaces` (no token) | 401 |
| `GET /api/workspaces` (token) | `[{"id":"self","name":"EKOS",…}]` |
| `GET /api/workspaces/self/stats` | `{"entries":20793,"objects":5533,"relationships":8364}` |
| `GET /api/workspaces/self/graph?level=aggregate&group_by=kind` | real `GraphExport`: 16 nodes, 15 edges, `Σ node.count == 5533` |
| `GET /api/workspaces/self/search?q=ledger&limit=3` | real `ekos_search` hits (`Ledger`, `ekos_ledger::Ledger`, `ekos-ledger`) |
| wrong console token | 401 |

---

## Knowledge Captured

- **`EKOS_CONSOLE_WORKSPACES_JSON` is the workspace-registry env var, not `EKOS_CONSOLE_WORKSPACES`.**
  pydantic-settings with `env_prefix="EKOS_CONSOLE_"` maps field `workspaces_json` →
  `EKOS_CONSOLE_WORKSPACES_JSON`. Getting this wrong yields a silent empty workspace list (`[]`)
  and every `/{id}/…` route returns `unknown workspace`.
- **The MCP server returns tool results as JSON-inside-JSON.** `tools/call` →
  `{"content":[{"type":"text","text":"<a JSON string>"}]}`. Any client has to parse `content[0].text`
  a second time. The Python client does; a naive client that returns `content` verbatim gets a
  string, not an object.
- **The console needs one long-lived MCP connection per workspace, serialized.** `KnowledgeStore`
  is `!Sync` and each TCP connection is a fresh independent ledger open (RFC 0115). The client is
  single-flight per connection by an `asyncio.Lock`; concurrency lives in FastAPI's request
  handling and the per-workspace client pool, not in the socket.
- **Pre-existing clippy noise on `main` with a newer local toolchain.** `cargo clippy -p ekos
  --all-targets` fails locally (clippy 0.1.98, 2026-08-18) on `field_reassign_with_default` in
  `ask.rs` / `recover.rs` and `unnecessary get().is_none()` in `tests/skeleton.rs` — these are on
  `main`, unrelated to this branch, and are new lints in a clippy version ahead of whatever CI's
  `dtolnay/rust-toolchain@stable` resolved to when that code landed. Not fixed here; worth a
  separate chore pass.
- **`ekos mcp serve --tcp` with a token still answers `initialize` normally on success** — it
  doesn't consume the first message and then wait for a second `initialize`. The client sends one
  `initialize` (with `_meta.token`), gets the normal `initializeResult` back, and proceeds.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0128-web-console-phase-0.md` | New — the RFC |
| `ekos/crates/cli/src/bin/ekos.rs` | `--tcp-token-file` on `McpCommands::Serve`; reads file / `EKOS_MCP_TOKEN`; threads `Option<String>` to `mcp::run` |
| `ekos/crates/cli/src/commands/mcp.rs` | `ct_eq`, `authorize_initialize`, `require_token` param on `serve_messages`, auth-handshake preamble, `Option<String>` token on `run`/`serve_tcp`; 5 new tests |
| `web/api/**` | New — FastAPI skeleton, `EkosMcpClient` + `ClientPool`, routes, settings, deps, Phase 1–4 stubs, tests, `pyproject.toml` + `uv.lock` + `ruff.toml` + `Dockerfile` |
| `web/ui/**` | New — Vite + React 18 + TS shell, `api/{client,types}.ts`, theme tokens, tsconfig set |
| `web/docker-compose.yml` | New — `api` + `ui` services |
| `web/README.md` | New — how to run the skeleton |
| `.github/workflows/ci.yml` | New `web` job (ruff + pytest + node build) |
| `.gitignore` | `web/` build-artifact ignores |
| `web/api/app/settings.py` | Fixed the `EKOS_CONSOLE_WORKSPACES` → `EKOS_CONSOLE_WORKSPACES_JSON` comment |
| `README.md` | Optional bearer-token auth paragraph in the MCP-over-TCP section; new "Web console" subsection |
| `TODO.md` | RFC 0128 ticked under RFC 0127; "next increment" re-pointed to 0129+ |
| `docs/generated/ekos-self-documentation.html` | R4 token paragraph in §10; "Web console" paragraph in the storage/graph section |
| `devlogs/devlog_151.md` | This file |
