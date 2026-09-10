# Devlog 176 — MCP over Streamable HTTP

**Date:** 2026-09-10
**PRs:** (local) `feat(cli): RFC 0143 — MCP over Streamable HTTP`
**Branch:** main (local)

---

## Summary

`ekos mcp serve` spoke JSON-RPC over stdio (RFC 0013) and a raw TCP socket (RFC 0115) — neither
of which is HTTP. A user configured a VS Code `mcp.json` HTTP endpoint (`url:
http://127.0.0.1:7331`) against the RFC 0115 TCP server and got **no tools at all**: the client
speaks MCP-over-HTTP, the socket answers bare JSON-RPC lines, and nothing bridges them. RFC 0143
adds `ekos mcp serve --http <addr>` — MCP's Streamable HTTP transport at one `POST /mcp`
endpoint. It is far smaller than RFC 0115's non-goals framing suggested, because EKOS has **no
server-initiated messages**, so the entire SSE half of the spec (event streams, sessions,
resumption) is out of scope: every POST answers `application/json`, `GET /mcp` is `405`.

---

## PR — RFC 0143: MCP over Streamable HTTP

### Problem / motivation

Most MCP clients only offer *stdio* (spawn a command) or *Streamable HTTP* (a URL) — VS Code /
GitHub Copilot agent mode, Visual Studio 2022, `mcp-remote`, browser tools. None can talk to the
RFC 0115 raw TCP socket. The stdio path works for them but spawns a fresh process + cold ledger
per client; the intent of a URL-addressable server (one process, many clients) had no
implementation.

### What was built

| Component | Detail |
|---|---|
| `serve_http` (`crates/cli/src/commands/mcp.rs`) | Binds an `axum` app on `<addr>`, one route `POST /mcp`, until killed. Runs on the CLI's ambient `#[tokio::main]` runtime via the same `block_in_place` bridge `run_clickhouse_query_blocking` uses. |
| `build_http_router` | The testable seam — spawns the worker thread + wires the `Router` without binding a socket. |
| One worker `std::thread` + `tokio::sync::mpsc`/`oneshot` | Owns the non-`Send` `StoreCache`; every HTTP handler forwards its raw line to it and awaits the reply. Serializes all MCP work (matches `handle_message`'s blocking one-at-a-time design) and **keeps the RFC 0097 store cache + RFC 0114 result cache alive across requests** — a per-request `spawn_blocking` with a fresh `StoreCache` would defeat both. |
| `http_post` | Origin check → auth check → parse → dispatch. Request (`id`) → `200` `application/json`. Notification (no `id`) → `202`, empty. Top-level array (pre-2025-06-18 batch) → array of responses. Malformed JSON → `-32700` inside a `200` (same as stdio, not HTTP 400). |
| `http_get` / `http_delete` | `405` with `Allow: POST` — the spec's defined "no server-initiated stream / no sessions here" signal. |
| `origin_allowed` | Loopback (`localhost`/`127.0.0.1`/`[::1]`, any port) + exact `--http-allow-origin` values. A *missing* `Origin` (editors, curl) is allowed by the caller. DNS-rebinding defence (spec requirement). |
| Auth | The existing token (`--token-file`, renamed from `--tcp-token-file` with the old name kept as a clap `alias`; or `EKOS_MCP_TOKEN`) now gates `--http` too, as `Authorization: Bearer <token>` on **every** request (not just `initialize`), constant-time compared. Missing/wrong → `401` + `WWW-Authenticate: Bearer`. |
| CLI (`bin/ekos.rs`) | `--http <ADDR>` (`conflicts_with = "tcp"`), `--http-allow-origin <ORIGIN>` (repeatable, `requires = "http"`), `--token-file` (`alias = "tcp-token-file"`). |
| Deps | `axum` promoted from a demo-server-only dep to a `cli` dep (already in the workspace lock via RFC 0045). `reqwest` added as a `cli` dev-dep for the transport tests. |

### Implementation details worth remembering

- **`KnowledgeStore` is still not `Send`.** RFC 0115 documented this and chose thread-per-connection
  (each with its own cache) rather than adding the bound. HTTP is connectionless, so that model
  doesn't map; instead one dedicated worker thread owns the cache and requests queue to it over a
  channel. `tokio::sync::mpsc` specifically (not `std::sync::mpsc`) because axum `State` must be
  `Send + Sync` and `std::sync::mpsc::Sender` is `!Sync`. The worker calls
  `UnboundedReceiver::blocking_recv` — legal because it is a plain `std::thread`, not a tokio task.
- **No SSE is spec-compliant.** MCP Streamable HTTP requires the server to answer a POST with
  *either* `text/event-stream` *or* `application/json`, and permits `GET` → `405`. A server with
  no server push never needs an event stream. If a future feature needs progress on a long
  `tools/call`, the `GET /mcp` handler becomes a real SSE stream then — the POST path stays valid.
- **No sessions.** The server issues no `Mcp-Session-Id` and requires none; each POST is
  self-contained. `handle_message` was already effectively stateless (the `StoreCache` is a perf
  cache, not protocol state).
- **`--http` replaces stdio**, like `--tcp` — it does not run alongside it. `--http` and `--tcp`
  are mutually exclusive (`clap` `conflicts_with`).

### Decisions (alternatives considered)

- **Per-request `spawn_blocking` + fresh `StoreCache`** — simpler, no worker thread, but reopens
  the ledger every request and defeats the RFC 0114 result cache. Rejected: the worker thread is
  ~15 lines and preserves both caches.
- **Enforce `Accept` / `MCP-Protocol-Version` headers** — the spec says the client MUST send them.
  Server-side enforcement is optional and only hurts compatibility (we always return
  `application/json` regardless). Accepted and ignored.
- **Rename `--tcp-token-file` outright** — would break `web/api`'s `supervisor.py` and its
  `conftest.py`. Kept as a clap alias; canonical name is now `--token-file`.

---

## Knowledge Captured

- **MCP has two standard transports and clients pick one arbitrarily.** stdio = spawn a command;
  Streamable HTTP = a URL. A raw TCP socket (RFC 0115) is *neither* — pointing an HTTP client at
  it fails silently with zero tools, no error. If a user reports "the MCP connection exposes no
  `ekos_*` tools" and their config has a `url:`, they need `--http`, not `--tcp`.
- **Streamable HTTP without SSE is a legitimate, minimal implementation.** The spec's SSE
  machinery is only needed for server-initiated messages. `POST → application/json`, `GET → 405`
  is a complete compliant server when there is no server push.
- **axum `State` must be `Send + Sync`; `std::sync::mpsc::Sender` is `Send` but `!Sync`.** Use
  `tokio::sync::mpsc` for a channel that lives in axum state, even when the receiving end is a
  blocking `std::thread` (`blocking_recv` covers that).
- **`block_in_place` + `Handle::current().block_on`** is the established pattern in this codebase
  for running async (here: `axum::serve`) from a sync fn that is itself already inside
  `#[tokio::main]` — see `run_clickhouse_query_blocking`, `ingest_sources`. A plain
  `Runtime::new().block_on` panics with "Cannot start a runtime from within a runtime".

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0143-mcp-http-transport.md` | New RFC (Accepted per user direction) |
| `ekos/crates/cli/src/commands/mcp.rs` | `serve_http`, `build_http_router`, `http_post`/`http_get`/`http_delete`, `origin_allowed`, worker-thread bridge; 9 transport tests |
| `ekos/crates/cli/src/bin/ekos.rs` | `--http`, `--http-allow-origin`, `--token-file` (alias `tcp-token-file`); dispatch |
| `ekos/crates/cli/Cargo.toml` | `axum` dep; `reqwest` dev-dep |
| `ekos/Cargo.lock` | axum/reqwest pulled into the `ekos` crate's graph |
| `TODO.md` | HTTP transport marked landed; SSE/server-push split out as the remaining open item |
| `README.md` | New "HTTP transport" subsection under AI agent access; `--token-file` rename noted |
| `docs/generated/ekos-self-documentation.html` | `--http` + auth paragraph in the MCP section |
