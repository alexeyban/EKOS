# RFC 0113 — Storage Phase B: Distributed Mode Implementation

**Status:** Draft — **B1–B5 all landed 2026-08-29/30** (per user direction, building incrementally
against this RFC while it's still Draft, same as RFC 0111 Phase A). Phase B is feature-complete at
the v1 scope, including **Service A's lease→real-pipeline binding** (2026-08-30): `ekos
compile-worker run` executes the actual `build → recover → resolve → compile → commit` under a
heartbeated, fencing-tokened coordinator lease and commits the resulting manifest generation.
A partition is now **self-describing in object storage** — sealed segments, `manifest.json`,
`dict.bin`, and tantivy's `search/` dir all route through the `SegmentBackend`
(`PartitionedLedger::with_segment_backend`, `[storage.partition] segment-backend-url`); only
`HEAD` and the active/unsealed segment stay local to the writer (writer-only crash-recovery
state a reader never needs). `ekos compile-worker run` publishes `search/` after every compile
(`PartitionedLedger::publish_search_indexes`). **Gateway v1.1 landed 2026-08-31**: `DistributedLedger`
now pools one connection per coordinator/worker address (reconnected on an I/O error, not
reconnected per call), dispatches every multi-partition fan-out concurrently instead of
sequentially, and prunes id-scoped reads (`get_object`, `object_history`, …) to the partitions the
coordinator's real `entity_id → partitions` index names for that id — populated by
`ekos compile-worker run` from each partition's actual object/relationship ids, replacing the
placeholder shard-name entry the index previously held (which had no pruning value). Phase B is
now fully closed at v1 scope — no tracked follow-ons remain.
**Author:** EKOS team
**Created:** 2026-08-29
**Implements:** RFC 0111 §4, §6, §7 (Distributed mode). RFC 0111 doubles as the Phase A
implementation RFC; this is the separate implementation RFC its Acceptance Criteria call for
Phase B. **No code ships until this is Accepted.**

---

## Context

RFC 0111 Phase A (Local mode) is complete for `entity-kind` (`devlog_128`): `PartitionedLedger` is
a drop-in `KnowledgeStore` that routes across many local `FactLedger` partitions, with a persisted
catalog + run-file index and cold tiering. Phase B replaces the single in-process
`PartitionedLedger` with **object storage as the one durable copy** and three independently
scalable services — a compile/ingest cluster, a query cluster, and a stateless query gateway —
per RFC 0111 §6. This RFC turns that architecture into an implementable, sequenced plan and pins
the interface-level decisions RFC 0111 left at design altitude.

RFC 0111's Architecture Review already **resolved** (carried forward here unchanged): async
(tokio) at the coordinator/RPC boundary only, sync compiler passes via `spawn_blocking`;
short-TTL write leases (≈30 s) with heartbeat (≈10 s), no dead-worker buffer recovery;
fencing tokens for manifest-commit contention; mutual TLS over a cluster-internal CA (v1);
Local mode stays the default indefinitely; single-coordinator v1 (acknowledged SPOF),
Raft-replicated metadata a named v2.

## Scope

- **B1 — `SegmentBackend` seam** (RFC 0111 §4): a trait behind which `SegmentStore` reads/writes
  sealed objects; `LocalFsBackend` wrapping today's exact `std::fs` behaviour, proven equivalent.
- **B2 — `ObjectStoreBackend`**: `object_store` crate integration (S3 / Azure ADLS Gen2 /
  S3-compatible / in-memory), byte-identical segment contents vs `LocalFsBackend`.
- **B3 — Coordinator + distributed single-writer**: the metadata service (catalog, leases,
  fencing tokens, tx watermarks), and Service A (`ekos compile-worker serve`) writing partitions
  through it.
- **B4 — Service B / Service C + distributed reads**: stateless query workers over cached
  object-storage partitions; the `DistributedLedger` gateway implementing `KnowledgeStore`.
- **B5 — Distributed search** (RFC 0111 §7): Service C fan-out + per-partition BM25 merge.

## Non-goals

- Changing the segment/frame format, `FactIndexes`, the append-only invariant, or anything in
  Phase A — reused byte-for-byte; only *where bytes live* and *who reaches them* changes.
- Raft-replicated coordinator metadata (named v2 in RFC 0111; revisited only on a real v1-SPOF
  incident).
- Auto-scaling / orchestration (k8s operators, autoscalers) — out of scope; operators run N
  worker processes however they run processes.
- Cross-cloud beyond what `object_store` gives for free.
- Global term-statistics aggregation for search (RFC 0111 §7 accepts the query-then-fetch
  approximation for v1).

## B1 — The `SegmentBackend` seam

### Interface (settled in the B1 interface pass)

Sealed, immutable objects only — one publish, many reads, never read-modify-write. Same DI pattern
as `Observer`/`LlmProvider`/`CompilerPass`.

```rust
pub trait SegmentBackend: Send + Sync {
    /// Make a just-sealed object durable in the backend. Its bytes currently live at `staged`
    /// (the writer's local file, already fsynced). `LocalFsBackend`: the file *is* the durable
    /// copy — fsync it and its dir. `ObjectStoreBackend`: PUT it; the staging copy may then be
    /// dropped.
    fn publish_sealed(&self, key: &str, staged: &Path) -> Result<(), BackendError>;

    /// A readable **local path** for a sealed object — fetched into a bounded local cache first if
    /// the backend is remote. `LocalFsBackend` returns the file in place. Callers `mmap` the
    /// result (`MappedSegment`), so a path (not `Vec<u8>`) keeps B1 a true zero-behaviour-change
    /// refactor.
    fn fetch(&self, key: &str) -> Result<PathBuf, BackendError>;

    /// Sealed objects present under `prefix` (e.g. `"segments/"`). Used by Service B (B4) to pull a
    /// whole partition; `SegmentStore` itself discovers segments from the manifest, not this.
    fn list(&self, prefix: &str) -> Result<Vec<String>, BackendError>;
    fn exists(&self, key: &str) -> Result<bool, BackendError>;
    /// Compaction / vacuum only.
    fn delete(&self, key: &str) -> Result<(), BackendError>;
}
```

`key` is backend-relative, mirroring today's local layout 1:1 within a partition:
`segments/seg-<seq>.facts`, `indexes/<order>/run-*.bin`, `search/*`. **B2 adds `get(key) ->
Vec<u8>` and `get_range(key, Range<u64>) -> Vec<u8>`** for the remote query path (Service B pulling
individual frames without a full download); B1 doesn't need them because `LocalFsBackend::fetch`
returns a path and mmap is unchanged.

### Crate layout

`crates/segment-backend` (`ekos-segment-backend`) — `SegmentBackend`, `BackendError`,
`LocalFsBackend`, and `MemBackend` (an in-memory backend, no external deps: the "publish bytes →
download to cache → mmap cache" fixture the B3/B4 harness builds on). `object_store` is an
**optional** dep behind the `object-store` feature (default off), with `aws` / `azure` features
layering the cloud SDKs on top. `ekos-ledger` depends on it (no features — a Local build never
compiles `object_store`) and re-exports the trait so `ekos_ledger::SegmentBackend` stays the
import path.

### What changes in `SegmentStore`

Exactly four call sites — the ones that touch a **sealed** segment by path:

- `seal_active()` — after the local active file is frozen and hashed, call
  `backend.publish_sealed("segments/seg-<seq>.facts", &local_path)`. Hashing + the tx-range mmap
  still read the local staged file directly (it is local at seal time in every mode).
- `batches_after()`, `batch_headers()`, `verify_sealed_report()` — replace
  `segment_path(&self.root, seq)` with `backend.fetch(&seg_key(seq))?` before `MappedSegment::open`
  / `hash_file`.

Everything else is **unchanged and stays local**: the active (unsealed) segment (object stores
have no append — it uploads only when it seals), `HEAD`, `manifest.json` (write-temp→fsync→rename
in Local mode; coordinator-arbitrated in Distributed, B3 — `SegmentBackend` carries no
manifest-commit atomicity), `dict.bin`, and tantivy's own `search/` I/O (Service B downloads that
directory into a cache before opening it, B4 — tantivy is never handed a `SegmentBackend`).

`SegmentStore` gains a `backend: Arc<dyn SegmentBackend>` field and an `open_with_backend`
constructor; `open` / `open_with_seal_threshold` default it to `LocalFsBackend::new(root)`, so
`FactLedger` and every existing caller are untouched. `SegmentStore` never calls `list` (it uses
the manifest); that method exists for B4.

### `LocalFsBackend` — the equivalence proof

Wraps the exact `std::fs` calls being replaced, rooted at a `PathBuf`. **Acceptance for B1 is a
no-behaviour-change refactor**: every existing `ekos-ledger` test passes unchanged with
`LocalFsBackend` wired in as the default, and a new test asserts a `SegmentStore` built on
`LocalFsBackend` produces byte-identical on-disk trees to one built the old way for the same write
sequence.

### Crate layout

New crate `ekos-segment-backend` (trait + `LocalFsBackend` + `BackendError`), depended on by
`ekos-ledger`. `ObjectStoreBackend` lands in the same crate behind an `object-store` feature flag
so a Local-mode build doesn't pull the AWS/Azure SDK transitive tree.

## B2 — `ObjectStoreBackend`

Built on the `object_store` crate (Apache Arrow ecosystem — one trait for `AmazonS3`,
`MicrosoftAzure`, `Http`/S3-compatible, `LocalFileSystem`, and `InMemory`). Config:

```toml
[storage.backend]
kind = "object-store"          # "local-fs" (default) | "object-store"
url  = "s3://ekos-prod-ledger" # or az://…, or file:///… (dev), or memory:// (tests)
# credentials via the standard provider env vars / instance metadata — never in ekos.toml
```

- `put`/`get`/`list`/`delete`/`exists` map directly onto `ObjectStore` methods; `get_range` onto
  `get_opts` with a byte range.
- **The sync/async bridge (settled):** a dedicated **current-thread `tokio::runtime::Runtime` per
  `ObjectStoreBackend`**, `block_on`-ing each call. Safe from a `spawn_blocking` thread (§B2 —
  Service A/B run the sync passes on blocking threads) and from a plain sync test; must not be
  called from *within* another runtime's async context. This is the one place RFC 0001's
  sync-pipeline decision meets object storage — a thin edge, not a retrofit.
- `object_store` 0.14's `put`/`get`/`get_range`/`head`/`delete` live on the `ObjectStoreExt`
  blanket trait (RPITIT moved them off `dyn ObjectStore`); the backend `use`s it.
- **Acceptance (met):** `ObjectStoreBackend` passes the shared `SegmentBackend` contract test
  against `object_store::memory::InMemory`; a full `SegmentStore` write → seal → **drop the local
  cache** → reopen → read round-trip works with data living only in object storage
  (`segment_store_round_trips_on_object_store_backend`). MinIO-in-a-container is a later
  integration check, not a B2 blocker.

## B3 — Coordinator + distributed single-writer

### The coordinator

A single process, `ekos coordinator serve --listen <addr> --backend <url>`. Holds, in a small
embedded store (sled or a single SQLite file — **decided in B3**, not here; it is *metadata only*,
not ledger data):

- **The partition catalog** — same shape as Phase A's `PartitionCatalog`, plus `PartitionLocation`
  = `ObjectStore(prefix)` for every entry.
- **Write leases** — `partition_key → (holder_worker_id, fencing_token, expires_at)`. Token is a
  per-partition monotonic `u64`, incremented on every grant.
- **Committed tx watermark** per partition — the last manifest generation the coordinator has
  accepted.

RPC surface (see "wire protocol" below):

```
lease_acquire(partition_key, worker_id)      -> Lease { token, ttl } | AlreadyLeased
lease_renew(partition_key, worker_id, token) -> ok | Expired | Fenced
lease_release(partition_key, worker_id, token)
manifest_commit(partition_key, token, new_manifest) -> ok | Fenced | StaleWatermark
catalog_get(scope_predicate)                 -> Vec<PartitionMeta>
catalog_register(partition_key, location)    -> PartitionMeta   // first write to a new partition
partitions_for_entity(entity_id, kind)       -> Vec<partition_key>   // the run-file index, served centrally
```

**The run-file index moves to the coordinator in Distributed mode.** In Local mode it's the
per-workspace `index/run-*.jsonl` (Phase A); distributed, the coordinator owns the
`id → partitions` mapping (obj/rel/endpoint/evt/evid) and Service A workers push new pairs to it
on write, Service C reads it for pruning. The run-file format is reused as the coordinator's
on-object-storage backup of that index, compacted the same way.

### Service A — compile/ingest worker

`ekos compile-worker serve --coordinator <addr>`. A worker loop:

1. Ask the coordinator for an unleased shard `(dimension_value, time_bucket)` (or take an
   operator-assigned one).
2. `lease_acquire`; on success it is the sole writer for that shard, fencing token in hand.
3. Run the **existing, unmodified** `build/recover/resolve/compile/commit` passes for that shard's
   inputs (via `spawn_blocking` — passes stay sync). Appends go to a local buffered active segment.
4. At `SEGMENT_SEAL_BYTES`: seal → `backend.put` the sealed object → `manifest_commit` through the
   coordinator with the fencing token. A stale token (lease expired, someone else took it) is
   **rejected** by the coordinator; the worker discards its buffer and stops.
5. Heartbeat `lease_renew` every ~10 s; TTL ~30 s.
6. On clean finish: flush + seal the tail, final `manifest_commit`, `lease_release`.

Crash: the lease expires; the next worker `lease_acquire`s the shard and resumes from the last
committed manifest — **losing at most the un-sealed ≤8 MB buffer** (RFC 0111 §4, accepted).

## B4 — Service B / Service C + distributed reads

### Service B — query / EAV-assembly worker

`ekos query-worker serve --coordinator <addr>`. Stateless compute:

1. Coordinator assigns it a set of partitions (or it pulls on demand per query).
2. For each: `backend.list` + `backend.get` the sealed segments, index runs, and `search/` dir
   into a **bounded local cache** (`~/.cache/ekos/query-worker/<partition-id>/`), mmap'd once on
   disk — RFC 0016's mmap reads apply unchanged to the cached copy.
3. Run the **existing, unmodified** `FactIndexes` EAVT/AEVT/AVET fold and tantivy search against
   the cache.
4. Serve `get_object` / `object_history` / `relationships_for` / `search(query, k)` / `diff` /
   `*_at` for its assigned partitions over RPC.

Because sealed segments are immutable and object storage is the one durable copy, **any** worker
can serve **any** partition — no owned/replica set. Losing a worker loses only warm cache.

**Cache eviction** (RFC 0111 Open Question): v1 = LRU by partition, size-bounded by a config
`[storage.query-cache] max-bytes`. This RFC does not commit to a smarter policy — it needs real
query-pattern data (deferred, tracked).

### Service C — query gateway

`ekos gateway serve` (or `ekos mcp serve --distributed <coordinator-addr>`). Stateless, N
interchangeable replicas. Its core is:

```rust
pub struct DistributedLedger {
    coordinator: CoordinatorClient,
    workers: WorkerPool,   // RPC clients, health-checked
}
impl KnowledgeStore for DistributedLedger { /* fan out + merge */ }
```

- `get_object(id)` → `coordinator.partitions_for_entity(id)` → pick newest → RPC that partition's
  Service B worker → pass through.
- `object_history(id)` / `relationships_for(id)` → all of the entity's partitions → parallel RPC
  → merge in tx order (§2) / dedup by id.
- `all_objects` / `all_objects_at` → coordinator catalog → every object partition → parallel RPC
  → dedup newest.
- `find_objects(query)` → B5.
- `diff(from,to)` → per-partition `diff` RPC → merge (`touched`/`unchanged` merge; `added` stays
  per-partition-local, as in Phase A).
- **Writes** (`append_*`) on `DistributedLedger` are rejected — writes go through Service A only.
  `ekos build`/`commit` in Distributed mode *is* Service A; the gateway is read-only, matching
  the Runtime-is-read-only invariant.

**No caller of `KnowledgeStore` changes** — Runtime, MCP tool handlers, `docs-gen` all see the
same trait. `open_store` gains a fourth branch: `[storage.distributed] coordinator = "…"` →
`DistributedLedger`.

## B5 — Distributed search

Service C fans `search(partition, query, k)` to the Service B workers holding the pruned partition
set, merges each worker's local BM25 top-K. **Per-partition term statistics** → the standard
query-then-fetch approximation (Elasticsearch's default), *not* a global ranking — RFC 0111 §7,
accepted for v1. Cold partitions (not cached by any worker) are rehydrated by whichever worker is
assigned, or skipped with a flag, same as Phase A's `find_objects`.

## Wire protocol

**Decided in B3 (deviations from the design-altitude sketch above, recorded here):**

- **Transport — newline-delimited JSON-RPC over TCP, not gRPC/tonic.** One JSON `Request` per line
  in, one `Response` per line out (`crates/cluster/src/protocol.rs`). Rationale: this is the exact
  pattern `ekos mcp serve` already uses (RFC 0013) — no protobuf toolchain, no tonic dependency
  tree, one framing the codebase already understands. The coordinator's RPCs are all small
  request/response (no segment bytes ever cross it — those go object-store↔worker directly), so
  gRPC's streaming advantage doesn't apply. tonic stays on the table for B4's Service-B segment
  transfers if a real need shows up.
- **Mutual TLS — deferred.** v1 assumes a trusted cluster network / localhost. The transport is a
  plain `TcpStream`; wrapping it in `tokio-rustls` is a transport-level follow-on that changes no
  protocol code. Cert-rotation mechanics deferred with it.
- **Metadata store — a single JSON file (atomic temp+rename), not sled/SQLite.** The coordinator's
  entire persisted state (catalog + per-partition watermarks + entity→partitions index) is a few
  KB of `serde_json`; an embedded KV/SQL engine is unwarranted weight for v1. Leases are **not**
  persisted — they are TTL-bounded, so a coordinator restart correctly invalidates every
  outstanding one (the monotonic fencing-token counter restarts too, which is safe because every
  pre-restart lease is definitionally dead). Raft-replicated metadata remains the named v2.
- **Crate — new `crates/cluster` (`ekos-cluster`)**, not `crates/cli` subcommands. It carries the
  coordinator, the client, the lease table, the protocol types, and `CompileWorker` (Service A's
  transport/lifecycle half). `PartitionId` is an **opaque `String`** here (`"<dimension_value>/
  <time_bucket>"`) so `ekos-cluster` needs no `ekos-ledger` dependency. `crates/cli` gets thin
  `ekos coordinator serve` / `ekos coordinator status` / `ekos compile-worker run` wrappers.
- **Async boundary**: the coordinator server and clients are async (tokio); when B4 wires Service
  A's lease to a real shard-scoped `build → commit`, every call into a compiler pass or
  `FactIndexes` fold is `spawn_blocking`. RFC 0001's sync-pipeline decision is untouched.

### `ekos-cluster` public surface (as built)

```
Coordinator::open(state_path) | ::ephemeral() | .with_ttl(d)     // in-memory state + JSON persistence
serve(Arc<Mutex<Coordinator>>, TcpListener)                       // NDJSON accept loop
spawn_ephemeral(addr, ttl) -> (SocketAddr, JoinHandle)            // test/`main` helper
CoordinatorClient::connect(addr) + typed helpers                  // one held-open TCP conn
CompileWorker::new(client, id).with_heartbeat(d).run_shard(p, |guard| async { … })
LeaseGuard { .token(), .partition(), .commit(watermark) }         // owned; fenced commit -> LostLease
LeaseTable::{acquire, check, renew, release}                      // fencing-token core (unit-tested)
```

RPC surface actually implemented: `CatalogRegister`, `CatalogGet{prefix}`, `LeaseAcquire`,
`LeaseRenew`, `LeaseRelease`, `ManifestCommit{watermark}`, `RecordEntityPartitions`,
`PartitionsForEntity`, `Watermark`. (`ManifestCommit` carries the new watermark directly rather
than a full manifest blob — the coordinator only ever needs the generation number; the manifest
itself lives with the partition.)

## Sequencing & Acceptance

Each sub-phase gated by its own review; B(n+1) does not start until B(n)'s acceptance passes.

| Sub-phase | Acceptance |
|---|---|
| **B1 ✅ (2026-08-29)** | `SegmentBackend` + `LocalFsBackend` (`crates/ledger/src/backend.rs`); `SegmentStore` routes sealed-segment publish/fetch (`seal_active`, `batches_after`, `batch_headers`, `verify_sealed_report`) through it; `open`/`open_with_seal_threshold` default to `LocalFsBackend`, `open_with_backend` for the rest. All 139 prior `ekos-ledger` tests green unchanged + `sealed_io_routes_through_the_segment_backend` (a counting backend proves the routing) + `local_fs_backend_round_trips`. |
| **B2 ✅ (2026-08-29)** | `crates/segment-backend` extracted (`SegmentBackend` + `LocalFsBackend` + `MemBackend` + `BackendError`, `get`/`get_range` added). `ObjectStoreBackend` behind the `object-store` feature (`object_store` 0.14, dedicated current-thread runtime). Contract test vs `InMemory`; `SegmentStore` round-trip on object storage with the local cache wiped mid-test. `ekos-ledger` lib build never compiles `object_store` (dev-dep only). MinIO integration check deferred (not a blocker). |
| **B3 ✅ (2026-08-29)** | `crates/cluster` (`ekos-cluster`): `Coordinator` (catalog + leases + fencing tokens + watermarks + entity index, JSON persistence), `serve` (NDJSON-over-TCP), `CoordinatorClient`, `CompileWorker`/`LeaseGuard` (Service A transport+lifecycle), `LeaseTable`. `crates/cli`: `ekos coordinator serve`/`status`, `ekos compile-worker run`. Harness (`crates/cluster/tests/harness.rs`, 4 tests): disjoint-shard concurrent commit; **lease contention** (two workers, one shard, exactly one wins, loser gets an "already leased" error); **expired-lease fencing** (worker stops heartbeating → TTL lapses → next worker takes over with a higher token, resumes from the committed watermark, the stale worker's late `manifest_commit` is rejected, no partial/lost write); coordinator-restart durability (catalog + watermarks survive, leases don't). Plus 3 `LeaseTable` unit tests. |
| **Service A real pipeline ✅ (2026-08-30)** | `ekos compile-worker run --coordinator <addr> --shard <name> --workspace <dir>`: acquires the `shard` lease (heartbeated), runs the **real** `build → recover → resolve → compile → commit` on a blocking thread with its own runtime (so the worker's executor stays free to heartbeat through a multi-minute compile), then registers every partition it wrote (`CatalogRegister` + `RecordEntityPartitions`) and `manifest_commit`s the store's monotonic entry count as the generation watermark — fenced, so a lost-lease worker's late commit is rejected. Requires a Local `[storage.partition]` workspace (not `[storage.distributed]`), partition roots on storage the query workers can also reach. Integration test (`compile_worker_runs_the_real_pipeline_under_a_lease`): coordinator + `compile_worker_run` over the ecommerce SQL fixture → 6 Table partitions registered, watermark advanced, the partitioned store really holds the 6 tables. |
| **B4a ✅ (2026-08-30)** | New crate `ekos-distributed`. `QueryWorker` (Service B): on the first request for a partition it asks the coordinator where the partition lives, materialises it into a bounded local cache (`PartitionCache` — object storage → local files via `ObjectStoreBackend`, or a co-located local dir used in place), opens it as a read-only `FactLedger`, and serves every `KnowledgeStore` read for it (`get_*`, `all_*`, `*_history`, `relationships_for`, `object_at`/`*_at`, `find_objects`, `diff`, counts) over newline-delimited JSON-RPC (`WorkerRequest`/`WorkerResponse`, `spawn_blocking` around every ledger call). `QueryWorkerClient`. `ekos query-worker serve`. `ObjectStoreBackend::from_url` (`s3://`/`az://`/`gs://`/`file://`). Object-storage partitions need a build `--features distributed`; a co-located Local cluster works without it. Test: coordinator + worker over a real `PartitionedLedger` workspace, every read over RPC == a direct read; object-storage partition materialisation via a `file://` backend. |
| **B4b ✅ (2026-08-30)** | `DistributedLedger` (`ekos-distributed`): `impl KnowledgeStore` — every read fans across the query workers named by the coordinator catalog and merges (newest-partition-wins for current-state, concat-oldest-first for history, `PartitionedLedger`'s own merge rules for `diff`); classifies partitions by id prefix (`rel:` / `events/` / `evidence/` / object); `append_*` + `vacuum_into` return `LedgerError::ReadOnly` ("goes through Service A"). Sync↔async bridge: `block_in_place` on the ambient runtime, or a transient current-thread runtime — **never owns a `Runtime`** (a stored one panics when dropped under `#[tokio::main]`, which is how `open_store` is reached). `crates/cli/src/commands/store.rs`: `[storage.distributed]` branch in `open_store`/`open_store_read_only`/`store_display`, ahead of every local backend. `compiler-core`: `StorageDistributedConfig` (strings only). Test: `DistributedLedger` over a 2-worker cluster answers `get_object`/`all_objects`/`object_history`/`relationships_for`/`object_at`/counts identically to the in-process `PartitionedLedger`, newest version wins, cross-partition objects resolve, writes rejected. **v1 limits (follow-ons):** no persistent connection pool (fresh connect per call); sequential fan-out (not parallel); no coordinator-index pruning (fans to every partition of the class); no `ekos` command yet to register an existing Local partitioned workspace's partitions with a coordinator. |
| **B5 ✅ (2026-08-30)** | `SearchIndex::query_scored` / `FactLedger::find_objects_scored(query, limit)` expose the raw tantivy BM25 score (`find_objects` now delegates through them, unchanged behaviour). `WorkerRequest::FindObjectsScored { k }` → `ScoredHits`. `DistributedLedger::search(query, k)` fans each **object** partition's local top-`k` to a worker, merge-sorts by score, dedups by id (highest shard-local score kept), truncates to `k`; `find_objects` (the trait method) rides on it with `k = 50`. Scores are **shard-local** — per-partition term statistics, the RFC 0111 §7 query-then-fetch approximation, *not* a corpus-global ranking. Test (`tests/search.rs`): matches split Table/File across two workers merge into one score-ordered, de-duplicated, `k`-bounded list whose id set == the in-process `PartitionedLedger`'s; explicitly asserts the shard-local-IDF caveat (the exact-name hit is *not* forced to rank #1). |

**Whole phase:** no `KnowledgeStore` caller changed; Local mode entirely unaffected (default,
`SegmentBackend` = `LocalFsBackend`, no coordinator running); benchmark showing distributed
compile throughput scales ~linearly with Service A worker count on an N-shard fixture.

## Architecture Review (2026-08-29)

Validated against `ekos.md` and CLAUDE.md's key invariants — this is the *implementation* plan for
a design RFC 0111's own Architecture Review already cleared; the checks here are that the
implementation-level choices don't reintroduce a violation.

- **Append-only ledger** — untouched. `SegmentBackend` writes only *sealed* (already-immutable)
  objects; `delete` is compaction/vacuum only (the same operations Phase A already permits on
  sealed data). No in-place mutation anywhere.
- **Evidence-traceable conclusions** — untouched. Distribution moves *where* facts live and *who*
  serves them; the fact/evidence primitives and their linkage are byte-identical.
- **Runtime is read-only** — `DistributedLedger` (Service C) rejects every `append_*`; writes go
  through Service A only, which *is* the compiler pipeline. The gateway holds no partition data.
- **Compiler passes are deterministic + side-effect-free** — Service A runs the **existing,
  unmodified** passes via `spawn_blocking`; the only new side effect is "seal + PUT + coordinator
  commit", which sits *around* the passes, not inside them (exactly as `ekos commit` already sits
  around them today).
- **AI consumes knowledge through the Runtime only** — MCP still talks to `KnowledgeStore`; in
  Distributed mode that's `DistributedLedger` behind the same `ekos mcp serve`. No AI path reaches
  object storage or a Service B worker directly.
- **Dependency injection through traits** — `SegmentBackend` follows `Observer`/`LlmProvider`/
  `CompilerPass`; the coordinator/worker RPC clients are traits with a real impl and a test
  double.
- **Technology independence** (`ekos.md`) — `SegmentBackend`'s `LocalFs`/`ObjectStore` split *is*
  the storage substitution this principle anticipates; `object_store` keeps S3/Azure/GCS/MinIO
  behind one trait.
- **RFC 0001 sync pipeline** — preserved. Async lives only at the tonic/RPC edge; every call into
  a pass or a `FactIndexes` fold is `spawn_blocking`. New edge beside the decision, not a retrofit
  through it.
- **Bias against speculative engineering** (RFC 0080 precedent) — Phase B is opt-in, Local mode
  stays the default indefinitely, single-coordinator v1 is an *acknowledged* SPOF with Raft named
  but not built, and the ≤8 MB loss window is accepted for v1 rather than engineered away
  pre-evidence.

## Open Questions

- [x] Coordinator metadata store — **resolved in B3**: a single JSON file (atomic temp+rename),
      not `sled`/SQLite; leases not persisted (TTL-bounded).
- [x] `object_store` sync bridge — **resolved in B2**: a dedicated current-thread `Runtime` per
      `ObjectStoreBackend`.
- [x] Coordinator RPC location — **resolved in B3**: new `crates/cluster` (`ekos-cluster`);
      `crates/cli` gets thin wrapper subcommands.
- [x] Transport — **resolved in B3**: newline-delimited JSON-RPC over TCP (the `ekos mcp serve`
      pattern), not gRPC/tonic. Revisit for B4 Service-B segment transfers only if needed.
- [ ] Service B cache eviction beyond size-bounded LRU (RFC 0111 Open Question — needs real
      query-pattern data; not a B4 blocker).
- [x] Gateway v1 → v1.1 — **resolved 2026-08-31**: a pooled connection per coordinator/worker
      address (`ConnSlot`, reconnect-and-retry-once on an I/O error, replacing v1's connect-fresh-
      per-call); every multi-partition fan-out (`fan_out`/`first_present`) dispatches concurrently
      via `futures::future::join_all`/`try_join_all` instead of a sequential loop, while preserving
      each method's original merge order (newest-partition-wins, oldest-first concat, "first
      candidate wins" for id-scoped lookups); id-scoped reads now prune via
      `candidate_partitions`, which consults the coordinator's `entity_id → partitions` index
      (populated by `ekos compile-worker run` from each partition's real object/relationship ids,
      via new `PartitionedLedger::partition_entity_ids`) and falls back to a full class scan only
      when the index has nothing for that id (events/evidence — unindexed, no `all_events`/
      `all_evidence` to enumerate ids from — and any not-yet-recompiled workspace). Broad reads
      with no id to prune by (`all_objects`, `relationships_for`, `diff`, …) still fan to every
      partition of the class — inherent to the query, not a pruning gap. Found and fixed in the
      same pass: the compile-worker's prior `record_entity_partitions` call recorded the *shard
      name* mapped to every partition it produced, not any real object/relationship id — a
      placeholder with no pruning value that a dedicated test now proves the gateway does not
      silently fall back past (`gateway_uses_the_entity_index_to_prune_when_present`, which
      mis-registers an id against the wrong partition and asserts the lookup misses). Test
      (`tests/integration.rs`) also caught a pre-existing latent bug the old placeholder was
      masking: the watermark assertion checked `watermark(catalog[0].id)` (a physical partition
      id), but watermarks are tracked per lease/shard name — always `0` under a partition id, true
      by coincidence only via the `||` against the placeholder entity-index check being removed.
- [x] Registering a Local partitioned workspace's partitions with a coordinator — **resolved**:
      `ekos compile-worker run` does it (`CatalogRegister` per partition + `RecordEntityPartitions`)
      after each compile.
- [x] `PartitionedLedger` writing through `SegmentBackend` — **resolved 2026-08-30**:
      `FactLedger::open_with_backend` / `open_read_only_with_backend` +
      `PartitionedLedger::with_segment_backend(resolver)` route each partition's **sealed segments**
      *and* its mutable metadata (`manifest.json`, `dict.bin`) through the `SegmentBackend` (new
      `SegmentBackend::publish` for overwriteable metadata alongside `publish_sealed`);
      `[storage.partition] segment-backend-url = "s3://…"` wires an `ObjectStoreBackend` per
      partition (cli `distributed` feature). A partition is now **self-describing in object
      storage**. **Still local per partition:** `HEAD` (active-segment watermark) + the
      active/unsealed segment (writer-only crash-recovery state a reader never needs).
      **Search resolved 2026-08-31**: `SegmentStore::publish_aux`/`fetch_aux` push/pull a flat
      directory's files through the backend under the same `<rel>/…` keys; `FactLedger` calls
      `fetch_aux("search")` on `open_read_only_with_backend` when no local `search/` exists (an
      unsynced partition degrades to zero search hits — every other read is unaffected — rather
      than erroring), and exposes `sync_search_to_backend()` for a writer to call post-commit;
      `PartitionedLedger::publish_search_indexes()` does it for every catalogued partition;
      `ekos compile-worker run` calls it after each compile, before registering partitions.
- [ ] Interrupting an in-flight compile when the lease is lost mid-run — v1 lets the pipeline
      finish, then the fenced `manifest_commit` fails (`LostLease`); the per-`FactLedger`
      `write.lock` is the real guard against a concurrent second writer.
- [x] `DistributedLedger` search — **resolved in B5**: `search(query, k)` does the per-partition
      BM25 top-`k` merge; `find_objects` rides on it. Cross-shard IDF is the accepted v1
      approximation (a global term-statistics pass is explicitly out of scope, RFC 0111 §7).
- [ ] Shrinking the ≤8 MB unsealed-segment loss window (RFC 0111 Open Question — periodic partial
      upload vs a lease-handoff-survived local WAL; not a B3 blocker, v1 accepts 8 MB).
- [ ] Mutual TLS + cert rotation — **deferred past B3**: v1 assumes a trusted cluster network;
      wrapping the `TcpStream` in `tokio-rustls` changes no protocol code.
- [ ] Coordinator consensus v1→v2 timing (RFC 0111 — deliberately deferred, not an acceptance
      blocker).

## Files Changed (projected)

| File / area | Change |
|---|---|
| `crates/segment-backend/` ✅ | `SegmentBackend` trait, `LocalFsBackend`, `MemBackend`, `ObjectStoreBackend` (feature-gated), `BackendError` |
| `crates/ledger/src/segment/mod.rs` | Route sealed-object reads/writes + run discovery through `SegmentBackend`; active segment + Local-mode manifest unchanged |
| `crates/ledger/src/partitioned/` | `PartitionLocation::ObjectStore`; catalog/index served from the coordinator in Distributed mode |
| `crates/cluster/` ✅ | `ekos-cluster`: `Coordinator` + `serve` (NDJSON/TCP) + `CoordinatorClient` + `CompileWorker`/`LeaseGuard` + `LeaseTable` + protocol types; harness test |
| `crates/cli/src/commands/cluster.rs` ✅ | `ekos coordinator serve`/`status`, `ekos compile-worker run` (thin wrappers over `ekos-cluster`) |
| `crates/distributed/` ✅ (B4, B5) | `ekos-distributed`: `QueryWorker` + `serve` (Service B, NDJSON/TCP) + `QueryWorkerClient` + `PartitionCache` (object storage → local cache) + `DistributedLedger` (`impl KnowledgeStore` + `search(query, k)`, Service C); `tests/query_worker.rs`, `tests/gateway.rs`, `tests/search.rs` |
| `crates/ledger/src/{search,fact_ledger}.rs` ✅ (B5) | `SearchIndex::query_scored` / `FactLedger::find_objects_scored` — expose the BM25 score; `find_objects` delegates, behaviour unchanged |
| `crates/cli/src/commands/cluster.rs` ✅ (B4a, Service A) | `ekos query-worker serve`; `ekos compile-worker run` — the real pipeline under a lease |
| `crates/cli/src/commands/store.rs` ✅ (Service A) | `build_partitioned` → `pub(crate)`; `[storage.partition] segment-backend-url` → an `ObjectStoreBackend` per partition (feature-gated) |
| `crates/ledger/src/{fact_ledger,partitioned}.rs` ✅ (SegmentBackend writes) | `FactLedger::open_with_backend` / `open_read_only_with_backend` / `..._and_seal_threshold`; `PartitionedLedger::with_segment_backend(resolver)` |
| `crates/ledger/src/segment/mod.rs` ✅ (manifest publishing) | `manifest.json` / `dict.bin` publish through `SegmentBackend::publish` + load via `exists`/`get`; only `HEAD` + active segment stay local |
| `crates/segment-backend/src/*` ✅ | `SegmentBackend::publish(key, bytes)` (overwriteable metadata) with impls on `LocalFsBackend` (atomic write), `ObjectStoreBackend` (PUT), `MemBackend` |
| `crates/compiler-core/src/config.rs` ✅ | `StoragePartitionConfig::segment_backend_url` |
| `crates/cli/src/commands/cluster.rs` ✅ | `collect_partitions` registers `PartitionLocation::ObjectStore` when `segment-backend-url` is set |
| `crates/segment-backend/src/object_store_backend.rs` ✅ (B4a) | `ObjectStoreBackend::from_url` + `object_store/fs` |
| `crates/cli/src/commands/store.rs` ✅ (B4b) | `[storage.distributed]` branch in `open_store`/`open_store_read_only`/`store_display` |
| `crates/ledger/src/segment/mod.rs` ✅ (search publishing) | `SegmentStore::publish_aux(rel)` / `fetch_aux(rel)` — generic flat-dir push/pull through `SegmentBackend`, used for `search/` |
| `crates/ledger/src/fact_ledger.rs` ✅ (search publishing) | `open_read_only_with_backend` fetches `search/` from the backend when absent locally; `sync_search_to_backend()` publishes it |
| `crates/ledger/src/partitioned/mod.rs` ✅ (search publishing) | `PartitionedLedger::publish_search_indexes()` — syncs every catalogued partition's search index |
| `crates/cli/src/commands/cluster.rs` ✅ (search publishing) | `compile_worker_run` calls `publish_search_indexes()` after each compile, before registering partitions |
| `crates/distributed/src/gateway.rs` ✅ (gateway v1.1) | `ConnSlot` pooled coordinator/worker connections with reconnect-and-retry; `fan_out`/`first_present` concurrent dispatch; `candidate_partitions` id-index pruning with class-scan fallback |
| `crates/distributed/Cargo.toml` ✅ (gateway v1.1) | `futures.workspace = true` (`join_all`/`try_join_all`) |
| `crates/ledger/src/partitioned/mod.rs` ✅ (gateway v1.1) | `PartitionedLedger::partition_entity_ids(key)` — every object/relationship id a catalogued partition holds |
| `crates/cli/src/commands/cluster.rs` ✅ (gateway v1.1) | `finalize_partitions` (renamed from `collect_partitions`) also collects `(id, partition)` pairs and calls `record_entity_partitions` per real id, replacing the old shard-name placeholder |
| `crates/compiler-core/src/config.rs` ✅ (B4b) | `StorageDistributedConfig` (`[storage.distributed]`) — strings only |
| `crates/cli/Cargo.toml` ✅ (B4a) | `distributed` feature = `ekos-distributed/object-store` (off by default; a stock build never compiles `object_store`) |
| `ekos/docs/rfcs/0111-…md` | Phase B checklist ticked as B1–B5 land |
