# RFC 0127 — Web Console: a browser surface over a compiled workspace

**Status:** Accepted (2026-09-02) — Phase 0 R1/R2/R3 landing per `devlog_150`
**Author:** EKOS team
**Created:** 2026-09-02
**Umbrella.** Per-increment implementation RFCs authored just-in-time (0128+), same pattern as
RFC 0118 → 0119–0126.
**Numbering note.** RFC 0118 §"Prior art" and TODO.md reserved *0127* in prose for a future
computed-staleness/drift RFC; this console RFC took the number instead. When the staleness/drift
RFC is authored it gets a fresh number (0128+), and the RFC 0118 / TODO.md cross-references are
re-pointed then.
**Depends on:** RFC 0013 (MCP server), RFC 0115 (MCP over TCP), RFC 0097 (cached read-only store
handle), RFC 0114 (query usage log), RFC 0116 (`ekos status`), RFC 0104 (cross-process write lock).

---

## 1. Motivation

Every way to look at a compiled EKOS workspace today is either a terminal or an agent:

| Surface | Consumer | What it can't do |
|---|---|---|
| CLI (`build`/`recover`/`compile`/`commit`/`status`/`ekl`/`ask`) | a human at a shell | show shape; nothing is visual |
| MCP over stdio/TCP (RFC 0013/0115) | an AI agent | it isn't a UI; per-object tools only |
| `crates/demo-server` (axum) | a browser | pre-baked static docs + one `POST /ask`; a fixed two-repo catalog, explicitly not general ingestion (RFC 0045) |
| `docs-gen` SVG diagrams (RFC 0073/0083/0102) | a browser | static, generated at build time, not navigable |

The gap is not "EKOS needs a prettier CLI." It is that the product's central claim —
*cross-system impact analysis with a traceable evidence chain* — is currently an assertion a
reader has to take on faith, because the only way to see an impact trace is to read
`ekos_impact`'s JSON. The evidence chain has no visual form. Ingestion speed and token reduction
have real published benchmarks; trust does not.

A web console closes that, and four operational gaps alongside it:

1. **Scan configuration** — `ekos.toml`'s `[observe] paths`/`ignore-patterns` are hand-edited, and
   getting them wrong is expensive in a way that is not obvious. Devlog 43 established the hard
   fact: the ledger is append-only, so narrowing `ignore-patterns` never retroactively removes
   already-compiled data — the only remedy is a full `.ekos/` wipe and rebuild. A UI that shows
   what a path change *will* and *will not* do is worth more than a config form.
2. **Scheduling** — there is no scheduler anywhere in the workspace. Recompilation is manual or
   whatever cron the operator wrote themselves.
3. **Storage statistics** — `ekos status --storage` reports real numbers, in prose, to a terminal.
4. **Command execution** — running the pipeline requires shell access to the machine holding the
   workspace.

## 2. Architecture

```
Browser (React + three.js)
      │ HTTPS / SSE / WS
      ▼
FastAPI console  (auth · job queue · scheduler · aggregation · caching)
      ├── NDJSON over TCP ──►  ekos mcp serve --tcp 127.0.0.1:7331     (reads)
      └── subprocess       ──►  ekos build|recover|resolve|compile|commit  (writes, long-running)
```

### 2.1 Why the console is Python, not another Rust HTTP server

`crates/demo-server` already exists and is axum-based, so extending it is the apparently obvious
choice. It is the wrong one, for a reason the codebase already discovered twice and wrote down:

- **RFC 0045** hit it first: `dyn KnowledgeStore` is not `Sync`, and axum requires handler futures
  to be `Send`. The fix was a per-request `spawn_blocking` thread with its own throwaway
  single-threaded runtime — a narrow adapter-layer workaround, explicitly not a change to shared
  infrastructure.
- **RFC 0115** hit it again: `KnowledgeStore` declares no `Send` bound, so `Box<dyn KnowledgeStore>`
  isn't `Send`, so `StoreCache` isn't, so sharing one cached handle across TCP connections fails
  with `E0277`. The considered-and-rejected fix — adding `Send` to the trait — needs a real audit
  of `Ledger`, `FactLedger`, `PartitionedLedger`, and `DistributedLedger` before it can be done
  with confidence rather than papered over with `unsafe impl Send`.

A console needs long-lived background jobs, a persistent scheduler, streaming log fan-out, and
concurrent request handling. Every one of those pushes on exactly the constraint those two RFCs
documented. Putting the concurrency in Python keeps the Rust side in the shape it already is —
synchronous, one owner per handle — and does not make the eventual `KnowledgeStore: Send` decision
a side effect of a UI RFC.

**The accepted cost**, stated plainly: a second runtime in the deployment, an extra network hop on
every read, and an API surface defined in two languages. Mitigated by generating the TypeScript
client from FastAPI's OpenAPI schema (§7.1) so the browser↔console contract is never hand-written
twice, and by a Compose file so "run it" is one command.

### 2.2 Why MCP for reads and subprocess for writes

The MCP TCP server already holds a warm, cached, read-only ledger handle (RFC 0097) and already
speaks a stable tool contract. Reads go there. Writes (`build`, `recover`, `resolve`, `compile`,
`commit`, `ledger migrate`, `artifact repack`, `ledger repair`) are minutes-long, take the RFC 0104
cross-process write lock, and produce streaming output — those are subprocesses, supervised by the
console's own job runner (§6.3). No write ever goes through MCP; the server's "one write-capable
tool" invariant (`ekos_identity_review` / `ekos_architecture_review`) is untouched.

## 3. Rust-side prerequisites

Four additions to the Rust workspace. R1 and R2 are **gating** — without them the console parses
human-readable CLI output with regexes, which will break on the first formatting change. R3 and R4
are optional and can land later.

| # | Change | Gating |
|---|---|---|
| **R1** | `ekos graph export` — bulk graph extraction as JSON | yes |
| **R2** | `--json` on `ekos status` / `ekos ledger status` | yes |
| **R3** | `ekos_graph_export` MCP tool (wrapper over R1) | no |
| **R4** | Optional shared-secret auth on `ekos mcp serve --tcp` | no |

---

## 4. R1 — `ekos graph export`

### 4.1 Problem

There is **no bulk graph extraction path** anywhere in EKOS today. Every existing read is
per-object or capped:

- `ekos_neighborhood` / `ekos_state` / `ekos_dependents` / `ekos_impact` — all take a `KirId`, one
  object at a time. Building a whole-workspace graph from them is N round trips.
- `find_objects` (SQLite `Ledger`) hard-caps its FTS query at `LIMIT 50` regardless of the caller's
  requested limit (confirmed in RFC 0124's own note about `ekos_search`'s new `limit` param).
- `retrieve` / `RankedResults` (RFC 0119) is a *ranked retrieval* API — it answers "what matches
  this query," not "give me the graph."
- EKL `FIND Object` returns rows, but EKL has no Object+Relationship `JOIN` in one query — TODO.md
  records this as the one extension that breaks EKL's flat-clause-type design, deferred to its own
  future RFC.
- `docs-gen`'s SVG renderers (`system_context_graph`, `dependency_graph_groups`,
  `er_diagram_graph`) do build node/edge sets, but each is a private helper shaped for one diagram,
  written to disk at doc-generation time.

So R1 is genuinely new capability, not a convenience wrapper.

### 4.2 Where the logic lives

A pure function in `ekos-runtime` (read-only by design invariant, RFC 0005 — this is a read):

```rust
// crates/runtime/src/graph_export.rs

pub struct GraphExportOptions {
    pub level: ExportLevel,                    // Object | Aggregate
    pub kinds: Option<Vec<ObjectKind>>,        // include-list; None = all
    pub rel_kinds: Option<Vec<RelationshipKind>>,
    pub exclude_rel_kinds: Vec<RelationshipKind>,
    pub group_by: GroupBy,                     // Kind | PathPrefix { depth: usize }
    pub max_nodes: usize,                      // default 5_000
    pub max_edges: usize,                      // default 20_000
    pub min_degree: usize,                     // default 0
    pub include_properties: Vec<String>,       // property keys to carry into node payloads
}

pub fn export_graph(
    store: &dyn KnowledgeStore,
    opts: &GraphExportOptions,
) -> Result<GraphExport, RuntimeError>;
```

Both the CLI command (`crates/cli/src/commands/graph.rs`) and the R3 MCP tool call this one
function. This is the same anti-drift discipline RFC 0102 applied when it hoisted
`MAX_GRAPH_EDGES` and factored `dependency_graph_groups` so the Markdown and SVG writers could not
silently disagree — the exact failure shape CLAUDE.md already names for identity resolution and the
two ledger backends.

Data source: `KnowledgeStore::all_objects()` + `all_relationships()`, already on the trait (both
are in the `DistributedLedger` `all_*` set served by `QueryWorker`, RFC 0113 B4a).

### 4.3 CLI surface

```
ekos graph export [--workspace <dir>]
                  [--level object|aggregate]        (default: object)
                  [--format json|ndjson]            (default: json)
                  [--kind <ObjectKind>]...          repeatable include-list
                  [--rel-kind <Kind>]...            repeatable include-list
                  [--exclude-rel-kind <Kind>]...    repeatable
                  [--group-by kind|path-prefix]     (default: kind; --level aggregate only)
                  [--path-prefix-depth <n>]         (default: 2)
                  [--max-nodes <n>]                 (default: 5000)
                  [--max-edges <n>]                 (default: 20000)
                  [--min-degree <n>]                (default: 0)
                  [--include-property <key>]...     repeatable
                  [--output <file>]                 (default: stdout)
```

### 4.4 Wire format — `--level object`

```jsonc
{
  "schema_version": 1,
  "workspace": "/abs/path/to/workspace",
  "generated_at": "2026-09-02T10:14:33Z",
  "level": "object",
  "id_space": "kir",

  "counts": {
    "total_objects": 5533,
    "total_relationships": 5136,
    "objects_after_filter": 5210,
    "relationships_after_filter": 4102,
    "returned_nodes": 5000,
    "returned_edges": 3987
  },
  "truncated": {
    "nodes": true,  "node_limit": 5000,
    "edges": false, "edge_limit": 20000,
    "selection": "degree_desc"
  },
  "filters": {
    "kinds": null,
    "rel_kinds": null,
    "exclude_rel_kinds": ["Custom(\"CoupledWith\")"],
    "min_degree": 0
  },

  "kind_index":     ["Crate", "File", "Person", "Symbol", "Table", "Technology"],
  "rel_kind_index": ["Contains", "DependsOn", "ForeignKey", "References"],

  "nodes": [
    { "id": "0f3c…-uuid", "n": "orders",       "k": 4, "d": 17, "p": { "path": "sql/orders.sql" } },
    { "id": "9ab1…-uuid", "n": "customers",    "k": 4, "d": 12, "p": {} }
  ],
  "edges": [
    { "s": 0, "t": 1, "k": 2 }
  ]
}
```

**Format decisions, each with a reason:**

- **Edges reference nodes by array index (`s`/`t`), not by `KirId`.** At 20 000 edges, two 36-char
  UUIDs per edge is ~1.4 MB of pure identifier text. Indices cut the edge payload by roughly 5×.
  Node ids stay full `KirId` strings because the client must be able to call `ekos_state`,
  `ekos_neighborhood`, and `ekos_impact` with them.
- **`k` is an index into `kind_index` / `rel_kind_index`.** Same reason — kind strings repeat on
  every element.
- **Short keys (`n`, `k`, `d`, `p`, `s`, `t`).** At this element count JSON key overhead is a real
  fraction of the payload. The long names live in this spec and in the generated TypeScript types,
  not on the wire.
- **`d` (degree) is computed over the *post-filter* edge set**, not the full graph, and this is
  stated in the schema rather than left for a reader to assume. A node's degree changes when you
  exclude a relationship kind; pretending otherwise would make the UI's node sizing quietly wrong.
- **`p` carries only the properties named in `--include-property`.** Default empty. Objects can
  carry `excerpt`, `symbols`, `ocr_text`, `ai_overview`, `ai_usage` — dumping those into a
  5 000-node export produces tens of megabytes of prose the visualiser will never render.

### 4.5 Wire format — `--level aggregate`

```jsonc
{
  "schema_version": 1,
  "level": "aggregate",
  "id_space": "synthetic",
  "group_by": "kind",
  "nodes": [
    { "id": "kind:File",  "n": "File",  "k": 1, "count": 2192, "d": 5 },
    { "id": "kind:Table", "n": "Table", "k": 4, "count":   41, "d": 3 }
  ],
  "edges": [
    { "s": 0, "t": 1, "k": 1, "w": 431 }
  ]
}
```

`id_space: "synthetic"` is load-bearing: these ids (`kind:File`, `path:crates/ledger`) are **not**
`KirId`s and must never be passed to `ekos_state`. A client that ignores `id_space` and tries will
get a clean "unknown id" tool error rather than silent nonsense, but the flag exists so it doesn't
have to find out that way. `w` is the number of underlying object-level relationships collapsed
into the group edge.

`--group-by path-prefix` groups by the first *n* segments of the object's `path` property
(`--path-prefix-depth`, default 2 → `crates/ledger`, `plugins/file`, `sql/staging`). Objects with
no `path` property fall into a single explicit `path:<unpathed>` group — never silently dropped.

### 4.6 Truncation is reported, never silent

When the filtered graph exceeds `--max-nodes`, nodes are selected by **degree descending, ties
broken by `KirId`** — deterministic, and it yields the structurally interesting core rather than an
arbitrary prefix. Edges are then restricted to those whose endpoints both survived, and truncated
by the same rule if still over `--max-edges`.

The `truncated` block always reports what happened, and the console always surfaces it in the UI
("showing 5 000 of 21 992 objects — most-connected first"). This is the same posture `docs-gen`
already takes when a dependency graph exceeds `MAX_GRAPH_EDGES` and falls back to an explicit
"omitted, too large" note instead of drawing a misleading partial diagram.

This matters more than it looks. Devlog 14 records a real estate-scale run that produced
**6 016 182 `CoupledWith` relationships** from one workspace before the co-change quadratic blowup
was fixed. An export path that can be handed a graph of that size and answers by silently returning
its first 20 000 edges would be actively misleading.

### 4.7 Determinism

Two runs against an unchanged ledger produce byte-identical output. Nodes sort by
`(kind, name, id)`; edges by `(source_index, target_index, kind_index)`; `kind_index` and
`rel_kind_index` sort lexicographically. `generated_at` is the one non-deterministic field and is
excluded from the determinism test. This matches the discipline `layer_nodes` already follows
(Kahn's algorithm with ties broken by node id, RFC 0073).

### 4.8 Deliberately not in R1

- **`--as-of <timestamp>`.** Point-in-time reads exist per-object (`object_at`, checkpoints
  RFC 0106) *and* in bulk — `all_objects_at` / `all_relationships_at` are already on
  `KnowledgeStore` (RFC 0096). A historical whole-graph export is still separately-scoped work
  (`GraphExportOptions.as_of` switching the two fetch calls), just not blocked on a missing
  primitive. Named here so it isn't rediscovered as a surprise.
- **Server-side layout.** No coordinates in the export. Layout is the console's job (§7.3) — the
  Rust side does not learn about force-directed graph drawing.
- **Default relationship exclusions.** `ekos graph export` with no flags exports every relationship
  kind. A built-in silent exclusion of `CoupledWith` would be convenient and dishonest; the
  *console* passes `--exclude-rel-kind` explicitly and shows in the UI that it did.
- **Streaming for very large graphs.** `--format ndjson` emits one JSON object per line (header,
  then nodes, then edges) for pipeline use, but the whole graph is still materialised in memory
  first. True streaming export is deferred until a real workspace needs it.

### 4.9 Testing

- Unit tests over the existing ecommerce SQL fixture: node/edge counts match a direct
  `all_objects`/`all_relationships` count; kind filtering removes exactly the expected kinds.
- **Determinism:** two `export_graph` calls on the same fixture produce identical output modulo
  `generated_at`.
- **Truncation:** a synthetic graph above `max_nodes` returns exactly `max_nodes` nodes, reports
  `truncated.nodes = true`, and the returned set is the top-degree set (asserted against a directly
  computed degree ranking).
- **Aggregate consistency:** `Σ node.count` over an aggregate export equals `objects_after_filter`
  of the equivalent object-level export; `Σ edge.w` equals `relationships_after_filter`.
- **Both backends:** the same assertions run against a SQLite `Ledger` workspace and a `FactLedger`
  workspace, since the default flipped in 2026-08-21 and pre-existing workspaces stay on SQLite.
- **Real-data check** (the project's primary bug-finding mechanism, per devlog 35): run against
  this repo's own populated `.ekos/` and against the Plausible workspace, and record node/edge
  counts and wall-clock in the RFC before marking it implemented.

---

## 5. R2 — machine-readable status

`ekos status --json` / `ekos ledger status --storage --json`:

```jsonc
{
  "schema_version": 1,
  "workspace": "/abs/path",
  "backend": "fact-segment",          // "sqlite-v1" | "sqlite-v2" | "fact-segment" | "partitioned" | "distributed"
  "entries": 20793,
  "objects": 5533,
  "relationships": 5136,
  "evidence": 3154,
  "integrity": "ok",                   // "ok" | "tampered" | "unchecked"
  "last_write": "2026-09-01T18:22:04Z",
  "storage": {
    "total_bytes": 68157440,
    "components": [
      { "name": "ledger/facts/segments", "bytes": 41943040, "files": 37 },
      { "name": "ledger/facts/search",   "bytes": 18874368, "files": 12 },
      { "name": "artifacts",             "bytes":  7340032, "files":  4 }
    ]
  }
}
```

**One deliberate divergence from RFC 0116.** That RFC explicitly declined relationship-count parity
between the CLI (`entries`/`objects`) and the MCP `ekos_status` tool
(`entries`/`objects`/`relationships`), as separate scope. The console's dashboard needs the
relationship count, and having `--json` report a different set of fields than the same command's
text output would be worse than closing the gap. So `--json` reports relationships; the **text
output is unchanged**, preserving RFC 0116's byte-identical-output property between `ekos status`
and `ekos ledger status`.

## 6. R3 — `ekos_graph_export` MCP tool

A thin wrapper over §4.2's `export_graph`, so the console uses one transport for all reads.

```jsonc
{ "name": "ekos_graph_export",
  "description": "Bulk graph extraction: nodes and edges for the whole compiled workspace, filtered and optionally aggregated. Use for visualisation and structural overview, not for answering questions about one entity.",
  "inputSchema": { "type": "object",
    "properties": {
      "level": { "type": "string", "enum": ["object", "aggregate"] },
      "kinds": { "type": "array", "items": { "type": "string" } },
      "rel_kinds": { "type": "array", "items": { "type": "string" } },
      "exclude_rel_kinds": { "type": "array", "items": { "type": "string" } },
      "group_by": { "type": "string", "enum": ["kind", "path_prefix"] },
      "max_nodes": { "type": "integer" },
      "max_edges": { "type": "integer" },
      "min_degree": { "type": "integer" },
      "include_properties": { "type": "array", "items": { "type": "string" } }
    } } }
```

Read-only. Cost class **`Expensive`** in the RFC 0114 query log — it walks the whole store, so it
is exactly the shape the opportunistic result cache exists for: an identical repeat while the
workspace hasn't changed underneath it is served from cache.

## 7. R4 — optional auth on the MCP TCP transport

RFC 0115 is explicit: `--tcp` has no authentication, no TLS, no access control, and exposes the
read surface plus two write-capable tools to anyone who can reach the address. Its stated mitigation
is loopback-only binding.

The console preserves that (§9.3): `ekos mcp serve --tcp 127.0.0.1:7331`, never published, with
FastAPI as the only reachable surface. R4 is defence in depth, not the primary control:

```
ekos mcp serve --tcp 127.0.0.1:7331 --tcp-token-file /run/secrets/ekos-mcp-token
```

The first message on a connection must be `initialize` carrying `params._meta.token`; a mismatch
returns a JSON-RPC error and closes. Also readable from `EKOS_MCP_TOKEN`. Constant-time comparison.
This is a bearer token over a plaintext socket — it defends against a process on the same host
connecting casually, **not** against a network attacker. TLS remains out of scope, as in RFC 0115
and RFC 0113.

---

## 8. Console design (FastAPI)

### 8.1 Stack

| Concern | Choice | Rejected alternative |
|---|---|---|
| Framework | FastAPI + Uvicorn, Python 3.12 | — |
| Schemas | Pydantic v2 | — |
| MCP client | ~150-line asyncio NDJSON/TCP client with a connection pool | the official `mcp` SDK — oriented at stdio and Streamable HTTP; this transport is raw NDJSON |
| Scheduler | APScheduler 3.x, `SQLAlchemyJobStore` on SQLite | Celery/Redis — a broker is disproportionate for a single-operator tool; ARQ is the escape hatch if that changes |
| Job execution | `asyncio.create_subprocess_exec` + bounded queue + per-workspace mutex | `BackgroundTasks` — no persistence, no cancellation, dies with the request |
| `ekos.toml` editing | `tomlkit` | `tomli-w` — destroys comments and formatting |
| Console state | SQLite + SQLModel at `.ekos-web/console.db` | the ledger — it is append-only and is not a place for UI state |
| Run logs | files at `.ekos-web/runs/<run_id>.log`, tailed over SSE | a database — multi-megabyte build logs do not belong in rows |
| Packaging | `uv` | — |

### 8.2 Module layout

```
web/
├── api/
│   ├── app/
│   │   ├── main.py           # app factory, auth middleware, static mount
│   │   ├── mcp_client.py     # NDJSON/TCP client, pool, retry, per-workspace routing
│   │   ├── runner.py         # job queue, subprocess supervision, per-workspace mutex
│   │   ├── scheduler.py      # APScheduler wiring, chain definitions
│   │   ├── commands.py       # COMMAND_ALLOWLIST — see §8.4
│   │   ├── graph.py          # LOD policy, caching, layout precompute
│   │   ├── config_io.py      # tomlkit read/patch/validate, scan preview
│   │   ├── models.py         # SQLModel: Workspace, Run, Schedule, User
│   │   └── schemas.py        # Pydantic request/response models
│   ├── tests/
│   └── pyproject.toml
├── ui/                       # Vite + React + TypeScript
└── docker-compose.yml
```

### 8.3 HTTP surface

```
# Workspaces
GET    /api/workspaces
POST   /api/workspaces                              # register a path containing ekos.toml
GET    /api/workspaces/{id}/health                  # wraps `ekos doctor`

# Scan configuration
GET    /api/workspaces/{id}/config                  # parsed ekos.toml
PUT    /api/workspaces/{id}/config                  # tomlkit patch; comments preserved
POST   /api/workspaces/{id}/config/validate
POST   /api/workspaces/{id}/config/preview-scan     # file count under current paths/ignore-patterns

# Statistics
GET    /api/workspaces/{id}/stats                   # R2 --json
GET    /api/workspaces/{id}/stats/kinds             # EKL COUNT/GROUP BY (RFC 0096)
GET    /api/workspaces/{id}/stats/timeline          # entries over time, from run history
GET    /api/workspaces/{id}/stats/queries           # aggregated .ekos/query-log.jsonl (RFC 0114)

# Graph
GET    /api/workspaces/{id}/graph                   # R1, level/filters as query params
GET    /api/workspaces/{id}/graph/neighborhood/{object_id}?depth=
GET    /api/workspaces/{id}/graph/impact/{object_id}?depth=&kinds=
GET    /api/workspaces/{id}/search?q=&limit=
GET    /api/workspaces/{id}/objects/{object_id}     # ekos_state + resolved evidence

# Commands and runs
GET    /api/commands                                # allowlist catalogue with JSON Schema per command
POST   /api/workspaces/{id}/commands/{name}         -> { run_id }
GET    /api/runs?workspace=&status=
GET    /api/runs/{run_id}
GET    /api/runs/{run_id}/logs                      # SSE tail
POST   /api/runs/{run_id}/cancel

# Schedules
GET|POST|PATCH|DELETE /api/schedules
POST   /api/schedules/{id}/run-now
```

### 8.4 Command execution — the allowlist is non-negotiable

"Run EKOS commands from a browser" is a remote-code-execution surface. Four rules, none optional:

1. **A hardcoded allowlist is the only way to run anything.** `commands.py` holds
   `{name, argv_template, param_schema, is_write, timeout, requires_role}`. There is no endpoint
   anywhere that accepts a command string.
2. **Never `shell=True`.** `create_subprocess_exec` with an argument list only. No interpolation
   into a shell.
3. **Path parameters are validated against registered workspace roots** after `Path.resolve()`,
   rejecting anything outside — `..` traversal cannot reach the filesystem.
4. **Write commands need a role.** `ekos_identity_review` and `ekos_architecture_review` (the only
   two write-capable MCP tools) and every pipeline write command sit behind a separate permission
   from read access.

Initial allowlist: `doctor`, `init`, `build`, `recover`, `resolve`, `compile`, `commit`, `clean`,
`status`, `graph export`, `ekl`, `ask`, `ledger status`, `ledger repair`, `ledger migrate`,
`artifact repack`, `docs generate`, `identity review`.

### 8.5 Job runner

One bounded queue per workspace, plus a **per-workspace mutex**: EKOS takes a real cross-process
file lock on writes (RFC 0104), so two concurrent `build`s on one workspace is a guaranteed
conflict, not a race worth discovering in production. Cancellation sends `SIGTERM`, then `SIGKILL`
after a grace period, and the run is recorded as cancelled — an interrupted `commit` is safe
because commits are idempotent (entry ids derive from content hashes; a re-run skips entries
already present).

Chained runs (`build → recover → resolve → compile → commit`) are a single queue entry with
per-stage status, so a failure at `recover` doesn't leave a half-run pipeline in the history with no
explanation of where it stopped.

### 8.6 Configuration UX — the append-only warning

Editing `[observe] paths` or `ignore-patterns` must surface what devlog 43 established: the fix is
two-step, config change **plus** full rebuild, because the append-only ledger never retroactively
drops already-committed data. Narrowing a path in the UI shows an explicit dialog —
"this affects future builds only; already-compiled data for the removed path stays in the ledger" —
with a button that performs the wipe-and-rebuild if that is what the operator actually wants.

A second, subtler one worth surfacing in the ignore-pattern editor: `WalkDir`'s `filter_entry`
matching in this codebase is directory-**name** equality, not a path prefix or a glob
(`observation-sdk/src/lib.rs`, `plugins/file/src/lib.rs`). Adding `fixtures` excludes *every*
directory named `fixtures` anywhere in the tree. The preview-scan endpoint makes that concrete
rather than a footnote.

## 9. Frontend design

### 9.1 Stack

Vite + React 18 + TypeScript; TanStack Query for server state; Zustand for view state (filters,
selection, camera); Tailwind + shadcn/ui; Recharts for statistics; xterm.js for run logs;
**react-force-graph-3d** (three.js) for the graph. API types are generated with
`openapi-typescript` from FastAPI's schema — the browser↔console contract is never written twice.

### 9.2 Choosing the 3D library

| Option | Ceiling | Verdict |
|---|---|---|
| **react-force-graph-3d** | ~5–10k nodes / 20k edges | Orbit, zoom, fly-to-node, custom `THREE.Object3D` per node, neighbour highlighting, all out of the box. **Chosen.** |
| Cosmograph / cosmos.gl | 100k–1M+ | GPU layout, but 2D only — fails the requirement |
| Raw three.js + `InstancedMesh` | 50k+ in 3D | Full control, but picking, labels, and layout are all hand-built |
| Sigma.js | ~100k in 2D | Abandons 3D |

The measured workspace sizes (20 793 entries / 5 533 objects on this repo; 21 992 objects /
5 136 relationships on the estate config) fit the chosen option **given server-side filtering**.
Without it, nothing fits — hence §9.3.

### 9.3 Three levels of detail

**Level 0 — overview (default).** `--level aggregate`. Super-nodes are object kinds
(`File`, `Table`, `Person`, `Crate`, `Technology`, `Symbol`, `TransformNode`, `Custom(...)`) or
path prefixes; edges are weighted group edges. Always under ~100 nodes. Instant.

**Level 1 — expansion.** Clicking a super-node expands it into real objects under a node budget
(default 500); every other group stays collapsed. This is the "separate a part out" interaction.

**Level 2 — neighbourhood.** Clicking an object calls `ekos_neighborhood` at depth 1–3 (slider).
"Isolate" hides everything else. The side panel shows `ekos_state` plus resolved evidence — path,
fragment, confidence, and the analyzer that produced each claim.

**Impact mode.** `ekos_impact` renders a highlighted directed trace over the current graph. This is
the differentiating screen and should get the most design attention — it is the visual form of the
claim that currently has none.

Encoding: colour by kind, size by degree (post-filter, per §4.4), edge width by aggregate weight at
level 0. Later, once `ConflictingEvidence` exists as a diagnostic, a conflict halo on affected
nodes — the graph is the natural place to make conflicting evidence visible.

Filters: object kinds and relationship kinds as toggles. `Custom("CoupledWith")` (co-change) and
`Custom("FeedsInto")` (pipeline step wiring — one real Pentaho workspace has 86 `TransformNode`s per
transformation) are **off by default and shown as off**, following `render_architecture`'s existing
decision to exclude `FeedsInto` from dependency graphs for exactly this reason.

Search: `ekos_search` with `limit` (RFC 0124) feeds a command palette; selecting a hit flies the
camera to the node, highlights its neighbours, and dims everything else.

### 9.4 Layout

Client-side force simulation below ~2 000 nodes. Above that, the console precomputes coordinates
with **graphology + ForceAtlas2** and returns fixed positions, cached per
(workspace, ledger generation, filter set). Fixed coordinates also make the layout stable across
sessions, which matters more than it sounds — a graph that rearranges itself every load cannot be
learned.

## 10. Phasing

| Phase | Scope | Estimate |
|---|---|---|
| **0. Contracts** | This RFC; R1 + R2 implemented and verified against real workspaces; Python MCP client with an integration test against a live `--tcp` server; `web/` skeleton; Compose file | 1–1.5 wk |
| **1. Shell + statistics** | Auth, workspace registry, dashboard: entries/objects/relationships, storage breakdown, objects by kind, growth timeline, `doctor` status | 1 wk |
| **2. Configuration** | `ekos.toml` form + raw editor, validation, preview-scan, append-only warning flow | 1 wk |
| **3. Commands and runs** | Allowlist, job runner, per-workspace mutex, SSE logs into xterm.js, run history, cancellation | 1.5 wk |
| **4. Scheduling** | APScheduler, cron/interval UI, pipeline chains, failure notification | 1 wk |
| **5. Graph v1** | LOD 0 + 1, orbit/zoom, kind filters, search with fly-to, object panel with evidence | 2–2.5 wk |
| **6. Graph v2** | Neighbourhood isolation, impact mode, server-side ForceAtlas2, PNG/glTF export | 1.5 wk |
| **7. Hardening** | Performance passes, theming against the existing house `theme.css`, docs, packaging | 1 wk |

**~10–11 weeks** for the full scope, single developer. A demonstrable slice — phases 0–3 plus 5 —
lands around week 6.

Each phase gets its own implementation RFC authored just-in-time (0128+), consistent with how
RFC 0118 sequenced 0119–0126.

## 11. Non-Goals

- **Multi-tenancy.** One deployment serves one operator or one small trusted team. No org model, no
  per-workspace ACLs beyond the read/write role split.
- **Replacing the CLI.** Every console action maps to a real CLI command; the CLI stays the
  primary, fully-supported interface. The console never grows behaviour the CLI cannot express.
- **Editing compiled knowledge from the browser.** The ledger is append-only. The console can
  trigger recompilation and confirm/reject identity and architecture review candidates — nothing
  else writes.
- **Multi-workspace routing inside one MCP server.** Still one `--workspace` per process
  (RFC 0115's Non-Goal, unchanged). The console runs one `ekos mcp serve --tcp` per registered
  workspace and routes on its own side.
- **HTTP/SSE MCP transport.** Still tracked in TODO.md, still not attempted here.
- **`KnowledgeStore: Send`.** Deliberately untouched — see §2.1.
- **Embedding the demo server (RFC 0045).** Different product surface, fixed pre-baked catalog. Not
  merged, not deleted.

## 12. Open questions

1. **Historical graph export** (`--as-of`). `all_objects_at`/`all_relationships_at` already exist
   (RFC 0096), so this is a small additive option, not a trait change. Worth its own increment, or
   is a `ekos_diff`-based "what changed" view enough?
2. **Aggregate-level caching invalidation.** The store fingerprint (newest mtime under the store
   root — already used by the MCP result cache, `mcp.rs`) is the pragmatic key R3 uses; a true
   ledger generation number = `entry_count()` (on the trait, all backends) if a monotonic key is
   wanted later.
3. **Progress reporting for long builds.** `ekos build` on the estate ran 6m01s and `recover`
   20m50s. Tailing stderr proves liveness but not progress. Is a structured
   `--progress-json` stream worth its own increment, or is stage-level granularity from the chained
   job runner sufficient?
4. **Free-form relationship kinds.** The document-semantics analyzer emits bare prepositions and
   natural-language phrases as relationship kinds — devlog 144 records 95 distinct kinds from one
   Plausible workspace. The filter UI needs a policy for a long tail of one-off kinds; tightening
   the analyzer's vocabulary (tracked as DOC-SEM-1) may be the better fix.

## 13. Verification

- R1 and R2 unit- and integration-tested per §4.9, full workspace gate clean
  (`cargo fmt`, `build --workspace`, `clippy --workspace -D warnings`, `test --workspace`).
- The Python MCP client tested against a **real** `ekos mcp serve --tcp` process over a real socket,
  not a mock — same discipline RFC 0115 applied to its own concurrency test.
- Command allowlist tested adversarially: shell metacharacters in parameters, `..` traversal in
  path parameters, unknown command names, and a write command attempted without the write role.
- The console run end-to-end against this repo's own populated `.ekos/` workspace and against the
  Plausible workspace, with node/edge counts, payload sizes, and frame rates recorded in the
  implementation devlog rather than asserted.

## 14. Files Changed (projected)

| File / area | Change |
|---|---|
| `ekos/docs/rfcs/0127-web-console.md` | This RFC |
| `crates/runtime/src/graph_export.rs` | New — `GraphExportOptions`, `GraphExport`, `export_graph` |
| `crates/runtime/src/lib.rs` | `+pub mod graph_export;` |
| `crates/cli/src/commands/graph.rs` | New — `ekos graph export` |
| `crates/cli/src/commands/mod.rs` | `+pub mod graph;` |
| `crates/cli/src/bin/ekos.rs` | `Commands::Graph { … }`; `--json` on `Status` / `LedgerCommands::Status` |
| `crates/cli/src/commands/ledger.rs` | `--json` output path for `status` (text path unchanged) |
| `crates/cli/src/commands/store.rs` | `store_root`/`store_fingerprint`/`newest_mtime` lifted from `mcp.rs`, `pub(crate)` |
| `crates/cli/src/commands/mcp.rs` | `ekos_graph_export` tool (R3); calls `super::store::` for the fingerprint helpers; optional `--tcp-token` handshake (R4, deferred) |
| `crates/ledger/src/lib.rs` | trait `+evidence_count`; `impl Ledger +evidence_count +format_tag`; `delegate_store!` forwarder |
| `crates/ledger/src/fact_ledger.rs`, `crates/ledger/src/partitioned/{mod,knowledge_store}.rs`, `crates/distributed/src/gateway.rs` | `+evidence_count` (distributed → deferred `Err`, CLI maps to `null`) |
| `web/api/**` | New — FastAPI console |
| `web/ui/**` | New — React console |
| `web/docker-compose.yml` | New |
| `README.md` | Web console section |
| `TODO.md` | Console tracked; MCP auth item narrowed by R4 |
