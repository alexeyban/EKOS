# Devlog 141 — RFC 0115: MCP over TCP

**Date:** 2026-08-31
**PRs:** the commit landed with this devlog
**Branch:** main (direct)

---

## Summary

`ekos mcp serve` (RFC 0013) spoke newline-delimited JSON-RPC 2.0 over stdio only — fine for one
client that spawns the process itself, but it meant every additional MCP-speaking tool (PyCharm's
AI chat, another agent host, a second Claude Code session) needed its own spawned process and its
own cold-opened ledger handle. RFC 0115 adds `ekos mcp serve --tcp <addr>`, reusing the exact
NDJSON-over-TCP pattern this codebase already shipped twice (`coordinator serve`/`query-worker
serve`, RFC 0113). The implementation hit a real `Send`-trait wall partway through that changed the
concurrency design — documented below since it's the non-obvious part.

---

## PR — RFC 0115: MCP over TCP

### Problem / motivation

TODO.md already tracked "MCP HTTP/SSE transport + auth + multi-workspace routing" as a bundled,
unscoped future item. This RFC deliberately ships just the transport half, and picks plain TCP over
HTTP/SSE — no new framework, no request/response framing change, matching the pattern already
proven twice in this codebase. Auth, TLS, and multi-workspace routing stay explicitly out of scope.

### What was built

| Component | What it does |
|---|---|
| `ekos mcp serve --tcp <addr>` | New optional flag; stdio behavior is completely unchanged when omitted |
| `serve_messages()` | The shared dispatch loop both stdio and TCP now call — reads one JSON-RPC line, writes zero-or-one lines back |
| `serve_tcp()` | Binds a `std::net::TcpListener`, one `std::thread::spawn`'d OS thread per accepted connection |

### Implementation details worth remembering

**The original design shared one cache across connections; it doesn't compile.** The plan going in
was to reuse RFC 0097's `StoreCache` across every TCP connection via `Arc<Mutex<StoreCache>>`, so N
concurrent MCP clients would share one cached read-only ledger handle instead of each cold-opening
their own — the natural extension of "long-lived server sessions reuse one cached handle" to
multiple simultaneous sessions. This fails with:

```
error[E0277]: `(dyn KnowledgeStore + 'static)` cannot be sent between threads safely
```

`KnowledgeStore` (`crates/ledger/src/lib.rs`) declares no `Send` bound, so `Box<dyn KnowledgeStore>`
isn't `Send`, so `StoreCache` (which holds one) isn't `Send`, so `Mutex<StoreCache>` isn't `Send`
either — and `std::thread::spawn`'s closure requires everything it captures to be `Send`.

The fix actually considered and rejected: add `Send` to the trait. Every real implementor —
`Ledger`, `FactLedger`, `PartitionedLedger`, `DistributedLedger` — would need an actual audit before
that bound could be added with confidence rather than papering over a genuine concurrency question
with an `unsafe impl Send`. That's real, separately-scoped work, not something to bolt on as a side
effect of a transport RFC.

**What shipped instead:** each TCP connection gets its own independent `StoreCache`, created inside
its own spawned thread and never shared or sent anywhere. This sidesteps `Send` entirely — a
`StoreCache` is created and used within one thread, full stop. Cost: N concurrent clients do N
independent ledger opens instead of one shared one. Per RFC 0097's own numbers a read-only
fact-engine open is fast, so this is a real but accepted v1 cost, not a correctness compromise.
Sharing the cache safely is real, separately-scoped follow-on work once `KnowledgeStore: Send` is a
deliberate design decision rather than an incidental one.

This also simplified `serve_messages()` itself — no `Mutex` to lock per-message, just
`cache: &mut StoreCache` threaded straight through, identical in shape to how stdio always worked.

**Concurrency model matches `handle_message`'s own shape.** `handle_message` is fully synchronous
and blocking (blocking ledger reads, `std::thread::sleep` inside `acquire_write_lock`'s retry) — one
OS thread per connection matches that directly. Mixing blocking calls into an async runtime task
would starve other work on that executor thread instead; `mcp::run` deliberately stays a plain sync
function with no new `tokio` dependency on this path, even though the binary's `main()` already runs
under `#[tokio::main]` for unrelated subcommands.

**No authentication, explicitly.** Same v1 posture RFC 0113 already stated for its own
coordinator/query-worker TCP servers: no auth, no TLS, opt-in only, loopback or a trusted network
only. The startup log line and CLI help both say this plainly.

### Decisions (alternatives considered, why this choice)

- **Plain TCP over HTTP/SSE**: HTTP/SSE is MCP's other standard transport, but it's a materially
  bigger undertaking (real HTTP framework, different request/response framing, CORS for
  browser clients) — deferred as a separate, still-unscoped item, not attempted here.
- **Shared cache vs. per-connection cache**: covered above — per-connection won because it needed no
  changes to a core trait's `Send`-ness, which would need its own audit-first RFC.
- **One thread per connection vs. async tasks**: threads won because `handle_message`'s blocking I/O
  would starve an async executor; this mirrors the exact reasoning already applied to
  `coordinator serve`/`query-worker serve` in RFC 0113.

---

## Knowledge Captured

- **`KnowledgeStore` is not `Send`**, and nothing in the codebase currently requires it to be — the
  first time this session hit a design that wanted to share a `Box<dyn KnowledgeStore>` (or anything
  containing one) across `std::thread::spawn` boundaries. Any future work with the same instinct
  (share one cached handle across concurrent threads) will hit the identical `E0277` and should
  reach for the same per-thread-owns-its-own-handle sidestep unless a real `Send`-safety audit of
  every implementor (`Ledger`, `FactLedger`, `PartitionedLedger`, `DistributedLedger`) has already
  happened elsewhere.
- **`&[u8]` and `&mut Vec<u8>` are enough to unit-test a `BufRead`/`Write`-generic function** — no
  real socket needed to test `serve_messages` end-to-end; a real `TcpStream`-based test is reserved
  for proving the concurrency/isolation property specifically (two real clients, two real threads,
  asserting neither response leaks into the other).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0115-mcp-tcp-transport.md` | New RFC — TCP transport design, including the `Send`-driven pivot to per-connection caches |
| `ekos/crates/cli/src/commands/mcp.rs` | `serve_messages()` (shared dispatch loop), `serve_tcp()` (thread-per-connection, per-connection `StoreCache`), two new tests |
| `ekos/crates/cli/src/bin/ekos.rs` | `--tcp <addr>` flag on `mcp serve`, wired through to `mcp::run` |
| `TODO.md` | TCP transport marked landed; auth/HTTP-SSE/multi-workspace routing left open, split out explicitly |
| `README.md` | AI agent access (MCP) section documents `--tcp` |
| `docs/generated/ekos-self-documentation.html` | §10 (AI agent access) documents the TCP transport |
