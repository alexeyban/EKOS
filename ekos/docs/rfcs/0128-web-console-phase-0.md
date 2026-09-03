# RFC 0128 — Web Console Phase 0 (rest of): TCP auth, Python MCP client, `web/` skeleton

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-09-03
**Implemented:** 2026-09-03 (`devlog_151`)
**Phase 0 (part 2) of:** RFC 0127 · **builds on:** RFC 0127 R1/R2/R3 (`devlog_150`), RFC 0115 (MCP over TCP), RFC 0097 (cached read-only handle)
**Defers:** RFC 0127 Phases 1–7 (statistics dashboard, config UX, job runner, scheduler, graph v1/v2) → their own just-in-time RFCs (0129+)

---

## Motivation

RFC 0127 (Web Console, Accepted) shipped its first Phase 0 contracts last session: `ekos graph
export` (R1), `ekos status --json` (R2), and the `ekos_graph_export` MCP tool (R3). All three are
pure Rust and on `main`.

Phase 0 ("Contracts") has three pieces left, and this RFC covers all of them:

1. **R4** (RFC 0127 §7) — optional bearer-token auth on `ekos mcp serve --tcp`.
2. **The Python asyncio NDJSON/TCP MCP client** (RFC 0127 §8.1) — the console's only read path.
3. **The `web/` skeleton + `docker-compose.yml`** (RFC 0127 §8.2, §10 Phase 0) — the FastAPI app
   factory, module layout, a handful of real endpoints proving the MCP wiring end to end, a Vite
   + React + TypeScript shell, and a one-command Compose file.

This is the increment that introduces Python and Node/Vite to a Rust-only repo. RFC 0127 §2.1
argues for that deliberately: a console needs long-lived jobs, a scheduler, and concurrent request
handling, and every one of those pushes on the `KnowledgeStore: Send` constraint that RFC 0045 and
RFC 0115 both documented and worked around. Putting the concurrency in Python keeps the Rust side
synchronous and one-owner-per-handle. The accepted cost: a second runtime, an extra network hop on
reads, and an API surface described in two languages (mitigated by generating the TypeScript
client from FastAPI's OpenAPI schema).

**Explicitly not in this RFC:** the statistics dashboard, `ekos.toml` config UX, the command
allowlist + job runner + per-workspace mutex + SSE logs, APScheduler, and the graph views. Those
are RFC 0127 Phases 1–7, each authored just-in-time. The `web/` modules for them exist here as
stubs so the layout is real, nothing more.

---

## 1. R4 — bearer-token auth on `ekos mcp serve --tcp`

RFC 0115 is explicit that `--tcp` has no authentication and its only mitigation is loopback
binding. The web console preserves that (the MCP server is never published; FastAPI is the only
reachable surface). R4 is defence in depth so a second unrelated process on the same host cannot
connect casually.

### 1.1 Surface

```
ekos mcp serve --tcp 127.0.0.1:7331 --tcp-token-file /run/secrets/ekos-mcp-token
```

- The token is read from `--tcp-token-file` (contents trimmed of surrounding whitespace) or, if
  that flag is absent, from the `EKOS_MCP_TOKEN` environment variable. The flag wins if both are
  present.
- `--tcp` with **no** token configured stays exactly as RFC 0115 shipped it: unauthenticated. This
  is a back-compat guarantee, not an oversight — existing loopback setups keep working.
- stdio transport is **never** gated. It is already a private pipe owned by the spawning host.

### 1.2 Handshake

When a token is configured, the **first** line on every accepted TCP connection must be an
`initialize` JSON-RPC request whose `params._meta.token` equals the configured token. `_meta` is
the MCP-standard place for transport-level metadata.

- Match → the server answers that `initialize` normally (echoing `protocolVersion`, naming the
  server) and the connection proceeds as usual.
- No token, wrong token, or a first message that isn't `initialize` → the server writes a single
  JSON-RPC error `{"code": -32001, "message": "unauthorized"}` and closes the connection. No tool
  is ever reachable on an unauthenticated connection.

The comparison is constant-time. It is hand-rolled (length check, then XOR-accumulate over the
bytes) rather than pulling in `subtle`/`constant_time_eq` — `subtle` is only a transitive
dependency of rustls today and promoting it for eight lines of code is not worth it. The length of
the token leaks through the length check; that is acceptable. This is a bearer token over a
plaintext socket: it defends against a local process connecting casually, **not** against a
network attacker who can read the wire. TLS remains out of scope, consistent with RFC 0115 and
RFC 0113.

### 1.3 Where it lives

`crates/cli/src/commands/mcp.rs`: `serve_messages` gains a `require_token: Option<&str>`
parameter; the stdio path and token-less TCP pass `None`. `mcp::run` and `serve_tcp` gain an
`Option<String>` token, threaded from a new `--tcp-token-file` arg on `McpCommands::Serve` in
`bin/ekos.rs`.

---

## 2. The Python MCP client

`web/api/app/mcp_client.py`. Around 150 lines, `asyncio`, no third-party MCP SDK — the official
`mcp` package is oriented at stdio and Streamable HTTP, and this transport is raw newline-delimited
JSON over a TCP socket.

```python
class EkosMcpClient:
    def __init__(self, host: str, port: int, token: str | None = None): ...
    async def connect(self) -> None          # opens the socket, does initialize + notifications/initialized
    async def list_tools(self) -> list[dict]
    async def call_tool(self, name: str, arguments: dict | None = None) -> Any
    async def aclose(self) -> None
```

- **Framing:** one JSON object per line, `\n`-terminated, UTF-8. Request ids are a monotonic
  integer per client.
- **Handshake:** `connect()` sends `initialize` with `params._meta.token` when a token is set,
  waits for the result, then sends the `notifications/initialized` notification (no id, no
  response).
- **`call_tool` result unwrapping:** this server returns tool results as
  `{"content": [{"type": "text", "text": "<json string>"}], "isError": bool}` (`mcp.rs::tool_ok`).
  The client parses `content[0].text` as JSON and returns it; `isError: true` raises `McpToolError`
  with the text as the message. A JSON-RPC `error` object raises `McpError`.
- **Reconnect:** one automatic retry on `ConnectionResetError` / EOF mid-call; a second failure
  propagates.
- **`ClientPool`:** a dict of `{workspace_id: EkosMcpClient}` created lazily from settings. Real
  per-workspace MCP-server lifecycle management is Phase 1 (RFC 0127 §11 — one `ekos mcp serve
  --tcp` per workspace, the console owns the processes). For the skeleton the pool connects to
  already-running servers named in configuration.

**Testing** (RFC 0127 §13): unit tests drive framing / handshake / id-monotonicity against an
in-memory duplex stream; one live test starts a real `ekos mcp serve --tcp 127.0.0.1:0
--tcp-token-file`, connects, asserts `list_tools()` contains `ekos_graph_export`, calls it and
gets a graph back, and asserts a wrong token is rejected. The live test is skipped unless
`EKOS_BIN` points at a built `ekos` binary; CI builds it and sets the variable.

---

## 3. The `web/` skeleton

### 3.1 Layout

```
web/
├── api/
│   ├── pyproject.toml        # uv, python 3.12
│   ├── ruff.toml
│   ├── app/
│   │   ├── main.py           # create_app() factory: CORS, auth dependency, routers, static mount
│   │   ├── settings.py       # pydantic-settings: CONSOLE_TOKEN, WORKSPACES, MCP host, EKOS_MCP_TOKEN
│   │   ├── mcp_client.py     # §2
│   │   ├── schemas.py        # Health, WorkspaceOut, StatusOut, GraphOut
│   │   ├── routes/{meta,workspaces,graph}.py
│   │   ├── runner.py         # STUB — RFC 0127 §8.5, Phase 3
│   │   ├── scheduler.py      # STUB — RFC 0127 §8, Phase 4
│   │   ├── commands.py       # STUB — COMMAND_ALLOWLIST: list[Command] = []  + the dataclass shape
│   │   └── config_io.py      # STUB — RFC 0127 §8.6, Phase 2
│   └── tests/
├── ui/                       # Vite + React 18 + TypeScript shell
│   └── src/{main.tsx, App.tsx, api/{client.ts, types.ts}, index.css, theme.css}
└── docker-compose.yml
```

### 3.2 Skeleton endpoints

| Method | Path | Backed by |
|---|---|---|
| `GET` | `/api/health` | static — version + `ok` (public, no auth) |
| `GET` | `/api/workspaces` | configured workspace list |
| `GET` | `/api/workspaces/{id}/stats` | `ekos_status` MCP tool (entries / objects / relationships) |
| `GET` | `/api/workspaces/{id}/graph` | `ekos_graph_export` MCP tool (R3), query params → tool args |
| `GET` | `/api/workspaces/{id}/search?q=&limit=` | `ekos_search` MCP tool |

The richer R2 `ekos status --json` payload (storage breakdown, evidence count, `last_write`) needs
a subprocess call, which arrives with the Phase 1 job runner; the skeleton's `/stats` uses the
existing `ekos_status` tool.

### 3.3 Console API auth

A single static bearer token from `CONSOLE_TOKEN`, checked with `secrets.compare_digest` in a
FastAPI dependency applied to every router except `meta`. Real users, roles, and the read/write
split (RFC 0127 §8.4) are Phase 1. This token is unrelated to the R4 MCP token.

### 3.4 Frontend shell

Vite + React 18 + TypeScript + Tailwind + TanStack Query, one page: fetch `/api/health` and
`/api/workspaces`, render a status card and a workspace list. The house palette tokens
(`docs/assets/theme.css`) are copied into `src/theme.css`. No graph, no router, no component
library — those are Phase 1+. `npm run gen:api` (openapi-typescript) is wired but `src/api/types.ts`
is a hand-stub until the API is running to generate against.

### 3.5 Compose

`api` (python:3.12-slim, uvicorn) + `ui` (node:20, `vite dev --host`). The `ekos` release binary
is bind-mounted into the `api` container; a workspace directory is bind-mounted too. The console
does not run the MCP server from Compose — per RFC 0127 §11 it spawns one `ekos mcp serve --tcp`
per registered workspace itself (Phase 1); the skeleton connects to a server the operator starts.

---

## 4. CI

One `web` job in `.github/workflows/ci.yml`: build `ekos` (release, for the live MCP test),
`uv sync` + `ruff check` + `ruff format --check` + `pytest` for `web/api`, then `npm ci` +
`typecheck` + `build` for `web/ui`. Cargo / uv / npm caches. `pages.yml` is untouched.

---

## 5. Verification

- Rust workspace gate clean (`fmt`, `build --workspace`, `clippy --workspace -D warnings`,
  `test --workspace`), including the four new R4 tests and one real-socket auth test.
- `web/api`: `ruff check` clean, `pytest` green (unit + the `EKOS_BIN`-gated live test).
- `web/ui`: `tsc --noEmit` clean, `vite build` succeeds.
- End to end (recorded in `devlog_151`, not CI-asserted): `docker compose -f web/docker-compose.yml
  up` against this repo's own `.ekos/`, `/api/workspaces/<id>/graph` returns a real `GraphExport`
  through the Python client talking to a token-authed `ekos mcp serve --tcp`.

---

## 6. Files Changed

| File / area | Change |
|---|---|
| `ekos/docs/rfcs/0128-web-console-phase-0.md` | This RFC |
| `crates/cli/src/commands/mcp.rs` | `ct_eq`; `require_token` param on `serve_messages`; token on `run`/`serve_tcp`; auth-handshake preamble; tests |
| `crates/cli/src/bin/ekos.rs` | `--tcp-token-file` on `McpCommands::Serve`; reads file / `EKOS_MCP_TOKEN`; passes to `mcp::run` |
| `web/api/**` | New — FastAPI skeleton + `EkosMcpClient` + tests |
| `web/ui/**` | New — Vite + React + TS shell |
| `web/docker-compose.yml` | New |
| `.github/workflows/ci.yml` | New `web` job |
| `.gitignore` | `web/` ignore entries |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Web console section / tick / capability note |

---

## 7. Open questions

1. **Per-workspace MCP-server lifecycle.** Phase 1 has the console spawn and supervise one
   `ekos mcp serve --tcp` per workspace. That is a second subprocess-supervision concern next to
   the job runner (§8.5) — should they share one supervisor abstraction, or stay separate because
   one is long-lived and idle-cheap and the other is bursty and heavy?
2. **Console auth beyond a static token.** Phase 1 needs real users for the read/write role split.
   Session cookies + a local user table, or delegate to an external identity provider and keep the
   console stateless? The Non-Goals (RFC 0127 §11) say "one operator or one small trusted team",
   which argues for the smallest possible thing.
3. **Generated TypeScript types in CI.** `npm run gen:api` needs the API running. Worth a CI step
   that boots uvicorn and diffs the committed `schema.d.ts` against a fresh generation (so a drift
   fails the build), or is that more machinery than a skeleton warrants?
