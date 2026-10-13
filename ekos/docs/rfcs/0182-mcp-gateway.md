# RFC 0182 — MCP gateway on the console

**Status:** Accepted — implemented 2026-10-13 (devlog_263)
**Author:** EKOS team
**Created:** 2026-10-13
**Builds on:** RFC 0143 (MCP Streamable HTTP), RFC 0129 (the console's per-workspace MCP supervisor), RFC 0180 (API tokens, roles), RFC 0181 (read scope)
**Part of:** production deployment (0178–0181, this RFC, then 0183 operations)

## Motivation

An agent reaches EKOS through `ekos mcp serve`, which knows nothing of tenants, roles or
scopes: whoever can connect reads everything in that project and can call the two review tools.
RFC 0180 gave agents API tokens, but a token works only against the console's HTTP API, which
no MCP client speaks.

Each registered project also holds an `ekos mcp serve` process from the console's start to its
end. One such server held 13 GB of a 16 GB machine.

## The endpoint

```
POST /mcp/{tenant}/{project}
Authorization: Bearer ekos_…
```

MCP Streamable HTTP, as `ekos mcp serve --http` speaks it (RFC 0143): one JSON-RPC message or a
batch per request; the response is JSON, or a single-shot `text/event-stream` when the request's
`Accept` lists it; a request of notifications only answers `202`; `GET` and `DELETE` are `405`.
There is no session and no server push.

A client configuration:

```json
{"url": "https://ekos.example.com/mcp/default/shop",
 "headers": {"Authorization": "Bearer ekos_…"}}
```

## What the gateway does

| Message | Handling |
|---|---|
| `initialize` | answered by the gateway: the client's protocol version echoed, `tools` capability |
| `ping` | answered by the gateway |
| `tools/list` | the project's own list, without the tools this caller may not use |
| `tools/call` | checked, given the caller's scope, forwarded to the project's server, recorded |
| a notification | accepted, not forwarded |
| anything else | JSON-RPC error `-32601` |

### Who may call what

| Tool | Needs |
|---|---|
| `ekos_identity_review`, `ekos_architecture_review` | `review`: a signed-in person with the `reviewer` role. A token can never hold it (RFC 0180), so no agent reaches these through the gateway |
| `ekos_session_note` | `operate`: it writes the session inbox |
| every other tool | `read` |

A caller who sees part of the project (RFC 0181) is offered and may call only the tools that
read solely through the ledger. The project's server enforces the same list; the gateway applies
it first so the caller's tool list is true.

### The scope cannot be supplied by the caller

The gateway builds the forwarded call from the tool name and arguments alone and attaches the
scope it computed from the caller's bindings. Anything the client sent in `_meta` is dropped. A
caller therefore cannot widen their scope, or send one at all.

### Visibility

A project the caller has no role in answers `404`, as does one that does not exist. A missing or
invalid credential answers `401`.

### Audit

Every `tools/call` is recorded in the tenant's audit log, reads included: who, which tool, which
project, allowed or denied. Arguments are not recorded; a query may contain what the caller is
looking for.

## Servers on demand

The supervisor no longer starts every project's server when the console starts.

- A server starts when something first needs it: a console page, or a gateway call.
- A server unused for `EKOS_CONSOLE_MCP_IDLE_SECONDS` (default 900; `0` keeps them) is stopped.
- At most `EKOS_CONSOLE_MCP_MAX_SERVERS` (default 8) run at once. Starting one more stops the
  one used longest ago.

A stopped server's project shows `idle` in the workspace list. The first call after a stop
waits for the server to start.

## Not built

- OAuth for MCP clients. The credential is a bearer token set in the client's configuration.
- Rate limits per token.
- Resources and prompts: the gateway serves tools only, as the servers do.
- A limit on a single call's duration beyond the MCP client's own timeout.
- Evicting a server in the middle of a long call is avoided only by "least recently used"; a
  call longer than the idle time on a server at the cap can be cut.

## Verification

- Protocol: `initialize`, `ping`, notification → `202`, batch, SSE on `Accept`, `GET` → `405`,
  malformed JSON, unknown method.
- Access: no credential → `401`; another tenant's project and an unknown one → the same `404`;
  a viewer token lists no write tool and is refused each; a restricted token lists only the
  scoped tools and every forwarded call carries its scope; a `_meta` scope sent by the client
  is ignored.
- The Python list of scoped tools equals the one in `commands/mcp.rs`, read from its source.
- Audit: each call appears once, with its outcome and without its arguments.
- Supervisor: nothing starts at console start; a server starts on first use; an idle one is
  stopped and starts again on the next use; the cap evicts the least recently used.
- With the real binary: a token lists tools and searches through the gateway; a deny rule added
  to it hides the matching objects on the next call.
