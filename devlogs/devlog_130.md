# Devlog 130 — RFC 0113 B4: distributed read path (query workers + gateway)

**Date:** 2026-08-30
**PRs:** `cd68967` (B4a), plus the B4b commit landed with this devlog
**Branch:** main (direct)

---

## Summary

Devlog 129 shipped RFC 0113 B1–B3 (the `SegmentBackend` seam, `ObjectStoreBackend`, and the
coordinator + Service A compile workers). B4 adds the **read** half of Distributed mode: a new
crate `ekos-distributed` with **Service B** (`QueryWorker` — serves `KnowledgeStore` reads for a
partition it has pulled into a local cache) and **Service C** (`DistributedLedger` — a
`KnowledgeStore` that fans reads across the workers the coordinator knows about and merges them).
Pointing a workspace at a cluster is now just `[storage.distributed]` in `ekos.toml`; every
existing command, the MCP server, and `docs generate` work unchanged against it because they only
ever see the `KnowledgeStore` trait.

Not done: B5 (distributed search ranking), and a batch of gateway v1 follow-ons (persistent
connection pool, parallel fan-out, coordinator-index pruning, a command to register an existing
Local partitioned workspace with a coordinator).

---

## B4a — Service B query worker (`cd68967`)

### What was built

| Component | Role |
|---|---|
| `PartitionCache` | `materialize(partition_id, location)` → a local dir. `PartitionLocation::Local` is opened in place; `PartitionLocation::ObjectStore { url, prefix }` is pulled — every object under the prefix — into `<cache_root>/<partition-id>/`, preserving the backend-relative layout so `FactLedger::open_read_only` opens the cache dir exactly as if it were the original workspace partition. Immutable segments already present (right size) are never re-downloaded. |
| `QueryWorker` | On the first request for a partition: ask the coordinator (`catalog(Some(id))`) where it lives → `PartitionCache::materialize` → `FactLedger::open_read_only` → cache the `Arc<FactLedger>`. Every ledger call runs on `spawn_blocking` (materialise + mmap + an object-store backend's own runtime must be off the async executor). |
| `WorkerRequest` / `WorkerResponse` | The RPC surface — one variant per `KnowledgeStore` read, each carrying `partition: String`. `WorkerResponse` is **adjacently tagged** (`tag = "status", content = "data"`) — internally-tagged serde can't serialize the newtype/sequence variants (`Objects(Vec<…>)`). |
| `QueryWorkerClient` | Held-open TCP connection, typed helpers for the full read surface. |
| `ekos query-worker serve` | Thin CLI wrapper. |
| `ObjectStoreBackend::from_url` | `s3://` / `az://` / `gs://` / `file://` via `object_store::parse_url`; added `object_store/fs` to the `object-store` feature. |

### Keeping `object_store` out of the stock build

`ekos-distributed` has its own `object-store` feature (default off). `crates/cli` gets a
`distributed` feature that turns it on. A stock `cargo build --workspace` compiles no
`object_store`; `cargo test --workspace` does, via `ekos-distributed`'s self-dev-dependency
`ekos-distributed = { path = ".", features = ["object-store"] }`. A **co-located Local cluster
works without the feature** — only object-storage partition locations need it.

### Test

`crates/distributed/tests/query_worker.rs`: a coordinator + a query worker in front of a real
`PartitionedLedger` workspace; every read over RPC (`get_object`, `object_history`,
`relationships_for` against the `rel:` partition, `object_count`, `object_at`, `find_objects`,
error on an unknown partition) equals a direct `PartitionedLedger` read. Plus a sync test that
materialises a partition through a `file://` object-store backend and opens the cached copy.

---

## B4b — Service C: the `DistributedLedger` gateway (with this devlog)

### What was built

`DistributedLedger` (`impl KnowledgeStore`): every read

1. asks the coordinator for the catalog,
2. selects the partitions of the right **class** — classified by id prefix: `rel:*` →
   relationship, `events/*` → event, `evidence/*` → evidence, everything else → object,
3. fans a worker RPC to each (round-robin worker pick),
4. merges:
   - current-state single (`get_object`): newest-partition-first, first `Some` wins;
   - current-state bulk (`all_objects`): dedup by id, newer partition overwrites;
   - history (`object_history`): concat, oldest partition first (`PartitionKey` order == time
     order, RFC 0111 §1);
   - `diff`: `PartitionedLedger::diff`'s own merge (extend `added`, union `touched`, sum
     `unchanged`);
   - counts: sum.

`append_*` and `vacuum_into` return `LedgerError::ReadOnly` — writes in Distributed mode are
Service A (`ekos compile-worker`), and the gateway matches the Runtime-is-read-only invariant.

`crates/cli/src/commands/store.rs`: a `[storage.distributed]` branch at the top of `open_store`,
`open_store_read_only`, and `store_display`, ahead of every local backend. `compiler-core` gets
`StorageDistributedConfig { coordinator: Option<String>, query_workers: Vec<String> }` (strings
only — `compiler-core` doesn't depend on `ekos-distributed`).

### The runtime-drop bug

First cut gave `DistributedLedger` an owned `tokio::runtime::Runtime` for the "no ambient
runtime" case. `ekos query find` against an unreachable cluster then panicked:

```
Cannot drop a runtime in a context where blocking is not allowed. This happens when a
runtime is dropped from within an asynchronous context.
```

`open_store` is called from inside `#[tokio::main]`; the `Box<dyn KnowledgeStore>` (and its
nested `Runtime`) is dropped there. Fix: **never own a `Runtime`.** The sync↔async bridge is
`block_on_sync`:

```rust
fn block_on_sync<F: Future>(fut: F) -> F::Output {
    match Handle::try_current() {
        Ok(h) => tokio::task::block_in_place(|| h.block_on(fut)),
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all().build().unwrap().block_on(fut),
    }
}
```

Reuse the ambient multi-threaded runtime via `block_in_place` when there is one (the CLI,
`ekos mcp serve`); otherwise spin a transient current-thread runtime for that one call — built
and dropped entirely within a sync context, so its `Drop` is fine.

### Test

`crates/distributed/tests/gateway.rs`: `DistributedLedger` over a **2-worker** cluster in front
of a real `PartitionedLedger`; `get_object` / `all_objects` / `object_count` / `object_history` /
`relationships_for` / `object_at` all match the in-process `PartitionedLedger`; the newest
version wins (`orders` v2's `owner` property); a cross-partition object (`main.rs`, `File`
partition) resolves; `append_object` is rejected; an unknown id is a clean `None`. The gateway's
sync `KnowledgeStore` calls run from a plain `std::thread` so `block_on_sync` takes the transient
path.

---

## Knowledge Captured

- **serde internally-tagged enums can't hold sequence/newtype variants.**
  `#[serde(tag = "status")] enum WorkerResponse { Objects(Vec<KirObject>), … }` compiles but
  *serializing* `Objects(_)` fails at runtime ("cannot serialize tagged newtype variant
  containing a sequence"), which on the wire looks like the server closing the connection with no
  response. Use **adjacently** tagged (`tag = "…", content = "…"`) or externally tagged.
- **Never store a `tokio::runtime::Runtime` in a type that a `#[tokio::main]` program drops.**
  Dropping a runtime inside an async context panics. A sync API over async that must work from
  both contexts: `Handle::try_current()` → `block_in_place` + `handle.block_on` if inside a
  (multi-thread) runtime, else a **transient** current-thread runtime per call. `block_in_place`
  itself panics on a current-thread runtime — fine for `#[tokio::main]` (multi-thread by
  default), so tests that need the sync path call from a plain `std::thread`, not
  `#[tokio::test]`.
- **`object_store` 0.14 `parse_url` needs the `fs` feature for `file://`.** With
  `default-features = false` you get "feature for Local not enabled" from `parse_url`, not a
  compile error. Add `object_store/fs` (cheap, no cloud SDK).
- **Relationships route to their own `rel:<Kind>` partitions**, disjoint from object partitions
  (RFC 0111 amendment). A query worker serving `Table/2026-08` returns nothing for
  `relationships_for` — that entity's relationships live in `rel:DependsOn/2026-08`. The gateway
  has to know the partition taxonomy; a single worker doesn't.
- **`find_objects` on a freshly-written, unsealed partition returns nothing after
  `open_read_only`.** The tantivy index isn't committed/visible to a second reader until a
  segment seals (the accepted ≤8 MB window). Not a B4 regression — a pre-existing
  `FactLedger`/`PartitionedLedger` property; the B4a test asserts RPC == a direct read-only open,
  not non-emptiness.
- **The shell's cwd in this workspace is `ekos/`**, not the repo root — `tests/integration` and
  `benchmark` are at `../tests/integration` / `../benchmark`; `cd ekos` fails.

---

## Files Changed

| File | Change summary |
|---|---|
| `crates/distributed/` (new) | `ekos-distributed`: `cache.rs` (`PartitionCache`), `protocol.rs` (`WorkerRequest`/`WorkerResponse`/`DiffWire`), `worker.rs` (`QueryWorker` + `serve`), `worker_client.rs` (`QueryWorkerClient`), `gateway.rs` (`DistributedLedger`); `tests/query_worker.rs`, `tests/gateway.rs` |
| `crates/segment-backend/src/object_store_backend.rs` | `ObjectStoreBackend::from_url`; `object-store` feature gains `url` + `object_store/fs` |
| `crates/ledger/src/partitioned/mod.rs` | `PartitionedLedger::partition_root` |
| `crates/compiler-core/src/config.rs` | `StorageDistributedConfig` (`[storage.distributed]`) |
| `crates/cli/src/commands/store.rs` | `uses_distributed` / `build_distributed`; `[storage.distributed]` branch in `open_store`/`open_store_read_only`/`store_display` |
| `crates/cli/src/commands/cluster.rs`, `bin/ekos.rs`, `Cargo.toml` | `ekos query-worker serve`; `ekos-distributed` dep; `distributed` feature |
| `ekos/Cargo.toml` | `crates/distributed` workspace member + dep |
| `ekos/docs/rfcs/0113-…md`, `0111-…md` | B4a/B4b acceptance + follow-ons |
| `TODO.md`, `README.md` | Phase B progress through B4 |
