# Devlog 137 — RFC 0114: query usage log + heuristic result caching (storage Phase 5 groundwork)

**Date:** 2026-08-31
**PRs:** the commit landed with this devlog
**Branch:** main (direct)

---

## Summary

Storage plan Phase 5 (materialized views alongside the EAV fact engine) has always named its own
prerequisite: "a pass over real EKL/MCP query logs to find what's actually worth materializing."
Checking the actual codebase before starting that pass found the prerequisite doesn't exist — the
only real query log anywhere is RFC 0056's ClickHouse audit trail, scoped to that one
live-external-system tool. `ekos_ekl` and the other 13 read-only MCP tools have zero persisted call
history. Per explicit user direction, this session built the missing usage log **and**, so real
value doesn't wait for months of accumulated data, a lightweight heuristic that opportunistically
caches expensive-looking queries starting from the very first call. RFC 0114 covers both; Phase 5's
actual materialized-view design still waits for real data — this is the groundwork that lets that
data start existing.

---

## PR — RFC 0114: query usage log + heuristic result caching

### Problem / motivation

Scoping Phase 5 properly required first checking what already exists, not assuming the RFC 0080
prerequisite was satisfied. It wasn't. A dedicated research pass confirmed: no logging in
`crates/ekl`'s interpreter, no audit trail for any of the 13 read-only MCP tools beyond stderr
operational logging, no persisted history of `ekos ask` questions, nothing on disk in `.ekos/` or
`archive/demo/` resembling a query log. RFC 0080 and TODO.md both already named this gap in
writing — it just hadn't been closed.

### What was built

| Component | What it does |
|---|---|
| `crates/cli/src/commands/query_log.rs` | `record(ekos_dir, entry)` appends one JSON line to `<workspace>/.ekos/query-log.jsonl`; `classify_ekl(&EklAst)` and `classify_tool(name, &args)` are pure, static-threshold heuristics returning `(CostClass, reason)`. |
| `StoreCache` (`mcp.rs`) | Gained a `result_cache: HashMap<(tool, args-json), Value>`, a `refresh()` method (the fingerprint check factored out of `get()`), and `cached_result`/`cache_result` accessors. |
| `tools_call` (`mcp.rs`) | Classifies every read-tool call before running it, checks/populates the result cache for `Expensive` calls (except `ekos_clickhouse_query`), and always writes one usage-log entry with the call's *real measured* duration. |
| `ekos ekl` (`ekl.rs`) | Logs via the same `classify_ekl`/`record` — no cache, since a one-shot CLI process has no session to cache across. |

### Why not just extend RFC 0056's ledger-based audit pattern

The obvious first move — append an Evidence/Event pair per call, like `record_query_event` does
for ClickHouse — was the initial guess (including from a research sub-agent) and turned out wrong
on inspection of the actual code, for two concrete reasons:

1. **Semantic mismatch.** RFC 0056's ledger write records that a *live external system* was
   queried; the SQL and result hash are themselves evidence. An internal read of the
   already-ledgered knowledge model creates no new evidence — ledgering "someone called
   `ekos_search`" is usage telemetry wearing evidence's clothes, in a store that's append-only
   forever with no delete/tombstone anywhere.
2. **Lock contention.** The 13 read tools go through `StoreCache`, deliberately **read-only**
   (RFC 0097) specifically because a writable `FactLedger` open holds tantivy's exclusive
   `IndexWriter` lock for its whole lifetime — RFC 0097 fixed a real regression from exactly this.
   Appending an Event/Evidence per call needs a writable store: either break `StoreCache`'s
   read-only invariant (reintroducing RFC 0097's bug) or open a fresh writable store per call
   (reintroducing the pre-RFC-0097 latency regression the same devlog also fixed, 19s → 71ms for
   `ekos_status`). `identity_review`/`architecture_review` get away with a fresh writable open
   because they're rare, human-paced writes, not a fit for potentially-per-turn AI reads.

Usage telemetry instead gets its own append-only local file, entirely outside the ledger — no lock
contention, no permanent bloat, no evidence-semantics mismatch. `ekos_clickhouse_query` keeps its
existing RFC 0056 ledger audit unchanged and *also* gets one usage-log entry, so Phase 5's eventual
scoping pass sees one consolidated log across every tool.

### Implementation details worth remembering

- **The heuristic is a caching gate, not a scoring system.** `classify_tool`/`classify_ekl` decide
  whether to *attempt* caching before a call runs; they don't have to be accurate for the logging
  half to be sound, because every call's real measured `duration_ms` is recorded regardless of the
  guess. `ekos_transformation_explain`/`diff` are deliberately always `Cheap` despite taking a
  `max_hops` cap, specifically because that parameter bounds a real chain walk that usually
  terminates on its own long before the cap — guessing expense from the parameter value would
  misclassify more often than not. The measured duration, not the guessed class, is what a real
  Phase 5 analysis will use.
- **A real bug found while wiring cache invalidation, not before shipping.** The first version
  cleared `result_cache` only inside `StoreCache::get()`, called only on an actual store-open
  attempt. A call that kept *hitting* the result cache never reached `get()`, so it would never
  notice the store had changed underneath it — a cache entry could stay poisoned forever once
  populated, contradicting the whole "invalidated by the fingerprint" design. Fixed by extracting
  `refresh()` (the fingerprint check + reopen + cache-clear) and calling it unconditionally at the
  top of `tools_call`'s cacheable path, before consulting the cache — not only on a miss.
- **Proving a cache is real needs a deliberately wrong test, not just a correct one.** A test that
  registers a value and checks it comes back correct would also pass if caching were silently
  disabled end-to-end. `expensive_tool_call_is_served_from_a_poisoned_cache_when_present`
  deliberately inserts an impossible value (`crates_total: 999, poisoned: true`) directly into the
  cache and asserts it comes back — the only way that assertion holds is if the cache path is
  actually taken. This is the same technique `gateway_uses_the_entity_index_to_prune_when_present`
  (devlog 136) used for RFC 0113's pruning — worth keeping as the general pattern whenever a cache
  or an index-based shortcut needs proving, not just correctness-checking.
- **`ekos_clickhouse_query` is excluded from caching, not just classified differently.** Its
  `Expensive`-shaped cost (network I/O) would otherwise make it a caching candidate, but the
  workspace's on-disk fingerprint — the cache's only invalidation signal — says nothing about
  whether the *live* ClickHouse database changed since an identical question was last asked. A
  dedicated `is_cacheable(name, cost_class)` predicate makes this exclusion an explicit, testable
  rule rather than an inline condition easy to lose in a future edit.
- **Live-verified through the real binaries**, not just unit tests: a scratch workspace built with
  `ekos build`, then `ekos ekl "FIND Object" --json` produced a real log line
  (`cost_class: expensive, reason: "no predicates or FROM scope"`); a real `ekos mcp serve` session
  with two identical `ekos_architecture_evaluate` calls showed the second served from cache
  (`cache_hit: true, duration_ms: 0`) against the first's real measured 3ms.

### Decisions (alternatives considered, why this choice)

- **A dedicated local file vs. reusing the ledger** — covered above; the deciding factors were
  evidence semantics and lock contention, not just "simpler."
- **`serde` as a new direct dependency of `ekos-cli`** — the crate only had `serde_json`/`serde_yaml`
  before; `LogEntry`/`CostClass` need `#[derive(Serialize)]` for a clean, typed log-line shape
  rather than hand-building a `serde_json::json!` object per entry. A one-line `Cargo.toml` addition
  (`serde.workspace = true`, the workspace already declares it with the `derive` feature) versus
  duplicating field-by-field JSON construction at every log call site.
- **One log file, size-unbounded for now** — rotation/retention is explicitly a non-goal in the
  RFC. This log's whole purpose is to *start existing* so Phase 5 has something to analyze;
  designing retention for a file that doesn't have any real accumulated data yet would be
  speculative engineering ahead of the actual need.

---

## Knowledge Captured

- Before extending an existing audit/logging pattern to a new call site, check whether that call
  site's write-access story actually matches the original's — RFC 0056's pattern assumed a
  writable store was cheap to obtain per call, true for ClickHouse's own network-bound tool but
  false for the 13 tools RFC 0097 deliberately made read-only-cached for latency/lock reasons.
- A cache invalidated by "check on open" only actually invalidates on a *miss* — a hit-only path
  never revisits the invalidation check at all. Any cache gated by "reopen if X changed" needs that
  check to run unconditionally on every lookup, not just on the code path that already needed to
  reopen for other reasons.
- To prove a cache or a pruning shortcut is genuinely wired in (not silently bypassed by a fallback
  path), poison it with a value that could only come back if the shortcut were actually taken.
  Checking for the *correct* answer alone can't distinguish "shortcut worked" from "shortcut never
  ran, full computation happened to get it right anyway."

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/query_log.rs` | New: `CostClass`, `LogEntry`, `record`, `classify_ekl`, `classify_tool` + unit tests |
| `ekos/crates/cli/src/commands/mcp.rs` | `StoreCache` result cache + `refresh()`; `tools_call` classifies/caches/logs every read-tool call; new tests |
| `ekos/crates/cli/src/commands/ekl.rs` | Logs via `classify_ekl`/`record` after a successful query |
| `ekos/crates/cli/src/commands/mod.rs` | Registers `query_log` module |
| `ekos/crates/cli/Cargo.toml` | Added `serde.workspace = true` |
| `ekos/docs/rfcs/0114-query-usage-log-and-heuristic-caching.md` | New RFC |
| `TODO.md`, `README.md` | Phase 5 tracking + MCP section updated |
