# Devlog 133 — RFC 0113: PartitionedLedger writes through SegmentBackend

**Date:** 2026-08-30
**PRs:** the commit landed with this devlog
**Branch:** main (direct)

---

## Summary

Until now `PartitionedLedger` opened every partition via `FactLedger::open(local_path)` — no
`SegmentBackend` awareness, so a distributed cluster had to keep *all* partition data on a shared
filesystem. This wires the seam through: each partition's **sealed segments** (its bulk — 8 MB
immutable objects) can now be published to / fetched from a `SegmentBackend`, e.g. an
`ObjectStoreBackend` for S3/Azure. The partition's small local working state
(active/unsealed segment, `HEAD`, `manifest.json`, `dict.bin`, `search/`) still lives on its
local root.

Config: `[storage.partition] segment-backend-url = "s3://bucket/prefix"` (or `az://`, `gs://`,
`file://`). `open_store` builds one `ObjectStoreBackend` per partition, keyed
`<url>/<partition-id>`, cache = the partition's local root. Behind the cli `distributed` feature,
so a stock build still never compiles `object_store`.

Remaining: the **manifest** still lives only on the local root. Until it's published through the
backend too, a distributed cluster needs the per-partition metadata dirs on shared storage (or
belonging to the single writer). The bytes — the expensive part — are in object storage now.

---

## What was built

| Layer | Change |
|---|---|
| `FactLedger` | `open_with_backend(root, backend)`, `open_read_only_with_backend(root, backend)`, `open_with_backend_and_seal_threshold(root, backend, n)`. `open_with_seal_threshold` / `open_read_only` refactored to share `open_writable` / `open_ro` bodies taking `Option<Arc<dyn SegmentBackend>>`. Sealed segments → backend; everything else → `root`, unchanged. |
| `PartitionedLedger` | New `BackendResolver` = `Fn(&PartitionKey, &Path) -> Option<Arc<dyn SegmentBackend>>` field + `with_segment_backend(resolver)` builder. `partition()` opens via `FactLedger::open{,_read_only}_with_backend` when the resolver returns `Some`. Default resolver returns `None` for every partition (today's behaviour). |
| `compiler-core` | `StoragePartitionConfig::segment_backend_url: Option<String>`. |
| `crates/cli` `store.rs` | `with_segment_backend_url` — validates the URL up front, then a per-partition `ObjectStoreBackend::from_url("<url>/<pid>", local_root)`, memoised in a `Mutex<HashMap>`. Feature-gated: `#[cfg(not(feature = "distributed"))]` bails with a clear "rebuild with `--features distributed`". |
| `crates/cli` `cluster.rs` | `collect_partitions` returns `PartitionLocation::ObjectStore { url: "<base>/<pid>" }` when `segment-backend-url` is set (so a query worker pulls the sealed segments straight from object storage), else the local root. |

---

## Knowledge Captured

- **You cannot force a real 8 MB segment seal cheaply in a test.** `open_store` /
  `build_partitioned` hardcode `SEGMENT_SEAL_BYTES`; batch bodies are zstd-compressed, so `"x"
  .repeat(9 MB)` shrinks to a few KB and never seals, and 12 000 distinct objects (~42 s to
  write) still don't reach 8 MB compressed. The seal-and-wipe proof has to live at the
  `FactLedger` level, where `open_with_backend_and_seal_threshold(root, backend, 1)` seals every
  batch. The `open_store` test just proves the config → `ObjectStoreBackend::from_url` chain
  builds and doesn't disturb normal reads, plus that a bogus URL is a clear error.
- **`object_store::parse_url` errors are opaque strings, not typed.** `http://…` without the
  `http` feature → `"Generic parse_url error: feature for Http not enabled"`. Validate the URL
  once at `open_store` time so the failure names `segment-backend-url`, not some deep seal-path
  panic later.
- **The active (unsealed) segment genuinely can't be on an append-less object store**, and mmap
  writes need a real file, so `HEAD` + `search/` can't either. `manifest.json` / `dict.bin`
  *could* be published (they're small, rewritten atomically) — that's the next increment toward a
  fully self-describing object-storage partition.
- **One `ObjectStoreBackend` per partition, memoised.** Each carries its own current-thread tokio
  runtime; building one is cheap (no `block_on`), but you don't want a fresh one per read.
  Keyed by partition id in a `Mutex<HashMap<String, Arc<dyn SegmentBackend>>>` inside the
  resolver closure.

---

## Files Changed

| File | Change summary |
|---|---|
| `crates/ledger/src/fact_ledger.rs` | `open_with_backend` / `open_read_only_with_backend` / `..._and_seal_threshold`; `open_writable` / `open_ro` shared bodies; `sealed_segments_are_served_from_the_backend_not_local_disk` test |
| `crates/ledger/src/partitioned/mod.rs` | `BackendResolver`, `with_segment_backend`, backend-aware `partition()` open; module-doc update |
| `crates/ledger/src/partitioned/tests.rs` | `with_segment_backend_routes_each_partition_through_its_backend` |
| `crates/compiler-core/src/config.rs` | `StoragePartitionConfig::segment_backend_url` |
| `crates/cli/src/commands/store.rs` | `with_segment_backend_url` (feature-gated); `segment_backend_url_wires_partitions_without_disturbing_reads` test |
| `crates/cli/src/commands/cluster.rs` | `collect_partitions` → `PartitionLocation::ObjectStore` when configured |
| `crates/cli/Cargo.toml` | `ekos-segment-backend` dep; `distributed` feature adds `ekos-segment-backend/object-store` |
| `ekos/docs/rfcs/0113-…md`, `0111-…md` | sealed-segments-through-backend acceptance; manifest-through-backend as the remaining piece |
| `TODO.md`, `README.md` | per-partition object storage |
