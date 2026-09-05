# EKOS Web Console

A browser surface over one or more compiled EKOS workspaces — RFC 0127 (umbrella).

**Status: Phases 0–6 shipped, Phase 7 (hardening) in progress.** A persisted workspace registry
with an auto-restarting MCP supervisor per workspace; a statistics dashboard; an `ekos.toml`
config editor with validation and a preview-scan; a command/job runner with SSE log streaming and
the read/write role split; scheduled runs; and a 2D graph view — overview → drill-in, kind/
relationship filters, search-with-fly-to, an evidence-backed object panel, a time-travel slider,
neighbourhood isolation, impact-mode tracing, a server-side layout for large graphs, and PNG/glTF
export; and an **Evals** tab browsing every saved RFC 0138 `ekos eval run` report (history table +
per-scenario detail), with a new run triggerable from the existing **Run** tab like any other
allowlisted command. See the root [`README.md`](../README.md)'s own "Web console" section for the
full, versioned feature list per RFC; this file stays focused on running and developing the
console itself.

```
web/
├── api/   FastAPI backend + EkosMcpClient (Python 3.12, uv)
├── ui/    Vite + React + TypeScript app
└── docker-compose.yml
```

## Architecture

```
Browser (React)
   │ HTTP (session cookie)
   ▼
FastAPI console ──┬─ NDJSON over TCP ──► ekos mcp serve --tcp   (reads only, one per workspace,
                   │                                              auto-spawned + restarted by
                   │                                              the console's own supervisor)
                   └─ subprocess ───────► ekos build/recover/…   (writes, job-queued, one at a
                                                                    time per workspace, SSE log)
```

Reads go through the MCP TCP transport (RFC 0115), which holds a warm read-only ledger handle
(RFC 0097) — the console never spawns `ekos mcp serve` itself by hand; registering a workspace is
enough, and the supervisor (RFC 0129) keeps a server up per workspace with exponential-backoff
restart. Writes (`build`/`recover`/`resolve`/`compile`/`commit`, a chained `pipeline`, `ekl`,
`ledger repair`, `docs generate`, …) are supervised subprocesses through the job runner (RFC 0131)
— **never** through MCP, and gated behind the write role.

## Run it (dev)

```sh
# 1. build the CLI once
(cd ../ekos && cargo build --release -p ekos)

# 2. start the API — token auth (no OIDC issuer configured => token mode)
cd api
EKOS_BIN=../../ekos/target/release/ekos \
EKOS_CONSOLE_CONSOLE_TOKEN=dev-read EKOS_CONSOLE_CONSOLE_WRITE_TOKEN=dev-write \
EKOS_CONSOLE_SESSION_SECRET=$(openssl rand -hex 16) \
uv run uvicorn app.main:create_app --factory --reload

# 3. UI (another shell)
cd ui && npm ci && npm run dev
# open http://localhost:5173, sign in with dev-write (or dev-read), then register a real
# workspace directory from the Workspaces page — the supervisor spawns its MCP server for you.
```

Or `EKOS_WS=/path/to/workspace docker compose up` (see `docker-compose.yml`'s own header for what
it seeds). OIDC (Authorization Code + PKCE) is the other auth mode — set `EKOS_CONSOLE_OIDC_ISSUER`
(+ `_OIDC_CLIENT_ID`/`_OIDC_CLIENT_SECRET`/`_OIDC_ROLE_CLAIM`/`_OIDC_WRITE_VALUES`) instead of the
two static tokens; both modes end in the same signed session cookie.

## Test

```sh
cd api && uv run ruff check . && uv run ruff format --check . \
        && EKOS_BIN=../../ekos/target/release/ekos uv run pytest
cd ../ui && npm run typecheck && npm run build
```

Live tests (a real `ekos mcp serve --tcp`, a real compiled `.ekos/`) are skipped unless `$EKOS_BIN`
points at a built binary — CI builds it and exports the variable. `ui` has no test runner yet;
`typecheck` + `build` are its whole gate, matching `.github/workflows/ci.yml`'s `web` job exactly.

## Performance notes (RFC 0136 Phase 7)

Every route is code-split (`react-router-dom`'s per-route `lazy`) — visiting `/w/:id/graph` never
downloads the dashboard's `recharts` dependency and vice versa. `GraphCanvas` (the force-graph
renderer) is separately lazy-loaded inside the graph page itself, on top of the route split. Above
~2,000 nodes the graph's layout is computed server-side (`POST /workspaces/{id}/graph/layout`,
`networkx` + `fa2_modified`, memoized on graph structure) rather than left to the browser's own
force simulation.
