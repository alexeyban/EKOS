# Devlog 135 — RFC 0113: publishing the partition search index (closing Phase B v1)

**Date:** 2026-08-31
**PRs:** the commit landed with this devlog
**Branch:** main (direct)

---

## Summary

Devlog 134 made a partition self-describing in object storage for everything except tantivy's
`search/` dir — a reader still needed `search/` on its local root, which meant a fresh reader of
an object-storage partition (no prior local mirror) couldn't serve `find_objects` for it. This
closes that gap: `search/` now routes through the same `SegmentBackend` seam as
`manifest.json`/`dict.bin`, publishable by a writer post-commit and fetchable by a reader that
finds none locally. This was the last open item tracked against RFC 0113 Phase B's v1 scope
(everything else remaining is v1 → v1.1 gateway polish — connection pool, parallel fan-out, index
pruning — none of it blocking).

---

## PR — publish/fetch the search index through `SegmentBackend`

### Problem / motivation

`manifest.json` and `dict.bin` publish through `SegmentBackend::publish` (devlog 134), but
tantivy's `search/` directory is a multi-file structure `SegmentStore` never managed — it was
always assumed local. That's fine for `ekos-distributed`'s `QueryWorker`, whose `PartitionCache`
already downloads *every* object under a partition's backend prefix (search included, if
present) before opening it as a plain local `FactLedger`. But `PartitionedLedger` also opens a
backend-attached partition directly, in-process, via `FactLedger::open_read_only_with_backend` —
that path only ever pulled `manifest.json`/`dict.bin`/segments, never `search/`, so a fresh root
would open with an **empty** search index and silently return zero hits — not because the
partition holds nothing, but because it read the wrong copy.

### What was built

| Component | What it does |
|---|---|
| `SegmentStore::publish_aux(rel)` | Pushes every flat file under `<root>/<rel>/` to the backend at the same `<rel>/…` keys. Generic — not search-specific — skips lock files, no-ops if the dir is absent. |
| `SegmentStore::fetch_aux(rel)` | The inverse: lists `<rel>/` on the backend, downloads each object into `<root>/<rel>/` (temp-file + rename). Returns `false` (no-op) if the backend has nothing under that prefix. |
| `FactLedger::open_read_only_with_backend` | If a backend is wired and no local `search/` exists, calls `fetch_aux("search")` first. If the backend has nothing either, creates an empty `search/` dir so `SearchIndex::open_read_only` still succeeds — the partition **degrades to zero search hits**, every other read (`get_object`, `relationships_for`, history, …) is unaffected. |
| `FactLedger::sync_search_to_backend()` | For a writer: commits the in-memory search index, then `publish_aux("search")`. Meant to be called post-commit/pipeline, not per-append. |
| `PartitionedLedger::publish_search_indexes()` | Iterates every catalogued partition and calls `sync_search_to_backend()` on each. Returns the count. |
| `ekos compile-worker run` (Service A) | Calls `publish_search_indexes()` right after the real pipeline finishes and before `collect_partitions`/registering partitions with the coordinator. |

### Implementation details worth remembering

- `publish_aux`/`fetch_aux` are deliberately generic over *any* flat directory, not hardcoded to
  `search/` — the docstrings call out tantivy's `search/` as the motivating case, but nothing in
  the implementation assumes it. Any future writer-local, reader-needed directory that isn't
  natively `SegmentStore`-managed can reuse the same seam.
- The degrade-to-empty-index behavior (rather than erroring `open_read_only_with_backend` when no
  `search/` exists anywhere) matches the RFC 0111 §7 stance already taken for query-then-fetch
  search: a missing/stale index is an accepted approximation, not a hard failure. A caller that
  needs to know whether search is "real" for a partition has no signal for that today — not needed
  yet, flagged here in case a future caller cares.
- `sync_search_to_backend` calls `inner.search.commit(last_tx)` before publishing — the in-memory
  tantivy writer buffers segments until committed; skipping this would publish a stale (or empty)
  on-disk index even though `find_objects` against the *live* writer would return correct results.
- This is orthogonal to `ekos-distributed`'s `PartitionCache`, which already round-trips whatever
  the backend holds under a partition's full prefix (search included) with no changes needed on
  that side — the gap was specifically `PartitionedLedger`'s direct backend-attached open path.

### Decisions (alternatives considered, why this choice)

- **Generic `publish_aux`/`fetch_aux` vs. a `search`-specific method pair.** Chose generic: the
  logic (list/read/write a flat directory in lockstep with backend keys) has nothing search-specific
  in it, and `SegmentStore` already has the backend handle — adding a second near-identical
  search-only method would just be the same code with narrower naming.
- **Empty index on total miss vs. propagating an error.** An error would make every read of an
  object-storage partition whose writer hasn't run `sync_search_to_backend()` yet (e.g. right after
  `PartitionedLedger::with_segment_backend` first points at pre-existing segment data with no
  search sync) fail outright, including reads that have nothing to do with search. Degrading only
  `find_objects` for that partition keeps the invariant that non-search reads never depend on
  search-index availability.

---

## Knowledge Captured

- RFC 0113 Phase B's last structurally-open item (`search/` publishing) is now resolved — the RFC
  header, its Open Questions checklist, and TODO.md's tracking paragraph are updated in the same
  commit as the code, matching how devlogs 132–134 tracked B1–B5 and manifest publishing.
- When adding a new `SegmentBackend`-routed directory, check both consumers: `ekos-distributed`'s
  `PartitionCache` (which already does a full-prefix download and needs nothing extra) and
  `PartitionedLedger`'s direct `open_*_with_backend` path (which only pulls what `FactLedger`
  explicitly asks for) — they don't automatically stay in sync just because one grew a new file
  type.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/ledger/src/segment/mod.rs` | `SegmentStore::publish_aux`/`fetch_aux` — generic flat-dir push/pull through `SegmentBackend` |
| `ekos/crates/ledger/src/fact_ledger.rs` | `open_read_only_with_backend` fetches `search/` on miss; new `sync_search_to_backend()`; test `search_index_travels_through_the_backend` |
| `ekos/crates/ledger/src/partitioned/mod.rs` | `PartitionedLedger::publish_search_indexes()` |
| `ekos/crates/cli/src/commands/cluster.rs` | `compile_worker_run` publishes search indexes after each compile |
| `ekos/docs/rfcs/0113-storage-phase-b-distributed-mode-implementation.md` | Header + Open Questions + Files Changed updated to reflect search publishing landed |
| `TODO.md` | RFC 0113 tracking paragraph updated — Phase B now fully closed at v1 scope |
