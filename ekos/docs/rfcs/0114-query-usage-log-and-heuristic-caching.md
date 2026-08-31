# RFC 0114 — Query Usage Log + Heuristic Result Caching (Storage Plan Phase 5 groundwork)

**Status:** Accepted (per user direction — building incrementally, same as RFC 0111/0113)
**Author:** EKOS team
**Created:** 2026-08-31
**Implemented:** 2026-08-31

---

## Motivation

RFC 0080's storage plan names Phase 5 (materialized views alongside the EAV fact engine) as its
least-scoped item, explicitly requiring "a pass over real EKL/MCP query logs to find what's
actually worth materializing" before any design work starts. Checking the actual codebase found
that prerequisite doesn't exist yet: the only real, persisted query log anywhere is RFC 0056's
ClickHouse audit trail (`crates/clickhouse-query/src/audit.rs`), scoped to that one live-external-
system tool. `ekos_ekl`'s interpreter and the other 13 read-only MCP tools (`ekos_search`,
`ekos_neighborhood`, `ekos_state`, `ekos_dependents`, `ekos_impact`, `ekos_diff`, `ekos_status`,
`ekos_transformation_explain`/`diff`, `ekos_architecture_evaluate`/`drift`/`diff`) have zero
persisted call history. Phase 5 as originally scoped ("analyze existing logs") has nothing to
analyze.

Per explicit user direction, this RFC covers two things landing together: (1) add the missing
usage log so a real Phase 5 scoping pass becomes possible once data accumulates, and (2) don't
wait for that data to provide *any* benefit — a lightweight, pre-execution heuristic classifies
each incoming call and opportunistically caches results for the ones it flags as expensive,
independent of any historical analysis. The heuristic doesn't have to be perfectly accurate for
this to be sound: the log always records each call's *real measured* duration and cache-hit status
regardless of what the heuristic guessed, so nothing is lost even when the classifier misses.
Phase 5's actual materialized-view design still waits for that real data; this RFC is the
groundwork, not that design.

## Why not extend RFC 0056's ledger-based audit pattern directly

The obvious first guess — append an Evidence/Event pair to the ledger per call, like
`record_query_event` does for ClickHouse — doesn't fit here for two concrete reasons found by
reading `crates/cli/src/commands/mcp.rs` before assuming:

1. **Semantic mismatch.** RFC 0056's ledger write records that a *live external system* was
   queried — the SQL text and result hash are themselves evidence of an observation. An internal
   read of the already-ledgered knowledge model creates no new evidence; ledgering "someone called
   `ekos_search`" would be usage telemetry wearing evidence's clothes, and the ledger is
   append-only forever (no delete/tombstone anywhere in the codebase) — telemetry doesn't belong in
   a store with no retention story.
2. **Lock contention.** The 13 read tools go through `StoreCache`, a **read-only**-opened
   `KnowledgeStore` cached across the server's session specifically because a *writable* `FactLedger`
   open holds tantivy's exclusive `IndexWriter` lock for its whole lifetime — RFC 0097 fixed a real
   regression from exactly this (see `StoreCache`'s own doc comment, `mcp.rs:27-47`). Appending an
   Event/Evidence per call would need a writable store, meaning either breaking `StoreCache`'s
   read-only invariant (reintroducing the lock-contention bug RFC 0097 fixed) or opening a fresh
   writable store per call (reintroducing the pre-RFC-0097 latency regression, 19s → 71ms per the
   status-command fix in the same devlog). `identity_review`/`architecture_review` get away with a
   fresh writable open because they're rare, deliberate, human-paced writes — not a fit for
   potentially-per-turn AI agent reads.

Conclusion: usage telemetry gets its own append-only **local log file**, outside the ledger
entirely — no lock contention with a concurrent `ekos build`/`commit`, no permanent ledger bloat,
and no evidence-semantics mismatch. `ekos_clickhouse_query` keeps its existing RFC 0056 ledger
audit unchanged (still the right tool for that one case) and *additionally* gets one usage-log
entry like every other tool, so Phase 5's eventual scoping pass sees one consolidated log.

## Design

### Usage log (`crates/cli/src/commands/query_log.rs`)

One JSON line per tool call, appended to `<workspace>/.ekos/query-log.jsonl`:

```json
{"ts":"2026-08-31T12:00:00Z","tool":"ekos_impact","cost_class":"expensive","reason":"max_hops=5 (default)","cache_hit":false,"result_count":42,"duration_ms":18}
```

`record(workspace, entry)` opens the file with `OpenOptions::create(true).append(true)` and writes
one line with a single `write_all` call — no locking beyond what the filesystem gives a single
`write_all` for a short line, matching how JSONL logs are conventionally written elsewhere. A
concurrent MCP server process and `ekos build` writing to this file at the same moment could in
principle interleave two lines' bytes on some filesystems; accepted for v1 (this is telemetry, not
a correctness-bearing store — a corrupted line is a bad log line, not a bad answer), same
"accept the small edge case, document it" posture RFC 0113 v1 already applies elsewhere (e.g. the
≤8 MB unsealed-segment loss window).

### Heuristic cost classifier

`classify_ekl(&EklAst) -> (CostClass, &'static str)` and `classify_tool(name, &Value) ->
(CostClass, String)` are pure, static-threshold functions — no learning, no history, evaluated
before the call runs:

| Tool | `Expensive` when |
|---|---|
| `ekos_ekl` | No predicates and no `FROM` scope (full entity scan), or `LIMIT` absent/> 500 |
| `ekos_neighborhood` | `depth` (or the default, 1) ≥ 3 |
| `ekos_impact` | `max_hops` (or the default, 5) ≥ 4 |
| `ekos_diff` / `ekos_architecture_diff` | window > 7 days, or `to` absent (open-ended to "now") |
| `ekos_architecture_evaluate` / `ekos_architecture_drift` | always (whole-workspace scan, no args to vary — the ideal case for caching: identical repeated calls) |
| everything else (`ekos_search`, `ekos_state`, `ekos_dependents`, `ekos_status`, `ekos_transformation_explain`/`diff`, `ekos_clickhouse_query`) | never |

`ekos_transformation_explain`/`diff` are deliberately left `Cheap` despite taking a `max_hops`
parameter: the parameter is a safety cap, not a proxy for actual chain length (a `max_hops: 50`
call against a real 3-node chain does 3 nodes of work, not 50) — a static pre-execution heuristic
has no way to see the real chain length, so guessing from the parameter would misclassify more
often than not. This is exactly why the classifier only *gates opportunistic caching* rather than
being the source of truth Phase 5 will use — the log's real `duration_ms` is that source of truth,
recorded for this tool the same as every other regardless of its `Cheap` classification.

`ekos_clickhouse_query` is classified `Expensive`-shaped (network-bound) but explicitly **excluded
from caching**: the workspace's on-disk fingerprint (the cache's invalidation signal) says nothing
about whether the live ClickHouse database has changed since the last identical question, so a
cached answer could silently go stale. It still gets a usage-log entry.

### Result cache

Added to `StoreCache` (`mcp.rs`) — process-local, one HashMap keyed by `(tool, canonicalized
args JSON string)`, storing the tool's JSON result. Reuses the exact fingerprint `StoreCache`
already computes to decide whether to reopen the store: the cache is cleared whenever the store
is reopened (i.e. whenever the on-disk fingerprint changes), so a cached answer can never outlive
the workspace state it was computed against. Only consulted/populated for tool calls the
heuristic classifies `Expensive` (and never for `ekos_clickhouse_query`, per above) — a `Cheap`
call always executes fresh, both because it's already fast and because caching every call for no
benefit would just grow the map. `tools_call` wraps the existing `call_tool` dispatch: classify →
check cache → execute on miss → populate cache on an expensive miss → always write one usage-log
entry (`cache_hit` reflects whether this call's *own* json result was served from the cache).

Key-canonicalization caveat: `serde_json::Value`'s map ordering depends on the `preserve_order`
feature; two logically-identical argument objects with differently-ordered keys are accepted as a
possible cache-miss (never a wrong answer) — real MCP clients send arguments in a fixed order per
the tool schema, so this is expected to be rare in practice and isn't worth a canonicalization
pass for v1.

### The CLI `ekos ekl` command

Also logs via the same `query_log::record` + `classify_ekl` (no result cache — a one-shot CLI
invocation has no server-session state to cache across).

## Non-Goals

- **The actual materialized-view design** (what to materialize, storage format, invalidation) —
  still Phase 5 proper, still waits for real accumulated log data. This RFC only makes that data
  start existing.
- **A corpus-global or cross-session cache** — the result cache is process-local and per-server-
  session, gone when the MCP server restarts. A persistent cache is out of scope until Phase 5's
  real design.
- **Perfect cost classification** — see above; the classifier is a caching gate, not a scoring
  system. Phase 5's later analysis works from measured `duration_ms`, not `cost_class`.
- **Log rotation/retention** — `query-log.jsonl` grows unbounded for now; a real deployment running
  this for a while (the actual Phase 5 prerequisite) will need rotation, deferred until this proves
  out as worth keeping.

## Testing

- `query_log::record` writes one well-formed JSON line; a second call appends rather than
  overwrites.
- `classify_ekl`/`classify_tool` unit tests per row of the table above, including default-value
  cases (`depth`/`max_hops` absent → the *default* is what's classified, not treated as absent).
- `mcp.rs`: a repeated identical `ekos_architecture_evaluate` call is served from cache the second
  time (asserted via a call counter on the underlying evaluation, not just wall-clock time); a
  store write between the two calls invalidates the cache (same fingerprint mechanism `StoreCache`
  already tests); `ekos_clickhouse_query` is never cached even when called identically twice.
