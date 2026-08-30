# Devlog 131 — RFC 0113 B5: distributed search (Phase B feature-complete)

**Date:** 2026-08-30
**PRs:** the B5 commit landed with this devlog
**Branch:** main (direct)

---

## Summary

B4 (devlog 130) gave the `DistributedLedger` gateway a `find_objects` that just concatenated each
shard's hits. B5 replaces that with a real **per-shard BM25 top-`k` merge** — the distributed
search path RFC 0111 §7 describes. With it, **RFC 0113 Phase B (B1–B5) is feature-complete at v1
scope**: `SegmentBackend` seam → `ObjectStoreBackend` → coordinator + Service A → query workers +
`DistributedLedger` gateway → distributed search. Remaining work is the tracked v1 → v1.1 polish
(connection pooling, parallel fan-out, coordinator-index pruning, a Local→Distributed partition
registration command) plus binding a Service A lease to a real shard-scoped `build`/`commit`.

---

## What was built

### Scores out of the ledger

`SearchIndex::query` already computed `(score, addr)` from tantivy and threw the score away.
Split into:

- `SearchIndex::query_scored(query, limit) -> Vec<(Uuid, String, f32)>` — the real body, score
  kept; `query` now delegates and drops the score.
- `FactLedger::find_objects_scored(query, limit) -> Vec<(KirId, String, f32)>` — same
  commit-on-read + bounded-limit path; `find_objects` delegates with `limit = 50`.

Behaviour of the existing `find_objects` (both `SearchIndex` and `FactLedger`) is byte-identical —
it's a pure refactor with a scored variant added beside it. Not added to the `KnowledgeStore`
trait (keeps the trait and every impl / `delegate_store!` untouched).

### The merge

`WorkerRequest::FindObjectsScored { partition, query, k }` → `WorkerResponse::ScoredHits`.

`DistributedLedger::search(query, k)`:

1. resolve the **object** partitions from the coordinator catalog (`rel:` / `events/` /
   `evidence/` excluded);
2. ask a query worker for each partition's local BM25 **top-`k`**;
3. dedup by id, keeping the highest shard-local score;
4. sort by score desc (name tie-break), truncate to `k`.

`find_objects` (the `KnowledgeStore` method) now calls `search(query, 50)` and strips scores — so
every existing caller (Runtime, MCP `ekos_search`, `docs generate`, `ekos query find`)
transparently gets a properly rank-merged cross-shard result instead of a naive concat.

### The caveat, made explicit

BM25 scores are **shard-local** — computed from each partition's own term statistics (document
frequency / IDF). A rare term in a small partition outscores the same term in a large partition
where it's common. This is RFC 0111 §7's accepted **query-then-fetch approximation** (what
Elasticsearch does by default); a global term-statistics pre-pass is explicitly out of scope for
v1. `tests/search.rs` asserts the caveat rather than papering over it: with `orders` /
`orders_archive` in the 3-doc Table partition and `orders.md` in the 2-doc File partition, the
File hit ranks **first** (higher local IDF), and the test asserts set-completeness + score
ordering + `k`-bounding, **not** that the exact-name match wins.

---

## Knowledge Captured

- **A read-only `FactLedger` can't commit the search index**, so a query worker that opens a
  partition via `open_read_only` only sees tantivy segments the *writer* already committed. In a
  test, force it: call `find_objects` once on the writable `PartitionedLedger` before the workers
  materialise — `FactLedger::find_objects` commits-on-read, persisting the tantivy segments each
  worker then opens. Without that, distributed search over a freshly-built workspace returns
  nothing (the accepted ≤8 MB unsealed window, not a bug).
- **`KirId` is `Hash + Eq` but not `Ord`** — dedup/collect with `HashSet`/`HashMap`, not
  `BTreeSet`.
- **Shard-local BM25 is not a bug to fix at v1.** Merging per-shard top-`k` by raw score is the
  industry-standard distributed-search approximation; the "correct" alternative (broadcast term
  stats, then a second scoring round) is a real cost RFC 0111 §7 declined to pay before there's
  evidence it matters.

---

## Files Changed

| File | Change summary |
|---|---|
| `crates/ledger/src/search.rs` | `SearchIndex::query_scored` (score kept); `query` delegates |
| `crates/ledger/src/fact_ledger.rs` | `FactLedger::find_objects_scored(query, limit)`; `find_objects` delegates |
| `crates/distributed/src/protocol.rs` | `FindObjectsScored { k }` request, `ScoredHits` response |
| `crates/distributed/src/worker.rs` | serve `FindObjectsScored` via `find_objects_scored` |
| `crates/distributed/src/worker_client.rs` | `find_objects_scored` helper |
| `crates/distributed/src/gateway.rs` | `DistributedLedger::search(query, k)` merge; `find_objects` rides on it |
| `crates/distributed/tests/search.rs` | cross-partition top-`k` merge + shard-local-IDF caveat |
| `ekos/docs/rfcs/0113-…md`, `0111-…md` | B5 acceptance; Phase B marked feature-complete at v1 |
| `TODO.md`, `README.md` | Phase B B5 / feature-complete |
