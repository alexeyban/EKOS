# Devlog 136 — RFC 0113 gateway v1.1: connection pool, parallel fan-out, real id-index pruning

**Date:** 2026-08-31
**PRs:** the commit landed with this devlog
**Branch:** main (direct)

---

## Summary

Devlog 135 closed RFC 0113 Phase B's last structurally-open item (publishing the search index).
What remained was explicitly tracked as non-blocking "v1 → v1.1 polish": `DistributedLedger`
connected fresh per call, fanned every multi-partition read out sequentially, and never used the
coordinator's `entity_id → partitions` index to prune id-scoped reads. This closes all three. Along
the way it found and fixed two real, pre-existing bugs: the compile-worker was populating that
index with a placeholder that had zero pruning value, and an integration test's watermark
assertion was checking the wrong key entirely — both masked by an `||` in one test assertion.
**RFC 0113 Phase B is now fully closed at v1 scope**, with no tracked follow-ons remaining.

---

## PR — pooled connections, concurrent fan-out, real pruning

### Problem / motivation

The gateway's own doc comment was explicit about the gap: "v1 keeps no persistent connection
pool... fans out sequentially... does not use the coordinator's `id → partitions` index to prune."
None of these were wrong so much as deferred — correct, just not minimal, exactly as documented.
Picking this up meant three genuinely separate pieces of work landing together because they touch
the same code paths in `DistributedLedger`.

### What was built

| Component | What it does |
|---|---|
| `ConnSlot<C>` | A tiny reconnect-on-demand cache — one cached `Arc<C>` behind a `tokio::sync::Mutex`, cleared and re-established on an I/O error. One per coordinator address, one per worker address. |
| `call_coordinator`/`call_worker` | Run a closure against the pooled connection; on `ClusterError::Io`/`Closed` (or the `DistributedError` equivalents), clear the slot, reconnect once, retry once. Every existing coordinator/worker call site now routes through these instead of connecting fresh. |
| `fan_out` | Dispatches a call to a list of partition ids **concurrently** (`futures::future::try_join_all`), round-robining across workers, returning results in the same order as the input list — so the existing sequential merge logic (`HashMap` insert in a specific order, `Vec` concat) needed no changes beyond swapping the source of the per-partition results. |
| `first_present` | Same idea for id-scoped "first match wins" lookups (`get_object`, `object_at`, …): fan out concurrently, then scan the *original* candidate-partition order (not completion order) for the first `Some` — preserves the newest-partition-wins semantics while parallelizing the network round trips. |
| `candidate_partitions` | Queries the coordinator's `entity_id → partitions` index for a given id; if it has entries for that id (filtered to the right class), uses only those, sorted newest-bucket-first; if empty, falls back to a full class scan — the exact v1 behavior, now just the fallback path. |
| `PartitionedLedger::partition_entity_ids(key)` | New: every object/relationship id a catalogued partition currently holds. Events/evidence aren't included — `KnowledgeStore` has no `all_events`/`all_evidence` to enumerate their ids from, an honest v1 scope boundary, not an oversight. |
| `ekos compile-worker run` | `finalize_partitions` (renamed from `collect_partitions`, now doing three things in one `PartitionedLedger` open) collects `(id, partition)` pairs from `partition_entity_ids` across every partition it wrote, groups by id, and calls `record_entity_partitions` once per distinct id. |

### Implementation details worth remembering

- **The generic-closure-over-`Fn` gotcha**: `fan_out`/`first_present` build a `Vec` of futures by
  mapping over partition ids, and each future needs to call the caller-supplied `call: F` closure.
  Writing `async move { ... call(...) ... }` inside a `.map()` closure tries to **move** `call` into
  every generated future — fine for a `Copy` closure, a compile error for anything else, since you
  can't move the same value into N different futures. Fix: `let call = &call;` before the `.map()`,
  so each future captures a `Copy` reference instead of the closure itself. This is a general
  pattern for fan-out-over-a-closure code, not specific to this crate.
- **Order matters more than it looks.** `try_join_all`/`join_all` preserve the input order in their
  output `Vec` regardless of completion order — this is *why* the existing merge logic
  (newest-partition-wins via `HashMap` insert order, oldest-first history concat, "first candidate"
  id lookups) needed zero changes: build the candidate list in the required order first, fan out
  concurrently, then fold over the *results* in that same order as if the loop had stayed
  sequential. Concurrency here is purely a latency win, not a semantics change.
- **Pruning only helps queries with an id to prune by.** `relationships_for(id)` looks up
  relationships by *endpoint*, not by the relationship's own id — the entity index (keyed by an
  entity's own id) has nothing to offer it, so it still fans to every rel partition, just
  concurrently now. This is inherent to what the query is asking, not a gap in the index.
- **Found while wiring the index population — the shard-name placeholder bug.** The pre-existing
  `compile_worker_run` called `client_w.record_entity_partitions(&shard_w, &ids)` — `shard_w` is
  the shard/lease name (e.g. `"main"`), `ids` is every partition the compile wrote. That's not an
  entity index at all; it's `shard_name → [every partition]`, which no query path ever looked up by
  shard name, so it was silent dead weight. Real per-id population needed enumerating each
  partition's actual objects/relationships (`partition_entity_ids`), which the trait didn't expose
  a cheap way to do — `all_objects()`/`all_relationships()` (full objects, not just ids) is what
  exists, and it's fine here since this only runs once per compile over what was just written, not
  a hot path.
- **A dedicated test had to *prove* pruning is real, not just correctness-preserving.** A test that
  registers the entity index correctly and checks the result stays correct would also pass if
  pruning were silently disabled and everything fell back to a full scan — it wouldn't catch a
  regression to "pruning never actually happens." `gateway_uses_the_entity_index_to_prune_when_present`
  instead deliberately mis-registers `orders`' id against the wrong partition (one that doesn't
  hold it) and asserts `get_object` returns `None` — the only way that assertion holds is if the
  gateway actually trusts the index over a full scan.
- **Fixing the placeholder surfaced a second, unrelated bug.** `tests/integration/tests/integration.rs`'s
  `compile_worker_runs_the_real_pipeline_under_a_lease` asserted `watermark(catalog[0].id) > 0 ||
  partitions_for_entity("main").len() == catalog.len()`. The first branch was dead code from day
  one: watermarks are tracked per lease/shard name (`"main"`), not per physical partition id
  (`"Table/2026-08"`) — two different key namespaces by design (a shard is a scheduling unit, not
  necessarily one physical partition). `watermark(catalog[0].id)` was always `0`; the test only
  ever passed via the second branch, which was itself testing the placeholder bug above. Fixed both:
  the assertion now checks `watermark("main")` (the real key), and a new assertion checks that a
  real compiled object's own id is indexed against its real partition.

### Decisions (alternatives considered, why this choice)

- **A hand-rolled `ConnSlot` vs. a real pooling crate (`deadpool`, `bb8`).** A cluster-internal TCP
  connection is cheap to establish (localhost/LAN, no TLS handshake in v1) and each `CoordinatorClient`/
  `QueryWorkerClient` already serializes calls internally via two `Mutex`es — there's no benefit to
  a multi-connection-per-address pool with checkout/return semantics here, just one cached
  connection reconnected on failure. Pulling in a pooling crate for that would be the kind of
  dependency this codebase's "pure functions, minimal deps" bias argues against — true parallelism
  across workers still comes from round-robining which worker's single pooled connection a given
  partition's request goes to, not from multiple connections per worker.
- **Retry-once vs. a real health check (ping-before-use).** A ping adds a round trip to every call
  for a failure mode (a dead pooled connection) that's rare and cheap to detect reactively — the
  first real call fails, gets retried once against a fresh connection. Matches the "correct, not
  maximally defensive" pattern the rest of RFC 0113 v1 already uses (e.g. the 8 MB unsealed-segment
  loss window accepted rather than engineered away pre-evidence).
- **One `RecordEntityPartitions` RPC per distinct id vs. a new bulk RPC.** A batched request would
  cut round trips for a large compile, but adds protocol surface for what's already a loopback/LAN
  call after a compile that just took much longer than the sum of these RPCs. Not pursued —
  flagged here as the natural next optimization if a real large-workspace compile shows it matters.

---

## Knowledge Captured

- When fanning a generic closure `F: Fn(...)` out across multiple futures built in a `.map()` or
  loop, take `&F` before entering the loop — passing the owned closure into more than one
  `async move` block is a "consumed multiple times" compile error, not a runtime bug, but it only
  shows up once you actually try to build more than one future from the same closure.
- `try_join_all`/`join_all` return results in input order, not completion order — this is the
  mechanism that lets concurrent fan-out be a pure drop-in replacement for a sequential loop when
  the loop's merge logic depends on order (which several methods here did, silently, via `HashMap`
  insert order for "newest wins").
- Two different coordinator concepts share a key namespace by name but not by meaning: a "shard" (a
  work-scheduling unit chosen by whoever calls `ekos compile-worker run --shard <name>`) and a
  "partition" (a physical storage unit like `Table/2026-08`). Watermarks are tracked per shard;
  the catalog and the (now-real) entity index are tracked per partition/id. Conflating them in a
  test assertion produced a passing-for-the-wrong-reason test that survived until this pass.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/distributed/src/gateway.rs` | `ConnSlot` pooling, `fan_out`/`first_present` concurrent dispatch, `candidate_partitions` id-index pruning with class-scan fallback |
| `ekos/crates/distributed/Cargo.toml` | `futures.workspace = true` |
| `ekos/crates/distributed/tests/gateway.rs` | New test `gateway_uses_the_entity_index_to_prune_when_present` |
| `ekos/crates/ledger/src/partitioned/mod.rs` | `PartitionedLedger::partition_entity_ids(key)` |
| `ekos/crates/ledger/src/partitioned/tests.rs` | New test `partition_entity_ids_lists_exactly_that_partitions_objects_and_relationships` |
| `ekos/crates/cli/src/commands/cluster.rs` | `finalize_partitions` (renamed, extended) populates the real entity index; removed the shard-name placeholder |
| `tests/integration/tests/integration.rs` | Fixed the watermark-by-wrong-key assertion; added a real entity-index assertion |
| `ekos/docs/rfcs/0113-storage-phase-b-distributed-mode-implementation.md` | Header + Open Questions + Files Changed updated — Phase B fully closed |
| `TODO.md`, `README.md` | RFC 0113 tracking updated to reflect gateway v1.1 landing |
