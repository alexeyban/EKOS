# RFC 0013 — MCP Server (`ekos mcp serve`)

| Field | Value |
|-------|-------|
| **Status** | Accepted |
| **Author** | EKOS team |
| **Created** | 2026-07-16 |
| **Gating** | AI Layer (v0.7) |

---

## Motivation

RFC 0009 gave EKOS an AI answer path (`ekos ask`), but AI *agents* — Claude Code, Claude
Desktop, any Model Context Protocol client — still have no way to consume compiled knowledge.
The architecture already promises the right integration point: agents consume knowledge through
the read-only Runtime, never through raw enterprise systems. MCP is the de-facto standard for
exposing tools to agents, and the Runtime's query surface (`find_objects`, EKL,
`load_neighborhood`, `reconstruct_state`) maps 1:1 onto MCP tools.

This RFC adds a `mcp serve` CLI subcommand that speaks MCP over stdio, so a coding agent can be
pointed at an EKOS workspace with one line of configuration.

---

## Design

### Transport and protocol

- **stdio transport**: newline-delimited JSON-RPC 2.0 messages on stdin/stdout, per the MCP
  specification. This is the transport every MCP client supports and requires no network
  surface, no auth story, and no new dependencies (hand-rolled dispatch over `serde_json`).
- Logging goes to **stderr** — stdout carries protocol frames only.
- Supported methods: `initialize`, `notifications/initialized` (no-op), `ping`, `tools/list`,
  `tools/call`. Unknown requests get JSON-RPC error `-32601`; notifications are never answered.
- The server echoes the client's requested `protocolVersion`.

### Workspace resolution

`ledger_path` is cwd-derived, but an MCP client launches the server from an arbitrary
directory. `mcp serve` therefore takes `--workspace <DIR>` (default: cwd) and resolves
`.ekos/` under it. The global `--config` flag keeps working for `ekos.toml`.

### Tools

| MCP tool | Wraps | Arguments |
|---|---|---|
| `ekos_search` | `Runtime::find_objects` | `query: string` |
| `ekos_ekl` | `ekl_parse` + `EklInterpreter::execute` | `query: string` |
| `ekos_neighborhood` | `Runtime::load_neighborhood` | `id: string`, `depth?: integer` (default 1) |
| `ekos_state` | `Runtime::reconstruct_state(_at)` | `id: string`, `at?: string` (RFC 3339) |
| `ekos_dependents` | `Runtime::relationships_for` (split by direction) | `id: string` |
| `ekos_diff` | `diff_ledger`, touched ids resolved to names (≤200 listed) | `from: string` (RFC 3339), `to?: string` (default now) |
| `ekos_status` | `Ledger` entry/object/relationship counts | — |

`ekos_dependents` answers impact analysis ("what breaks if this changes?"): incoming edges are
`dependents`, outgoing edges `dependencies`, each resolved to name/kind with the relationship's
properties. `ekos_diff` answers "what knowledge changed since T?" over the append-only ledger —
the two scenarios agents actually run that no code-navigation MCP can serve (added after the
Serena comparison, devlog 15).

Tool results are JSON serialized into a single `text` content block. Tool-level failures
(bad EKL syntax, unknown id, missing ledger) return `isError: true` with the message in the
content block — they are *tool* errors, not protocol errors, so the agent can read and react
to them.

### Invariants preserved

- The server holds the ledger strictly through `Runtime` / read-only `Ledger` queries — no
  write path exists in the handler.
- The ledger is opened per `tools/call`, so the server starts fine before a first build and
  reports a readable error until one exists.

## Non-goals

- HTTP/SSE transport, auth, multi-workspace routing — out of scope until a remote deployment
  story exists.
- MCP resources/prompts capabilities — tools only for v1.
- Write-path tools (triggering builds from the agent) — would violate the read-only runtime
  boundary; builds stay in the CLI.

## Testing

`handle_message` is a pure function of (config, workspace, request-line) → optional
response-line, unit-tested without spawning a process: initialize handshake, tools/list
shape, notification silence, unknown-method error, and `ekos_status`/`ekos_search` against a
temp-dir ledger.
