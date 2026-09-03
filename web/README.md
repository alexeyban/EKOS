# EKOS Web Console

A browser surface over a compiled EKOS workspace — RFC 0127 (umbrella) / RFC 0128 (Phase 0).

**Status: Phase 0 skeleton.** This tree contains the FastAPI app factory, the asyncio MCP client,
a few read endpoints, and a Vite + React shell. The statistics dashboard, `ekos.toml` config UX,
command runner, scheduler, and the 3D graph views are RFC 0127 Phases 1–7 — each its own
just-in-time RFC (0129+).

```
web/
├── api/   FastAPI backend + EkosMcpClient (Python 3.12, uv)
├── ui/    Vite + React + TypeScript shell
└── docker-compose.yml
```

## Architecture

```
Browser (React)
   │ HTTP
   ▼
FastAPI console  ── NDJSON over TCP ──►  ekos mcp serve --tcp   (reads only)
```

Reads go through the MCP TCP transport (RFC 0115), which holds a warm read-only ledger handle
(RFC 0097). Writes (`build`/`recover`/…) will be supervised subprocesses in Phase 3 — never
through MCP.

## Run it (dev)

```sh
# 1. build the CLI
(cd ../ekos && cargo build --release -p ekos)

# 2. start an MCP server for a workspace (Phase 1 will do this automatically)
printf 'dev-mcp-token\n' > /tmp/ekos-mcp-token
../ekos/target/release/ekos mcp serve --workspace /path/to/workspace \
    --tcp 127.0.0.1:7331 --tcp-token-file /tmp/ekos-mcp-token &

# 3. API
cd api
EKOS_MCP_TOKEN=dev-mcp-token \
EKOS_CONSOLE_CONSOLE_TOKEN=dev-console-token \
EKOS_CONSOLE_WORKSPACES_JSON='[{"id":"self","name":"EKOS","path":"/path/to/workspace","mcp_port":7331}]' \
uv run uvicorn app.main:create_app --factory --reload

# 4. UI (another shell)
cd ui && npm ci && npm run dev
# open http://localhost:5173 ; in the console:
#   localStorage.setItem('ekos-console-token', 'dev-console-token')
```

Or `EKOS_WS=/path/to/workspace docker compose up` (see `docker-compose.yml` header for the
one manual step).

## Test

```sh
cd api  && uv run ruff check . && EKOS_BIN=../../ekos/target/release/ekos uv run pytest
cd ../ui && npm run typecheck && npm run build
```

The live MCP-client test is skipped unless `EKOS_BIN` points at a built binary.
