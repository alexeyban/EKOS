# Devlog 222 — Audit fixes: a forgeable console session, an MCP server one request could wedge

**Date:** 2026-09-28
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

A reliability/security/maintainability check of `main` (ac3d403) using real tools and real
measurements rather than reading alone: `cargo audit`, a secret scan of the git history, 101
hostile messages against the running MCP server, and traversal timing on EKOS's own ledger. It found
one critical console flaw, one dependency advisory and four MCP server weaknesses. All six are fixed
here, each with a test that fails without the fix.

---

## The findings and fixes

| # | Finding | Fix |
|---|---|---|
| 1 | **Console session forgery.** `session_secret` defaulted to a literal in this public repo. The session cookie is checked *first, in every auth mode*, so with no secret set anyone could sign `{"role": "write"}` and run commands, even with no write token configured. | Placeholder or empty → random per-process secret plus a warning. A test forges a cookie with the published string and expects `401` (it got through before). |
| 2 | **rustls 0.23.41, RUSTSEC-2026-0285** (TLS 1.3 handshake messages accepted across encryption levels), via `reqwest`. | Lockfile bump to 0.23.45 in `ekos/` and `tests/integration/`; `cargo audit` now shows 0 vulnerabilities in all three workspaces. |
| 3 | **`ekos_neighborhood` had no bound.** One hub object on this ledger: depth 2 = 3.6 s, depth 3 = 28 s / 14 MB (release); depth 5 never finished. `--http` serves every client from one worker. `u64 as u32` wrapped 4294967296 to 0. | `depth` 0-3; a `max_objects` budget (default 500) with `truncated: true`; `max_hops` ≤20 (impact) / ≤200 (transformation). Out of range → refused with the limit named. Depth 3 now returns 1 MB, flagged truncated. |
| 4 | **A panic permanently wedged `--http`.** The worker thread died, the port stayed open, every later request got `500`. On stdio a panic killed the server. | `handle_message_isolated`: `catch_unwind`, a `-32603` for that request only, cache dropped (a poisoned lock may be inside it). Release is `panic = "unwind"`, checked. |
| 5 | **Unbounded `--http` queue.** | 64 waiting requests, then `503` + `Retry-After`. |
| 6 | **`--tcp` could be exhausted before auth.** One thread per connection with no limit, and `lines()` buffered a newline-free first line (the one carrying the token) without limit. | 4 MB line cap on all transports, 64-connection cap with a drop guard, and a warning for token-less non-loopback binds (`--http` already had one). |

Compose files now publish the console and the Migrate sandboxes on `127.0.0.1` only.

## Checked and fine

- **Hostile input:** 101 messages (empty and multibyte strings, 20 KB strings, NUL, broken EKL,
  non-object arguments, `null` params, invalid JSON) produced 101 responses and zero panics.
- **Secrets:** the history scan found only AWS's documented example key, obvious fakes and truncated
  PEM test fixtures.
- **Console subprocesses:** `exec` only, allowlisted commands, no path parameters.
- **`unsafe`:** clean (devlog_221).

---

## Knowledge Captured

**The first explanation for a slowdown was wrong, and measuring showed it.**
`load_neighborhood` did deduplicate edges with a linear scan per edge, which is quadratic, and it's
fixed. But the timings after the fix were identical. The real cost is `relationships_for`
reconstructing every candidate edge from facts, about 15-20 ms each in debug. The bound contains it
for MCP; the ledger fix is a TODO item. A plausible cause is not a measured one.

**A session cookie is only as secret as its key.** The bearer tokens were compared in constant time,
and it didn't matter: the cookie path ran first and was signed with a string in the repo. When
auth has several paths, audit each one; the weakest decides.

**Refuse, don't clamp.** An agent that asks for depth 10 and silently gets 3 reasons over a graph it
believes is complete. The same goes for truncation: the flag is in the return type, not a log line.

**Mutation-check the regression tests.** Swapping `handle_message_isolated` back to
`handle_message_with` made both panic tests fail. That is the evidence the tests guard the fix.

**The RFC 0151 guard earned its keep.** The new `pub fn load_neighborhood_bounded` failed
`every_object_returning_read_path_has_an_audited_session_decision` until it had its own
session-isolation assertions, including on the truncation path.

**`[skip ci]` hides more than test failures.** A known advisory, and two lockfiles in the separate
workspaces that had drifted (`benchmark/` still listed the local crates at 0.1.0), sat unnoticed
because the job that would have surfaced them never ran.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/runtime/src/lib.rs` | `load_neighborhood_bounded` (budget + truncation flag, no dangling edges); hash-set edge dedup; 2 tests + session-isolation coverage |
| `ekos/crates/cli/src/commands/mcp.rs` | `bounded_arg` + caps + schema `minimum`/`maximum`; `handle_message_isolated`; `read_bounded_line`; bounded `--http` queue; `--tcp` connection cap + warning; 8 tests |
| `web/api/app/settings.py` | Placeholder session secret → random per process; default-token warning |
| `web/api/tests/test_auth.py` | Forged-cookie regression test |
| `web/docker-compose.yml`, `docker-compose.migrate.yml` | Ports bound to `127.0.0.1` |
| `ekos/Cargo.lock`, `tests/integration/Cargo.lock`, `benchmark/Cargo.lock` | rustls 0.23.45; stale lockfiles refreshed |
| `README.md`, `docs/generated/ekos-self-documentation.html` | Traversal bounds, transport limits, console secret behaviour |
| `TODO.md` | Audit section: done items and 8 follow-ups |
