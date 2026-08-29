# RFC 0113 — Storage Phase B: Distributed Mode Implementation

**Status:** Draft — **B1 + B2 + B3 landed 2026-08-29** (per user direction, building incrementally
against this RFC while it's still Draft, same as RFC 0111 Phase A). B4–B5 not started.
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
| **B3 ✅ (2026-08-29)** | `crates/cluster` (`ekos-cluster`): `Coordinator` (catalog + leases + fencing tokens + watermarks + entity index, JSON persistence), `serve` (NDJSON-over-TCP), `CoordinatorClient`, `CompileWorker`/`LeaseGuard` (Service A transport+lifecycle), `LeaseTable`. `crates/cli`: `ekos coordinator serve`/`status`, `ekos compile-worker run`. Harness (`crates/cluster/tests/harness.rs`, 4 tests): disjoint-shard concurrent commit; **lease contention** (two workers, one shard, exactly one wins, loser gets an "already leased" error); **expired-lease fencing** (worker stops heartbeating → TTL lapses → next worker takes over with a higher token, resumes from the committed watermark, the stale worker's late `manifest_commit` is rejected, no partial/lost write); coordinator-restart durability (catalog + watermarks survive, leases don't). Plus 3 `LeaseTable` unit tests. Binding a lease to a real shard-scoped `build → commit` run is **B4**. |
| **B4** | Service B + Service C; `DistributedLedger` passes the same `KnowledgeStore` behavioural suite `PartitionedLedger` does, over RPC; cache-miss-then-hit test; entity-spanning-partitions full-history read fans to the right workers. |
| **B5** | Distributed search merge test — matching docs split across ≥2 partitions on different workers, merged top-K correctly ranked per-shard, cross-shard BM25 caveat exercised. |

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
| `crates/cli` `query-worker`/`gateway` | B4 |
| `crates/runtime` / `crates/cli/src/commands/store.rs` | `DistributedLedger: KnowledgeStore`; `open_store` `[storage.distributed]` branch |
| `crates/compiler-core/src/config.rs` | `[storage.backend]`, `[storage.distributed]`, `[storage.query-cache]` |
| `ekos/docs/rfcs/0111-…md` | Phase B checklist ticked as B1–B5 land |
