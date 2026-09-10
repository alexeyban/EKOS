# RFC 0143 — MCP over Streamable HTTP

**Status:** Accepted (per user direction — small, additive, the transport half only, same shape as RFC 0115)
**Author:** EKOS team
**Created:** 2026-09-10
**Builds on:** RFC 0013 (MCP stdio), RFC 0115 (MCP over TCP), RFC 0128 (bearer-token auth), RFC 0097 (cached read-only handle)
**Closes:** the "HTTP/SSE transport" bullet RFC 0115 Non-Goals split out and TODO.md tracked

---

## Motivation

`ekos mcp serve` speaks newline-delimited JSON-RPC 2.0 over **stdio** (RFC 0013) or a **raw TCP
socket** (RFC 0115). Neither is HTTP. Several MCP clients — VS Code / GitHub Copilot agent mode,
Visual Studio 2022, `mcp-remote`, browser-based tools — only offer **stdio** (spawn a command) or
**Streamable HTTP** (`type: "http"`, a URL). They cannot talk to a bare TCP socket, and a user
who configures `url: "http://127.0.0.1:7331"` against the RFC 0115 TCP server gets nothing: the
client speaks HTTP to a socket that only understands JSON-RPC lines, so no `ekos_*` tool ever
appears. This was reported live.

RFC 0115 deliberately deferred this ("a materially different, bigger undertaking — a real HTTP
framework, request/response framing … CORS considerations"). It is smaller than that framing
suggested, because EKOS has **no server-initiated messages**: no subscriptions, no progress
notifications, no resource-change events. The MCP Streamable HTTP spec permits a server with no
server push to answer every POST with a plain `application/json` body and to return `405` for the
SSE `GET` — which removes the SSE machinery, the event store, and session resumption from scope
entirely.

---

## Design

### `ekos mcp serve --http <addr>`

A new optional flag, parallel to `--tcp` and mutually exclusive with it (`clap`
`conflicts_with`):

```bash
ekos mcp serve --workspace <dir>                          # unchanged: stdio only
ekos mcp serve --workspace <dir> --tcp  127.0.0.1:7331    # RFC 0115: raw NDJSON/TCP
ekos mcp serve --workspace <dir> --http 127.0.0.1:7331    # RFC 0143: Streamable HTTP
```

With `--http`, the server binds an `axum` app (already a workspace dependency — RFC 0045's
demo-server) on `<addr>` and serves one route, `/mcp`, until killed. Absent `--http`, behaviour is
byte-for-byte unchanged.

### One dispatch core, three transports

`handle_message(config, workspace, line, &mut cache) -> Option<String>` (RFC 0013) stays the
single dispatch core. The HTTP transport is a thin adapter around it, exactly as `serve_tcp` is:

| HTTP | JSON-RPC |
|---|---|
| `POST /mcp` body = one JSON-RPC **request** (has `id`) | → `handle_message` → `200` `application/json`, body = the response object |
| `POST /mcp` body = one JSON-RPC **notification** (no `id`) | → `handle_message` returns `None` → `202 Accepted`, empty body |
| `POST /mcp` body = a JSON **array** (pre-2025-06-18 batch) | each element dispatched in order; `200` with a JSON array of the responses, or `202` if none produced a response |
| `POST /mcp` body = malformed JSON | `200` `application/json`, body = a JSON-RPC `-32700` parse-error object (same as stdio; not an HTTP 400) |
| `GET /mcp` | `405 Method Not Allowed`, `Allow: POST` — the spec's signal for "no server-initiated SSE stream here" |
| `DELETE /mcp` | `405` — this server is stateless; there is no session to terminate |

### Concurrency model — one worker thread

`axum` runs on the CLI's ambient `#[tokio::main]` multi-thread runtime, but `KnowledgeStore` is
**not `Send`** (RFC 0115 documented why, and why fixing that is its own scoped effort). A single
dedicated `std::thread` owns the `StoreCache`; each HTTP handler forwards its raw line over a
`tokio::sync::mpsc` channel and awaits the response on a `oneshot`. This:

- keeps every `KnowledgeStore` touch on one non-`Send` thread, no `unsafe`, no trait change;
- **preserves** the RFC 0097 store cache and the RFC 0114 result cache across requests (a
  per-request `spawn_blocking` with a fresh `StoreCache` would defeat both);
- serializes all MCP work, which matches `handle_message`'s fully blocking, one-message-at-a-time
  design (the stdio loop has the identical property). A slow `tools/call` blocks the next request
  for its duration — acceptable for a single-user editor session, stated plainly in the docs.

Unlike `serve_tcp`'s thread-per-connection (each with its own cache), HTTP is connectionless: one
worker, one cache, requests queued. N simultaneous editor requests do not each cold-open a ledger.

### Security posture

Same opt-in model as `--tcp`, plus two HTTP-specific additions:

- **Auth (RFC 0128, extended).** The existing token — `--token-file <FILE>` (renamed from
  `--tcp-token-file`, which stays as a hidden alias) or `EKOS_MCP_TOKEN` — now gates `--http`
  too. Over HTTP it is a standard `Authorization: Bearer <token>` header (constant-time compared
  via the existing `ct_eq`), checked on **every** request, not just `initialize`. Missing/wrong →
  `401` with `WWW-Authenticate: Bearer`. Token-less `--http` is unauthenticated, same as
  token-less `--tcp`; the startup log and `--help` say so.
- **Origin validation (DNS-rebinding defence).** The MCP spec requires servers to validate
  `Origin`. A request with **no** `Origin` header (curl, editors — the common case) is allowed;
  a request whose `Origin` host is loopback (`localhost`, `127.0.0.1`, `[::1]`) is allowed;
  anything else → `403`. `--http-allow-origin <ORIGIN>` (repeatable) adds exact-match exceptions.
- Bind default guidance is `127.0.0.1`; binding a non-loopback address without a token logs the
  same warning `--tcp` already does.

`--http` exposes the **same tool surface** as stdio and `--tcp`: all read tools plus the two
write-capable review tools (`ekos_identity_review`, `ekos_architecture_review`). No new tools, no
change to any tool.

### SSE responses on POST (amendment 2026-09-10 — see below)

> The original design shipped `application/json`-only. ChatGPT's connector client requires the
> `text/event-stream` response form. The **Amendment** section at the end of this RFC adds SSE as
> a *response encoding* chosen by content negotiation, still with no `GET` stream and no server
> push.

### Not in scope

- **Server-initiated messages / a `GET /mcp` stream.** No server push (progress on a long
  `tools/call`, resource subscriptions). If a future feature needs that, the `GET /mcp` handler
  becomes a real long-lived SSE stream then — the POST path already returns
  `application/json` and stays valid.
- **Sessions / `Mcp-Session-Id`.** The server issues none and requires none; each POST is
  self-contained. The spec permits this.
- **`MCP-Protocol-Version` header enforcement.** Accepted and ignored; `initialize` still echoes
  the client's `protocolVersion` in the body as before.
- **Multi-workspace routing.** One `--workspace` per process, same as every other transport.
- **TLS.** Terminate upstream (a reverse proxy) if a deployment needs it; loopback doesn't.

---

## Interfaces

```rust
// crates/cli/src/commands/mcp.rs
fn serve_http(
    config: &EkosConfig,
    workspace: &Path,
    addr: &str,
    token: Option<String>,
    allow_origins: &[String],
) -> Result<()>;

// The testable seam — builds the worker thread + router without binding a socket.
fn build_http_router(
    config: &EkosConfig,
    workspace: &Path,
    token: Option<String>,
    allow_origins: Vec<String>,
) -> axum::Router;

// origin allow-list check, unit-tested in isolation
fn origin_allowed(origin: &str, extra: &[String]) -> bool;
```

`mcp::run` gains an `http: Option<&str>` parameter and an `allow_origins: &[String]`; the
`(tcp, http)` pair is dispatched `serve_tcp` / `serve_http` / stdio. `clap` guarantees not-both.

---

## Testing (before implementation)

`build_http_router` bound on `127.0.0.1:0`, driven with `reqwest` (added to `cli` dev-deps —
already a workspace dep):

- `POST /mcp` `initialize` → `200`, `content-type: application/json`, body echoes `protocolVersion`,
  `serverInfo.name == "ekos"`.
- `POST /mcp` `tools/list` → `200`, body has the full tool array (no ledger needed).
- `POST /mcp` a notification (`notifications/initialized`, no `id`) → `202`, empty body.
- `POST /mcp` malformed JSON → `200`, body is a `-32700` JSON-RPC error object.
- `GET /mcp` → `405`, `Allow: POST`.
- `DELETE /mcp` → `405`.
- Auth: no token configured → `initialize` succeeds with no header. Token configured → no header
  and wrong header both `401` (`WWW-Authenticate: Bearer`); correct `Authorization: Bearer` →
  `200`.
- Origin: no `Origin` → allowed; `Origin: http://localhost:5173` and `http://127.0.0.1` → allowed;
  `Origin: https://evil.example` → `403`; same evil origin + `--http-allow-origin https://evil.example`
  → allowed.
- `origin_allowed` unit table: bare host, bracketed IPv6, trailing path, port variations.
- A real end-to-end `tools/call ekos_status` against a seeded `FactLedger` workspace → `200` with
  a status result — proves the worker-thread channel round-trips a real ledger read.
- Existing stdio + TCP tests unchanged and still green.

## Benchmark

Not performance-relevant — the dispatch core and ledger path are unchanged; this adds an
HTTP framing layer and one channel hop in front of the same `handle_message`. No `benchmark/`
addition.

---

## Files Changed

| File | Change |
|---|---|
| `crates/cli/src/commands/mcp.rs` | `serve_http`, `build_http_router`, HTTP handlers, `origin_allowed`, one worker thread + mpsc/oneshot bridge; HTTP transport tests |
| `crates/cli/src/bin/ekos.rs` | `--http <ADDR>` (conflicts_with `tcp`), `--http-allow-origin <ORIGIN>` (repeatable), `--token-file` (alias `tcp-token-file`); dispatch |
| `crates/cli/Cargo.toml` | `axum` dep (was demo-server only); `reqwest` dev-dep |
| `TODO.md` | HTTP transport marked landed; multi-workspace routing + SSE/server-push remain open |
| `README.md`, `docs/generated/ekos-self-documentation.html` | `--http` in the AI-agent-access section; VS Code / Visual Studio `mcp.json` example |

---

## Amendment (2026-09-10) — SSE responses on POST, for ChatGPT

### Why

ChatGPT's Developer-Mode connector (the only way to attach an arbitrary-tool MCP server to
ChatGPT — the Deep Research connector needs `search`/`fetch` tools EKOS doesn't have) drives the
Streamable HTTP transport with `Accept: text/event-stream` and **requires** the response to be an
SSE stream. Against the `application/json`-only server it fails to connect. The MCP spec has
always allowed the server to answer a POST with *either* `application/json` *or*
`text/event-stream` — the original design just picked one. This adds the other, chosen by content
negotiation.

### Design

`http_post`, after it has collected the response line(s) from the worker, picks the encoding:

| Condition | Response |
|---|---|
| no response lines (all notifications) | `202 Accepted`, empty — **unchanged** |
| request's `Accept` header contains `text/event-stream` | `200`, `Content-Type: text/event-stream`, body = one SSE `event: message` / `data: <json>` frame per response line, then the stream ends |
| otherwise (`application/json`, `*/*`, absent) | `200`, `Content-Type: application/json` — **unchanged** |

Still **no** long-lived stream: every response line is already computed before the reply starts,
so the SSE body is written in full and the connection closes — the spec's prescribed behaviour
once "all JSON-RPC responses have been sent". No `GET /mcp` stream, no keep-alive pings, no
sessions. `data:` values are split on any embedded newline into multiple `data:` lines per the
SSE grammar (defensive — `handle_message` emits compact single-line JSON).

A client that sends `Accept: */*` (e.g. plain `curl`) still gets `application/json` — the switch
is only on an explicit `text/event-stream`, so existing behaviour and every existing test is
unchanged. VS Code / Copilot send `application/json, text/event-stream` and now get SSE; their
client handles both.

### Interface delta

```rust
fn wants_sse(headers: &HeaderMap) -> bool;      // Accept contains "text/event-stream"
fn sse_response(response_lines: Vec<String>) -> Response;   // text/event-stream body
```

No CLI change. No new dependency (the body is a plain `String`; `axum::response::sse` is not
needed for a non-streaming reply).

### Testing delta

- `POST` with `Accept: text/event-stream` → `200`, `content-type: text/event-stream`; the body
  parses as SSE and its single `data:` frame is the JSON-RPC `initialize` result.
- Batch (`[initialize, ping]`) + SSE `Accept` → two `data:` frames, ids preserved.
- Notification + SSE `Accept` → still `202`, empty (SSE not used when there is nothing to send).
- `POST` with `Accept: application/json` and with no `Accept` → still `application/json` (the
  existing tests, unchanged).
- `wants_sse` unit table.
