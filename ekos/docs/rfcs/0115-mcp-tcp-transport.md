# RFC 0115 — MCP over TCP

**Status:** Accepted (per user direction — small, additive, same pattern as RFC 0113's coordinator/query-worker TCP transports)
**Author:** EKOS team
**Created:** 2026-08-31
**Implemented:** 2026-08-31

---

## Motivation

`ekos mcp serve` (RFC 0013) speaks newline-delimited JSON-RPC 2.0 over **stdio only** — the right
default for a single client that spawns the process itself (Claude Code's `claude mcp add ekos --
ekos mcp serve`), but it means every additional MCP-speaking tool that wants to talk to the same
compiled workspace (PyCharm's AI chat, another agent host, a second Claude Code session, anything
else that speaks MCP) needs its own spawned `ekos mcp serve` process and its own cold-opened,
independently-cached read-only ledger handle. There is no way to point two different tools at one
already-running EKOS server.

TODO.md already tracks "MCP HTTP/SSE transport + auth + multi-workspace routing" as a bundled,
unscoped future item. This RFC deliberately does **not** attempt that whole bundle — it ships just
the transport half, using the exact NDJSON-over-TCP pattern this codebase already established twice
(`ekos coordinator serve`, RFC 0113 B3; `ekos query-worker serve`, RFC 0113 B4), not a new HTTP/SSE
stack. Auth and multi-workspace routing stay open, tracked, unscoped — see Non-Goals.

## Design

### `ekos mcp serve --tcp <addr>`

A new optional flag on the existing `mcp serve` subcommand:

```bash
ekos mcp serve --workspace <dir>                    # unchanged: stdio only
ekos mcp serve --workspace <dir> --tcp 127.0.0.1:7331   # also/instead: TCP
```

When `--tcp` is given, `ekos mcp serve` binds a plain `std::net::TcpListener` and accepts
connections until killed — the same "serve forever" shape `coordinator serve`/`query-worker serve`
already have. When it's absent, behavior is byte-for-byte unchanged from before this RFC (stdio
only) — this is purely additive.

### One dispatch core, two transports

`handle_message(config, workspace, line, &mut cache) -> Option<String>` (RFC 0013's original
design) was already transport-agnostic — it takes one line in, produces zero-or-one lines out,
with no assumption about where the bytes came from. The stdio loop and the new TCP loop now both
go through one shared helper:

```rust
fn serve_messages(
    config: &EkosConfig,
    workspace: &Path,
    cache: &Mutex<StoreCache>,
    reader: impl BufRead,
    writer: impl Write,
) -> Result<()>
```

`cache` is locked **per message**, not held for the whole connection — necessary so one slow or
idle TCP client blocked on its next read never starves every other concurrent connection sharing
the same cache. For stdio (still exactly one client, the process's own parent), the lock is
uncontended and free.

### Concurrency model

Each TCP connection is handled on its own `std::thread::spawn` — matching `handle_message`'s own
fully synchronous, blocking design (blocking ledger reads, `std::thread::sleep` in
`acquire_write_lock`'s retry) rather than mixing blocking calls into an async runtime task, which
would starve other work on that executor thread. `mcp::run` itself stays a plain synchronous
function — no `tokio` dependency added to this path — `cargo run -p ekos -- mcp serve` already runs
under the CLI's ambient `#[tokio::main]`, but that's incidental (other subcommands need it); this
one doesn't.

**Each connection gets its own `StoreCache`, not a shared one.** The original design here shared
one `Arc<Mutex<StoreCache>>` across every connection so concurrent clients would reuse one cached
read-only ledger handle — this turned out to need `KnowledgeStore: Send`, which the trait doesn't
declare, and every real implementor (`Ledger`, `FactLedger`, `PartitionedLedger`,
`DistributedLedger`) would need auditing before adding that bound could be done with confidence
rather than papering over a real concurrency hazard with an `unsafe impl`. Not worth doing as a
side effect of a transport RFC. A per-connection cache means N concurrent clients do N independent
opens instead of one shared one — a real, accepted v1 cost (opening a read-only fact-engine handle
is fast per RFC 0097's own numbers, not the pre-097 problem), not a correctness compromise. Sharing
the cache safely is a real, separately-scoped follow-on once `KnowledgeStore: Send` is deliberately
designed, not incidentally bolted on.

A connection ending (client disconnect, malformed input, a real I/O error) only tears down that one
thread and its own cache; every other concurrent connection is unaffected.

### Security posture — explicitly no authentication

Matching RFC 0113's own stated v1 scope for its coordinator/query-worker TCP servers ("v1 assumes a
trusted cluster network"), this transport has **no authentication, no TLS, no access control** of
any kind. Anyone who can reach the bound address gets the exact same read surface stdio already
gives a spawning parent process, plus the two write-capable tools
(`ekos_identity_review`/`ekos_architecture_review`). `--tcp` is opt-in — never enabled unless
explicitly passed — and both the CLI help text and the startup log line say plainly that this is
unauthenticated and must not be exposed beyond a trusted network. `127.0.0.1` (loopback-only) is a
safe default binding for "let more than one local tool connect to one server"; binding `0.0.0.0` or
any externally-reachable address is the caller's explicit choice and explicit risk.

## Non-Goals

- **Authentication/TLS.** A real security boundary needs a real design pass (API keys? mTLS,
  matching the same open question RFC 0113 already has for its coordinator? OS-level socket
  permissions?) — not bolted on here. Tracked, same TODO.md item.
- **HTTP/SSE transport.** MCP's other standard transport (Streamable HTTP, with Server-Sent Events
  for server-to-client push) is a materially different, bigger undertaking — a real HTTP framework,
  request/response framing instead of a raw byte stream, CORS considerations for browser-based
  clients. Not attempted here; still tracked as its own item.
- **Multi-workspace routing.** This server still serves exactly one `--workspace` per process, same
  as stdio mode today — a TCP client doesn't get to pick which workspace per-request. Routing one
  listener across several workspaces is a separate, unscoped feature.
- **Reusing `ekos-cluster`'s coordinator/protocol code.** The coordinator's `Request`/`Response`
  enums are a purpose-built metadata protocol (leases, watermarks, catalog) — unrelated to MCP's
  JSON-RPC 2.0 tool-call shape. Only the *transport pattern* (NDJSON over TCP, one connection per
  client) is reused, not any of that crate's types.

## Testing

- `serve_messages` unit-tested directly against an in-memory `Cursor`/`Vec<u8>` reader/writer pair
  (no real socket needed) — proves the shared dispatch loop behaves identically to the pre-RFC
  stdio-only loop for a scripted request sequence.
- A real TCP integration test: bind an ephemeral (`127.0.0.1:0`) listener, connect two real
  `std::net::TcpStream` clients concurrently, send interleaved requests from both, and assert each
  gets its own correct response — proving concurrent connections are genuinely isolated (each its
  own thread, its own `StoreCache`) rather than serializing or interfering with each other.
- Existing stdio-path tests (`handle_message` called directly) are unchanged and still pass —
  confirms the stdio behavior genuinely didn't change.

## Files Changed

| File | Change |
|---|---|
| `crates/cli/src/commands/mcp.rs` | `serve_messages` (shared dispatch loop), `serve_tcp` (one thread + one independent `StoreCache` per connection) |
| `crates/cli/src/bin/ekos.rs` | `--tcp <addr>` flag on `mcp serve` |
| `TODO.md` | TCP transport marked landed; auth + HTTP/SSE remain open, split out explicitly |
| `README.md` | AI agent access section mentions `--tcp` |
