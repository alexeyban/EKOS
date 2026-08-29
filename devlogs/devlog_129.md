# Devlog 129 — RFC 0113: Storage Phase B (Distributed mode) B1–B3

**Date:** 2026-08-29
**PRs:** `aea491d` (RFC), `83f6df3` (B1), `80b6fea` (B2), plus the B3 commit landed with this devlog
**Branch:** main (direct)

---

## Summary

Devlog 128 shipped RFC 0111 **Phase A** (Local mode) — `PartitionedLedger`, single process. This
session started **Phase B** (Distributed mode): object storage as the one durable copy, plus
independently scalable compile / query / gateway services. Phase B got its own dated
implementation RFC, **RFC 0113**, which sequences RFC 0111 §4/§6/§7 into five gated sub-phases
B1–B5 and pins the interface-level choices Phase B's design left at altitude. B1, B2, and B3
landed, incrementally, against RFC 0113 while it is still Draft (same working style as Phase A).

- **B1** — `SegmentBackend`: the trait behind which `SegmentStore` publishes/fetches *sealed*
  objects. `LocalFsBackend` is the default and a proven zero-behaviour-change refactor.
- **B2** — `ObjectStoreBackend` on the `object_store` crate (S3 / Azure / in-memory), behind an
  `object-store` feature a Local build never compiles.
- **B3** — the coordinator (`ekos-cluster`): partition catalog, write leases with fencing tokens,
  per-partition tx watermarks, entity→partitions index; newline-delimited JSON-RPC over TCP; plus
  `CompileWorker` (Service A's transport + lifecycle half) and a multi-service harness.

Not done: B4 (Service B/C + `DistributedLedger`), B5 (distributed search), and binding a Service A
lease to a real shard-scoped `build → commit` run (also B4).

---

## PR `aea491d` — RFC 0113

The implementation RFC RFC 0111's own Acceptance Criteria call for before any Phase B code. It
carries RFC 0111's already-cleared Architecture Review forward, then sequences the work:

| Sub-phase | Scope |
|---|---|
| B1 | `SegmentBackend` seam + `LocalFsBackend` |
| B2 | `ObjectStoreBackend` (`object_store` crate) |
| B3 | Coordinator + Service A single-writer |
| B4 | Service B query workers + Service C `DistributedLedger` gateway |
| B5 | Distributed search fan-out + per-partition BM25 merge |

Each sub-phase is gated by its own review; B(n+1) doesn't start until B(n)'s acceptance passes.

---

## PR `83f6df3` — B1: the `SegmentBackend` seam

### What was built

`SegmentBackend` (`Send + Sync`, sealed-immutable objects only — one publish, many reads, never
read-modify-write):

```rust
fn publish_sealed(&self, key: &str, staged: &Path) -> Result<(), BackendError>;
fn fetch(&self, key: &str) -> Result<PathBuf, BackendError>;   // a readable LOCAL path — callers mmap it
fn list(&self, prefix: &str) -> Result<Vec<String>, BackendError>;
fn exists(&self, key: &str) -> Result<bool, BackendError>;
fn delete(&self, key: &str) -> Result<(), BackendError>;       // compaction/vacuum only
```

`fetch` returns a `PathBuf` (not `Vec<u8>`) on purpose — callers `mmap` the result
(`MappedSegment`), so returning a path keeps B1 a true zero-behaviour-change refactor. A remote
backend fetches into a bounded local cache first and returns the cache path.

### What changed in `SegmentStore`

Exactly four call sites — the ones that touch a *sealed* segment by path: `seal_active` (→
`publish_sealed`), and `batches_after` / `batch_headers` / `verify_sealed_report` (→ `fetch`
before `MappedSegment::open`). Everything else stays local and unchanged: the active unsealed
segment (object stores have no append), `HEAD`, `manifest.json`, `dict.bin`, and tantivy's own
`search/` I/O. `SegmentStore` gained an `Arc<dyn SegmentBackend>` field + `open_with_backend`;
`open` / `open_with_seal_threshold` default it to `LocalFsBackend::new(root)`, so `FactLedger` and
every existing caller are untouched.

### Acceptance

All prior `ekos-ledger` tests green unchanged with `LocalFsBackend` wired in as the default, plus
`sealed_io_routes_through_the_segment_backend` (a `CountingBackend` proves the routing actually
goes through the trait).

---

## PR `80b6fea` — B2: `ObjectStoreBackend`

### What was built

- Extracted `crates/segment-backend` (`ekos-segment-backend`): `SegmentBackend`, `BackendError`,
  `LocalFsBackend`, `MemBackend` (in-memory, no external deps — the "publish → cache → mmap"
  fixture the B3/B4 harness builds on). `get(key)` and `get_range(key, Range<u64>)` were added to
  the trait for B4's remote frame-level reads.
- `ObjectStoreBackend` on `object_store` 0.14, behind the crate's `object-store` feature
  (`aws` / `azure` layer the cloud SDKs on top). `ekos-ledger` depends on `ekos-segment-backend`
  **with no features**, so `cargo build --workspace` never compiles `object_store`; the
  `object-store` feature is pulled only through an `ekos-ledger` **dev-dep**, so
  `cargo test --workspace` does.

### The sync/async bridge (resolved open question)

A dedicated **current-thread `tokio::runtime::Runtime` per `ObjectStoreBackend`**, `block_on`-ing
each call. Safe from a `spawn_blocking` thread and from a plain sync test; must not be called from
*within* another runtime's async context. `object_store` 0.14 moved `put`/`get`/`get_range`/`head`
/`delete` off `dyn ObjectStore` onto the `ObjectStoreExt` blanket trait (RPITIT) — the backend
`use`s it; `get_range` takes `Range<u64>`.

### Acceptance

`ObjectStoreBackend` passes the shared `SegmentBackend` contract test against
`object_store::memory::InMemory`; `segment_store_round_trips_on_object_store_backend` does a full
`SegmentStore` write → seal → **wipe the local cache** → reopen → read with data living only in
the object store.

---

## B3 — Coordinator + Service A (landed with this devlog)

### What was built — `crates/cluster` (`ekos-cluster`)

| Component | Role |
|---|---|
| `Coordinator` | In-memory catalog + `LeaseTable` + watermarks + entity→partitions index; `open(path)` loads/persists one JSON file (atomic temp+rename), `ephemeral()` doesn't. `handle(Request) -> Response` is the single mutation path, so persistence is in one place. |
| `serve(Arc<Mutex<Coordinator>>, TcpListener)` | Newline-delimited JSON-RPC accept loop; one framed request/response stream per connection, all against the one `Mutex` (every `handle` is short). |
| `CoordinatorClient` | One held-open TCP connection, `call()` serialised by two `Mutex`es so concurrent callers can't interleave frames; typed helpers (`lease_acquire`, `manifest_commit`, …). |
| `LeaseTable` | The fencing-token core: `acquire` (or take over an *expired* lease), `check` (every mutating call fences on `l.token > token`), `renew`, `release`. Per-partition monotonic `u64`, survives lease expiry. Unit-tested independently. |
| `CompileWorker` / `LeaseGuard` | Service A's transport + lifecycle half: `run_shard(partition, |guard| async { … })` acquires the lease, spawns a heartbeat task (`lease_renew` at ~TTL/3), runs the closure, aborts the heartbeat, releases. `LeaseGuard` is **owned** (not `<'a>`) so the closure's future can hold it across `.await` freely; `guard.commit(watermark)` maps a coordinator rejection to `WorkerError::LostLease`. |
| `crates/cli/src/commands/cluster.rs` | `ekos coordinator serve` / `ekos coordinator status` / `ekos compile-worker run` — thin wrappers. |

### RPC surface (as implemented)

`CatalogRegister`, `CatalogGet{prefix}`, `LeaseAcquire`, `LeaseRenew`, `LeaseRelease`,
`ManifestCommit{watermark}`, `RecordEntityPartitions`, `PartitionsForEntity`, `Watermark`.
`ManifestCommit` carries the new watermark (generation number) directly, not a manifest blob — the
coordinator only ever needs the number; the manifest itself lives with the partition.

### Decisions (deviations from RFC 0113's design-altitude sketch, now recorded in the RFC)

- **Transport: newline-delimited JSON-RPC over TCP, not gRPC/tonic.** The exact `ekos mcp serve`
  pattern (RFC 0013). No protobuf toolchain, no tonic dependency tree. The coordinator's RPCs are
  all small request/response — no segment bytes ever cross it (those go object-store↔worker
  directly) — so gRPC's streaming edge doesn't apply. tonic stays on the table for B4's Service-B
  segment transfers if a real need appears.
- **Metadata store: a single JSON file, not sled/SQLite.** The whole persisted state is a few KB
  of `serde_json`. Leases are **not** persisted — they're TTL-bounded, so a restart correctly
  invalidates every outstanding one.
- **Crate: new `crates/cluster`, not `crates/cli` subcommands.** `PartitionId` is an opaque
  `String` there (`"<dimension_value>/<time_bucket>"`), so `ekos-cluster` needs no `ekos-ledger`
  dependency.
- **Mutual TLS: deferred.** v1 assumes a trusted cluster network / localhost. Wrapping the
  `TcpStream` in `tokio-rustls` changes no protocol code.

### Acceptance — `crates/cluster/tests/harness.rs`

Real coordinator over TCP + real workers/clients:

| Test | Asserts |
|---|---|
| `two_workers_commit_disjoint_shards_concurrently` | Two `CompileWorker`s on different shards commit in parallel; both watermarks land; catalog has both. |
| `two_workers_race_one_shard_exactly_one_wins` | A holds the lease inside its work closure; B's `run_shard` on the same shard fails with an error whose text contains "already leased"; A then commits cleanly. |
| `expired_lease_is_fenced_and_next_worker_resumes_from_watermark` | Short TTL. A acquires, commits watermark 3, stops heartbeating. After the TTL, B takes over with a strictly higher token and reads watermark 3 (resumes, not from zero). A's late `manifest_commit` with the stale token is **rejected**; watermark stays 3 (no partial/lost write); B commits 9 on top. |
| `coordinator_state_survives_restart_but_leases_do_not` | Persisting coordinator: catalog + watermark + entity index survive a restart from the same state file; the pre-restart lease is gone (fresh `acquire` succeeds, token sequence restarts). |

Plus 3 `LeaseTable` unit tests.

---

## Knowledge Captured

- **`object_store` 0.14 API shape.** `put` / `get` / `get_range` / `head` / `delete` are **not**
  on `Arc<dyn ObjectStore>` — they moved to the `ObjectStoreExt` blanket trait (RPITIT). `use
  object_store::{ObjectStore, ObjectStoreExt, PutPayload}`. `get_range` takes `Range<u64>` (not
  `usize`). `PutPayload::from(Vec<u8>)`. `NotFound` is `object_store::Error::NotFound { .. }`.
  `list` returns a `Stream`, so `use futures::StreamExt`.
- **`InMemory` from a downstream test crate.** `object_store::memory::InMemory` isn't reachable
  from `ekos-ledger` tests unless `ekos-segment-backend` re-exports it — `#[cfg(feature =
  "object-store")] pub use object_store;`, then the test uses
  `ekos_segment_backend::object_store::memory::InMemory`.
- **Keeping a cloud SDK out of production builds.** Depend on the backend crate with *no
  features*; pull the heavy feature only through a **dev-dep** of the same crate. `cargo build
  --workspace` then never compiles it; `cargo test --workspace` does. Verified by watching the
  compile list.
- **A guard handed to an async closure must be owned, not borrowed.** `run_shard<F, Fut>` where
  `F: FnOnce(LeaseGuard<'_>) -> Fut` fails to compile — "lifetime may not live long enough",
  because the returned future can't be proven to outlive the borrow. Making `LeaseGuard` own its
  `Arc<CoordinatorClient>` + `Arc<AtomicBool>` (instead of `&'a`) removes the lifetime entirely
  and the HRTB closure problem with it.
- **Fencing-token monotonicity vs coordinator restart.** The per-partition token counter is
  in-memory only. A coordinator restart resets it to 0 — which is *safe* precisely because leases
  aren't persisted either, so every pre-restart lease is already dead and can't come back to race
  a token-1 grant. Persisting the counter (but not the leases) would be strictly worse complexity
  for no correctness gain in v1. (Raft-replicated metadata — the named v2 — changes this.)
- **`manifest_commit` doesn't need the manifest.** The coordinator's job is arbitration +
  watermark, not storage. Passing just the generation `u64` keeps the RPC tiny and the manifest
  blob local to the partition, where B4's Service B reads it anyway.
- **`cd ekos` from the repo root is wrong in this workspace** — the shell's cwd is already
  `ekos/`. `tests/integration` and `benchmark` are at `../tests/integration` / `../benchmark`.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0113-…md` | New RFC (`aea491d`); B1/B2/B3 acceptance + wire-protocol decisions + open-question resolutions updated with this devlog |
| `crates/segment-backend/` (new) | `SegmentBackend`, `BackendError`, `LocalFsBackend`, `MemBackend`, `ObjectStoreBackend` (feature-gated), `object_store` re-export |
| `crates/ledger/src/segment/mod.rs` | `backend: Arc<dyn SegmentBackend>` field; sealed publish/fetch + run discovery route through it; `open_with_backend` |
| `crates/ledger/src/lib.rs` | Re-export `SegmentBackend` & friends from `ekos-segment-backend`; `backend.rs` deleted |
| `crates/ledger/Cargo.toml` | Dep on `ekos-segment-backend` (no features); dev-dep with `object-store` |
| `crates/cluster/` (new) | `ekos-cluster`: `catalog`, `lease`, `protocol`, `coordinator`, `client`, `worker`; `tests/harness.rs` |
| `crates/cli/src/commands/cluster.rs` (new) | `ekos coordinator serve`/`status`, `ekos compile-worker run` |
| `crates/cli/src/bin/ekos.rs`, `commands/mod.rs`, `Cargo.toml` | Wire the new subcommands + `ekos-cluster` dep |
| `ekos/Cargo.toml` | `crates/cluster` + `crates/segment-backend` workspace members & deps |
| `ekos/docs/rfcs/0111-…md` | Phase B checklist: B1/B2/B3 ticked |
| `TODO.md` | Phase 6 / Phase B progress note updated through B3 |
