# Devlog 128 — RFC 0111 Phase A: partitioned/tiered storage, end to end

**Date:** 2026-08-29
**PRs:** `e5e0fcc`, `d4d91eb`, plus one commit landed with this devlog
**Branch:** main (direct)

---

## Summary

The knowledge ledger has been single-process, single-machine since day one — one `SegmentStore`,
one set of order-preserving indexes, one tantivy index, one writer, for the entire workspace. RFC
0080 named "Phase 6 — horizontal distribution" as the last storage item and left it blocked on RFC
0034. This session merged RFC 0034 (single-machine partitioning/tiering) and RFC 0110 (multi-machine
distribution) into one conformed design, **RFC 0111**, and then built its **Phase A (Local mode)**
end to end, incrementally, against that RFC directly (0111 doubles as the Phase A implementation
RFC per explicit user direction; a separate one is still expected for Phase B).

The result is `PartitionedLedger` — a drop-in `KnowledgeStore` that routes reads/writes across many
`FactLedger` partitions keyed by a configurable dimension + time bucket, with a persisted catalog
and an AEVT-style run-file index so a reopened ledger resolves anything with zero partition scans.
It is opt-in via `[storage.partition]` and served by `open_store` for a fresh workspace; existing
SQLite/fact workspaces are never touched. RFC 0112 (lock-free snapshot reads for `FactLedger`) was
also filed as a Draft during the merge but not implemented.

---

## PR `e5e0fcc` — the merge + the first vertical slice

### What was built

| Component | What it does |
|---|---|
| RFC 0111 | Merges RFC 0034 + RFC 0110 into one design (one partition model, two deployment modes: Local / Distributed). Both source RFCs marked Withdrawn, kept as historical record. |
| RFC 0112 | Draft — closes the `FactLedger::open_read_only` cross-process visibility gap RFC 0104 documented (a WAL-style consistent snapshot). Not implemented. |
| `crates/ledger/src/partitioned.rs` | `PartitionedLedger` — `EntityKind` routing, `TimeBucket::{Daily,Weekly,Monthly}` (labels chosen so lexical order == chronological), `entity_id → Set<PartitionKey>` fan-out for history (§2), pruned `objects_in_kind` (§1), each partition an `Arc<FactLedger>` with its own lock (§1: "N partitions admit N concurrent writers"). |
| `compiler-core` `[storage.partition]` | Config parsing (`dimension` / `time-bucket` / `time-bucket-overrides`), data-only — `cli` does the string→enum translation. |

---

## PR `d4d91eb` — persistence, more dimensions, tiering, relationships

### Persisted catalog (RFC 0111 §5)

`<catalog_root>/catalog.json` — every partition ever created, its on-disk root, and its `Tier`.
Atomic (temp + rename), rewritten only when a partition is registered or changes tier. Also records
the dimension + time bucket and **refuses a reopen that changes either** (`DimensionMismatch`) —
the on-disk partition names encode both, so a config flip would orphan data.

### Persisted AEVT-style run-file index

`<catalog_root>/index/run-*.jsonl` — append-only `{k, id, p}` lines. `new` folds every run into
in-memory `id → partitions` maps and marks them authoritative, so after a reopen point/history
reads do **zero partition scans**. A line is appended only the first time an id lands in a
partition. `merge_runs`-style compaction at `COMPACT_AT` (16) runs. `rebuild_entity_index()` is the
`ekos ledger repair`-style full re-derive. A self-healing catalog scan runs only for an id absent
from the index (crash recovery).

### SourceScope / Composite routing

All three `PartitionDimension`s now route. `SourceScope`/`Composite` take a caller-supplied
`with_source_resolver(|&KirObject| -> Option<String>)` closure — `KirObject` has no source field
yet, so the closure is the seam. A `None` under a source dimension is a hard `UnresolvedSource`
error, never a silent misroute. `Composite` value is `"<source>\u{1f}<kind>"`.

### Cold tiering (RFC 0111 §3, policy layer)

`mark_cold_before(cutoff)` demotes past-bucket partitions to `Tier::Cold`, evicts their open
handles, and flags them relocate-eligible. Any read promotes one back to hot. The §3 search-index
drop + zstd recompression are deferred (they need `FactLedger` support).

### Relationships (RFC 0111 amendment 2026-08-29 §1/§2)

Relationships route by `"rel:"+RelationshipKind` + time bucket — **independent of the object
dimension**. They have no clean source and their `from`/`to` may be in different partitions;
kind is the query axis for impact analysis; the `"rel:"` prefix keeps relationship partitions
disjoint from object partitions in the shared catalog. The unified index gained `rel` (relationship
id → partition) and `endpoint` (an entry per `from` and `to` → partition) kinds; `relationships_for(X)`
prunes to X's relationship partitions via `endpoint`, never a full graph scan.

---

## Commit landed with this devlog — full `KnowledgeStore`, `open_store` wiring, submodule split

### The rest of the trait surface (amendment §3)

- **Events / evidence** — own `"events"` / `"evidence"` partitions, `evt` / `evid` index kinds.
  `rebuild_entity_index` can't re-derive these (`FactLedger` has no `all_events`/`all_evidence`) —
  they recover only via the per-read self-healing scan.
- **Point-in-time** — `object_at` (fan out newest→oldest, first `Some` wins — respects
  observation-time cuts), `all_objects_at`, `relationships_at` (pruned), `all_relationships_at`.
- **`find_objects`** — fans out **hot** object partitions' tantivy indexes, merges + dedups by id.
  Per-partition BM25 (RFC §7 query-then-fetch approximation); cold partitions skipped.
- **`diff`** — merges per-partition `LedgerDiff`s (`touched`/`unchanged` merge cleanly; `added`
  entry-ids are per-partition-local, concatenated).
- **`vacuum_into`** — self-contained copy: rewritten `catalog.json` (roots → `dest`), the `index/`,
  each partition's `FactLedger` under `dest/p/<sanitized-key>/`.
- **`impl KnowledgeStore for PartitionedLedger`** — via `From<PartitionError> for LedgerError`
  (a wrapped `Ledger` error is unwrapped; anything else → `Corrupt`). Tested through a
  `Box<dyn KnowledgeStore>`.

### `open_store` wiring

`open_store` / `open_store_read_only` (`crates/cli/src/commands/store.rs`) now build a
`PartitionedLedger` when `[storage.partition]` is enabled **and** the workspace is genuinely fresh
(no SQLite `ledger.db`, no `facts/manifest.json`) — or already has `partitioned/catalog.json`.
Mirrors the fact-engine default-switch rule exactly: an existing workspace is never implicitly
switched. `PartitionedLedger::read_only()` opens each partition via `FactLedger::open_read_only`
(RFC 0097) and never mutates the catalog or index. Only `entity-kind` wires from config so far —
`source-scope`/`composite` return a clear error (no `KirObject` source field yet).

### Submodule split

`partitioned.rs` (2576 lines) → `partitioned/{mod.rs, types.rs, knowledge_store.rs, tests.rs}`.
Pure refactor, git-tracked as a rename; `lib.rs`'s `pub mod partitioned` + re-exports unchanged.

---

## Knowledge Captured

- **`FactLedger` internal tx time is `Utc::now()`, not `KirObject.created_at`.** The partition
  *routing* uses `created_at` (so you can backfill a historical bucket), but `object_at(id, t)`
  and `diff` cut on the ledger's own *observation* timestamp. A test that wrote objects with
  historical `created_at` and expected `object_at` to see them at those times was wrong — point-in-time
  tests must use real wall-clock ordering with a captured `mid = Utc::now()` between writes, the way
  `fact_ledger.rs`'s own tests do. `FactLedger::append_version` (pub(crate)) is the only dated-append
  path and it's migration-only.

- **`resolve_sites`'s catalog scan must be lazy.** First cut passed the scan set (`catalog_snapshot(None)`,
  which *opens every partition*) as an argument, so the fast path — "id is in the loaded index, return
  its set" — still eagerly opened everything. Pass a closure (`impl FnOnce`), call it only on the slow
  path. Caught by a reopen test asserting `partition_keys().len() == 1` after one point read.

- **Rust child modules can access private struct fields of ancestor modules.** This makes splitting a
  big inherent `impl` across submodule files feasible — `impl PartitionedLedger` in
  `partitioned/knowledge_store.rs` still touches `self.catalog` etc. with `use super::*`. Privacy is
  "visible to the defining module and its descendants", fields included.

- **`#[cfg(test)] mod tests;` + `#![cfg(test)]` in the file = "duplicated attribute" clippy error.**
  Pick one: the `#[cfg(test)]` on the `mod` declaration is idiomatic; the extracted file then needs
  no inner attribute.

- **`RelationshipKind` and `EventKind` have no `Display`** (only `ObjectKind` does, plus
  `RelationshipKind` via a manual impl — `EventKind` genuinely has none). Relationships partition by
  `rel.kind.to_string()` (the manual `Display`); events can't, so they all share one `"events"`
  partition per time bucket — fine, `KnowledgeStore` has no `events_for`/`all_events`.

- **CI runs `cargo clippy --workspace -- -D warnings` without `--all-targets`.** Test-code lints
  (`useless_vec`, `clone`→`from_ref`) in `ekos-docs-gen`/`ekos-compiler-core` test modules fail an
  `--all-targets` run but are pre-existing and don't block CI. Don't chase them when they surface.

- **Merging `LedgerDiff` across partitions is lossy for `added`.** Each partition's `LedgerEntryId`s
  number from 1, so they're not globally unique — concatenated as opaque markers. `touched` (logical
  KirId strings) and `unchanged` (a count) merge cleanly, and those are what `ekos diff` consumers
  actually use.

---

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0111-partitioned-tiered-and-distributed-storage.md` | The merged design + the 2026-08-29 relationships/KnowledgeStore amendment + Phase A checklist tracking |
| `ekos/docs/rfcs/0112-lock-free-snapshot-reads.md` | New — Draft |
| `ekos/docs/rfcs/0034-…`, `0080-…`, `0110-…` | Withdrawn / superseded / roadmap pointers |
| `ekos/crates/ledger/src/partitioned/mod.rs` | `PartitionedLedger` — routing, catalog, run-file index, tiering, all `KnowledgeStore` methods, `read_only()` |
| `ekos/crates/ledger/src/partitioned/types.rs` | `PartitionError`, `PartitionDimension`, `TimeBucket`, `PartitionKey`, `Tier`, `PartitionEntry`, `PartitionCatalog` |
| `ekos/crates/ledger/src/partitioned/knowledge_store.rs` | `impl KnowledgeStore` + `From<PartitionError> for LedgerError` |
| `ekos/crates/ledger/src/partitioned/tests.rs` | 22 tests |
| `ekos/crates/ledger/src/lib.rs` | `pub mod partitioned` + re-exports |
| `ekos/crates/compiler-core/src/config.rs` | `[storage.partition]` config |
| `ekos/crates/cli/src/commands/store.rs` | `uses_partitioned` / `build_partitioned` / wiring into `open_store*` / `store_display` |
| `TODO.md` | Phase 6 / Phase A progress |
| `benchmark/Cargo.lock` | `fs4` sync (RFC 0104, was stale) |
