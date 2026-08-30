# RFC 0111 — Partitioned, Tiered, and Distributed Fact-Segment Storage

**Status:** Under Review — merges and supersedes RFC 0034 and RFC 0110 (below). 5 Open Questions
remain (1 partially). Not yet Accepted.

**Implementation note (updated 2026-08-29):** per explicit user direction, Phase A (Local mode) is
being built **incrementally against this RFC directly** — this document doubles as the Phase A
implementation RFC rather than spawning a separate one (a separate implementation RFC is still
expected for Phase B / Distributed mode). Progress is tracked in the Phase A checklist under
Acceptance Criteria (`[x]` done / `[~]` partial / `[ ]` not started), each item pointing at the
real code. Landed so far: `crates/ledger/src/partitioned.rs` (`PartitionedLedger` — all three
`PartitionDimension`s routing (`SourceScope`/`Composite` via a `with_source_resolver` closure,
since `KirObject` has no source field yet), configurable `TimeBucket`, catalog-recorded
dimension/bucket with a `DimensionMismatch` guard on reopen, `entity_id → Set<PartitionKey>`
fan-out, pruned scoped reads, concurrent multi-partition writers, a **persisted `PartitionCatalog`**
(`catalog.json`, §5), a **persisted AEVT-style run-file index** (`index/run-*.jsonl`, unified
`{k, id, p}` lines for objects, relationships, endpoints, events, evidence; `merge_runs`-style
compaction + a `rebuild_entity_index` repair path) so a reopened ledger resolves anything with
**zero partition scans**, **relationships** (routed by `"rel:"+kind`, amendment 2026-08-29),
**events + evidence + point-in-time + full-text search + `diff` + `vacuum_into`**, so
**`impl KnowledgeStore for PartitionedLedger`** — a drop-in for `FactLedger`, tested through a
`Box<dyn KnowledgeStore>` — and **cold tiering** (`Tier::Cold` via `mark_cold_before` — handle
eviction + read-triggered rehydration, RFC §3 policy layer)), `compiler-core`'s
`[storage.partition]` config parsing, and the **`open_store` wiring** (`PartitionedLedger` +
`.read_only()` served for a fresh workspace opting into `[storage.partition]`, existing workspaces
untouched). **Phase A (Local mode) is functionally complete for `entity-kind`.** Remaining
polish: `source-scope`/`composite` from `open_store` (needs a `KirObject` source field), per-scope
time-bucket overrides, and the RFC §3 search-index-drop half of cold tiering.
**Author:** EKOS team
**Created:** 2026-08-27
**Supersedes:** RFC 0034 (2026-08-07, "Partitioned, Tiered Fact-Segment Storage") and RFC 0110
(2026-08-27, "Horizontal Distribution and Distributed Search"), merged into one conformed design per
explicit user direction. Both source RFCs are marked Withdrawn/superseded and kept on disk as the
historical record of how this design was reached — nothing in them is invalidated, only unified into
one document so a reader no longer has to hold two cross-referencing files in their head to
understand one system.

---

## Motivation

`FactLedger::open(root)` (`crates/ledger/src/fact_ledger.rs:226`) composes exactly **one**
`SegmentStore`, **one** `FactIndexes`, **one** tantivy `SearchIndex`, all rooted at **one** local
directory — for the *entire* workspace, on **one** machine, in **one** process. Every fact from
every connector shares one segment stream, one set of order-preserving indexes, one full-text index.
`SegmentStore` is explicitly single-writer (`fs4`/`flock`-enforced as of RFC 0104), and its `open`
(`segment/mod.rs:157`) takes a bare `PathBuf` — there is no storage-backend abstraction, local disk
is the only option. `ekos mcp serve` (`crates/cli/src/commands/mcp.rs:110`) opens one
`KnowledgeStore` per workspace process and answers every tool call from it directly.

RFC 0016's segment/frame/compression design is sound and log-structured — the right foundation, the
same pattern the industry uses to hold terabytes (Kafka/LSM-tree-style immutable segments). What's
missing is a **partition dimension**, a **hot/cold tiering policy**, and — for workspaces that
outgrow one machine — a way to place those same partitions **across machines**, backed by durable
object storage (S3, ADLS Gen2) instead of node-local disk.

These were originally written as two RFCs: RFC 0034 (single-machine partitioning/tiering) and RFC
0110 (multi-machine distribution, built directly on top of RFC 0034's partition model — amending its
`PartitionMeta`, consuming its catalog, inheriting its correctness fixes). By the time RFC 0110 was
drafted, it depended on RFC 0034 for almost everything load-bearing, and RFC 0034 pointed forward at
RFC 0110 in its own Non-goals section. That cross-referencing was accurate but made the *actual*
architecture — one partition model, two ways of physically realizing it — harder to see than it
needs to be. **This RFC is that one conformed design**, at the user's explicit request: a single
partition model (dimension, time bucket, tiering, entity→partition-set correctness) with two
deployment modes built on it — **Local** (single-machine, default, exactly RFC 0034's original
design) and **Distributed** (opt-in, object-storage-backed, RFC 0110's three-service architecture).

## Scope

- The partition model: a configurable dimension (`SourceScope` | `EntityKind` | `Composite`), a time
  bucket (globally defaulted, per-scope overridable), hot/cold tiering, and a catalog — §1–§3.
- Correctness: `entity_id → Set<PartitionId>`, built into the base design from the start rather than
  bolted on as a later correction — §2.
- A storage-backend seam (`SegmentBackend`: `LocalFs` | `ObjectStore`) so the same partition model
  can be backed by either — §4.
- **Deployment mode: Local** (default) — one process, one machine, in-process catalog, no
  coordinator, no network — §5.
- **Deployment mode: Distributed** (opt-in) — object storage as the durable copy, a coordinator for
  metadata/leases, and three independently-scalable services (compile/ingest MPP, query/EAV-assembly
  MPP, a single logical query gateway) — §6.
- Distributed search across the pruned partition set in Distributed mode — §7.

## Non-goals

- **Implementation.** Design only, per CLAUDE.md's Mandatory Development Workflow — implementation
  RFCs (one for the Local-mode phase, one for the Distributed-mode phase, matching RFC 0080's own
  precedent of one dated implementation RFC per phase) follow once this is Accepted.
- **Distributed transactions across partitions.** Every write is scoped to exactly one partition in
  both deployment modes — no fact ever needs atomic writes to two partitions.
- **Changing the append-only invariant, the segment/frame format, or `FactIndexes`.** Reused
  byte-for-byte in both modes; only where the bytes live and who can reach them changes.
- **Retention or deletion policy.** Purely about access/compaction efficiency for data that is kept.
  RFC 0033's per-channel opt-in remains the actual lever for not ingesting unwanted volume; any
  future "delete data older than N" policy is a distinct RFC given the ledger's append-only
  invariant.
- **Replacing the SQLite backend.** Applies to the fact engine (RFC 0016) only.
- **A general-purpose cloud-storage abstraction, or multi-cloud beyond what one crate gives for
  free.** Scopes to `object_store` (Apache Arrow/DataFusion ecosystem) — S3, Azure (ADLS Gen2), GCS,
  S3-compatible (MinIO), and local disk behind one trait.

## What already exists and is reused as-is

- Frame/segment format, seal-on-8MB-threshold (`SEGMENT_SEAL_BYTES`, `segment/mod.rs:53`),
  manifest+SHA-256 verification, crash recovery (torn-tail truncation, stale-watermark catch-up) —
  `segment/mod.rs`. Unchanged; every partition, in either deployment mode, gets its own instance of
  exactly this.
- `FactIndexes`/`merge_runs` compaction, order-preserving EAVT/AEVT/AVET byte-key runs — `index.rs`.
  Unchanged in mechanism; scoped per-partition instead of globally.
- Tantivy `SearchIndex`, BM25, mmap'd reads — `search.rs`. Unchanged in mechanism.
- RFC 0104's single-writer `write.lock` — reused directly in Local mode; its *invariant* (exactly one
  writer per partition at a time) is reused in Distributed mode too, just brokered by a coordinator
  lease instead of a local `flock`.
- RFC 0033's per-channel allowlist — the primary noise-reduction lever, still the first line of
  defense before storage engineering even matters.

## Design

### 1. Partition model: configurable dimension + time bucket

```rust
pub enum PartitionDimension {
    /// e.g. "sql", "discord:#governance"
    SourceScope(String),
    /// KIR object/entity kind — e.g. Table, Module, Symbol, Custom("Risk").
    /// Useful when a workspace's query load skews toward "all Tables"
    /// rather than "everything from one connector."
    EntityKind(String),
    /// Both axes at once. Finer-grained placement; an unscoped query fans
    /// out to the product of both dimensions' cardinalities (§8).
    Composite(Box<PartitionDimension>, Box<PartitionDimension>),
}

pub struct PartitionMeta {
    pub id: PartitionId,
    pub dimension: PartitionDimension,
    pub wall_time_range: (i64, i64),
    pub tx_range: (TxId, TxId),
    pub tier: Tier,                    // Hot | Cold
    pub location: PartitionLocation,   // §4/§5/§6 — Local(PathBuf) | Distributed(ObjectStoreUrl)
}

pub struct PartitionCatalog {
    pub partitions: Vec<PartitionMeta>,
}
```

Time bucket defaults globally, with per-scope overrides — reusing this codebase's existing
glob-override config shape (`[[recover.sql.dialect-rules]]`, `compiler-core/src/config.rs`) rather
than inventing a second one:

```toml
[storage.partition]
dimension = "source-scope"   # "source-scope" | "entity-kind" | "composite"
time-bucket = "monthly"      # global default

[[storage.partition.time-bucket-overrides]]
scope-glob = "discord:*"
time-bucket = "daily"        # a firehose source opts into finer buckets
```

**Writes** route to the partition matching a fact's dimension value + *current* time bucket,
opening (or creating) that partition's `FactLedger` on demand — the fix for the single-writer
bottleneck, since N partitions admit N concurrent writers instead of one global `SegmentStore`.
**Broad reads** (`ekos_search`, `ekos_ekl` range/full-text queries) fan out only to partitions whose
dimension value/time range could match, pruned by the catalog — the same pruning principle
`batches_after` already applies at the segment level, lifted one level up to whole partitions.

### 2. Correctness: `entity_id → Set<PartitionId>`, not `→ PartitionId`

Because writes route by *current* time bucket, later versions of a long-lived entity can land in a
*different* partition than earlier versions once a time-bucket boundary is crossed — so the
entity→partition lookup must be a **set**, not a single id, from the start:

- **Point reads** (`get_object`, current state): resolve to exactly one partition — the entity's
  *most recent* one, since current state always lives in the newest partition. No fan-out, in either
  deployment mode.
- **Full-history reads** (`object_history`, or `object_at`/`relationships_at` for a timestamp that
  could predate the entity's most recent time-bucket partition): fan out to every partition in the
  entity's set. In Local mode this is N in-process `FactLedger` reads; in Distributed mode (§6) it's
  N RPC calls to whichever Service B workers hold those partitions.

The set is naturally bounded, not a runaway structure: it grows by one entry only when an entity's
writes cross into a *new* time-bucket partition, not per version — most versions of an
actively-mutated entity land in the same (current) partition between boundaries. Set size is bounded
by `workspace_age ÷ time_bucket_granularity`: even a 10-year-old, monthly-partitioned, continuously
mutated entity accumulates at most ~120 entries. No cap or compaction policy is needed.

### 3. Hot/cold tiering

A partition's tier is a property of the catalog entry, not a different storage format:

- **Hot**: full `FactLedger` (segments + indexes + tantivy search), mmap'd.
- **Cold**: a sealed partition past a configurable age (e.g. 90 days with no new writes) is
  recompressed — reusing the existing `BODY_ZSTD_LEVEL = 19` dict-zstd mechanism (RFC 0016 §7) via
  an additive cold-tier dictionary generation (`dict_version`/`SegDict`), not a new compression
  lever — its tantivy `SearchIndex` is dropped (rebuildable on demand, `search.rs`'s existing
  rebuild-from-scratch capability), and the partition directory becomes eligible to move to cheaper
  backing storage.
- **Promotion back to hot** happens automatically on any read that touches a cold partition (lazy
  rehydration) — no separate "unfreeze" operation.

In Local mode, "cheaper backing storage" for a cold partition stays local-disk (v1, simplest — no
cloud dependency for a single-machine deployment). In Distributed mode, *every* tier already lives in
object storage (§4) — cold partitions there simply mean "not cached by any Service B worker right
now," not a separate storage location; tiering there is a caching policy, not a placement one.

### 4. Storage backend seam

```rust
/// Follows this project's existing dependency-injection convention
/// (Observer, LlmProvider, CompilerPass are all traits selected by config).
trait SegmentBackend {
    fn put(&self, path: &str, bytes: Bytes) -> Result<(), SegmentError>;   // sealed objects only
    fn get(&self, path: &str) -> Result<Bytes, SegmentError>;
    fn list(&self, prefix: &str) -> Result<Vec<String>, SegmentError>;
}
```

`LocalFsBackend` wraps today's unmodified `std::fs` calls. `ObjectStoreBackend` is new, built on the
`object_store` crate's `ObjectStore` trait (`AmazonS3` / `MicrosoftAzure` / S3-compatible / local —
one dependency covers both providers named, plus a free local/dev-mode option). Layout mirrors
today's local directory 1:1 either way: `<root>/<partition-id>/segments/seg-<seq>.bin`,
`.../indexes/<order>/run-*.bin`, `.../search/*`.

- **Sealed segments map cleanly onto object storage**: immutable once sealed — one `PUT`, many
  `GET`s. No read-modify-write, ever, for sealed data.
- **The active (unsealed) segment does not** — object stores have no append operation. It stays
  buffered on whichever writer currently holds the partition (§5's local process, or §6's
  lease-holding Service A worker), durable in object storage only once it seals at
  `SEGMENT_SEAL_BYTES` (8 MB, unchanged) and uploads as one immutable object. **Stated precisely:** a
  writer crash in Distributed mode loses at most that one partition's current unsealed segment,
  bounded at 8 MB — everything before it is already sealed and durable. (Local mode has no analogous
  loss window: a crash there is recovered the same way `SegmentStore` has always recovered, via
  torn-tail truncation on next open.)
- **Manifest commits need atomicity object stores don't natively give.** Rather than depend on a
  provider-specific primitive (S3 needs an external lock table for compare-and-swap; ADLS Gen2 has
  native blob leases but that's Azure-only), Distributed mode's coordinator (§6) — which already
  grants write leases — also arbitrates manifest commits: one mechanism, portable across both named
  providers, needed only in Distributed mode (Local mode's manifest commit is the existing
  write-temp→fsync→rename `SegmentStore` already does, unchanged).

### 5. Deployment mode: Local (single-machine, default)

Exactly RFC 0034's original design, unchanged in mechanism:

```rust
pub struct PartitionedLedger {
    catalog: PartitionCatalog,               // in-process struct, no network
    open: HashMap<PartitionId, FactLedger>,  // lazily opened, LocalFsBackend
}
```

One process, one machine. The catalog is a plain in-process struct — no coordinator, no RPC, no
leases: RFC 0104's local `write.lock` already gives one-writer-per-partition within a process, and a
single process opening `PartitionedLedger` is the only writer by construction. `PartitionLocation`
for every partition is `Local(PathBuf)`. This is the default for every workspace; nothing above this
mode requires any of §6's machinery to exist or run.

### 6. Deployment mode: Distributed (opt-in, multi-machine)

Object storage (§4) becomes the ledger's single durable copy; three independently-scalable services
replace the single in-process `PartitionedLedger`:

**Coordinator** — holds the partition catalog (§1, unchanged in shape), who currently holds a
partition's write lease (short-lived, renewable, fencing-tokened — §9), and the tx watermark per
partition. Same shape as a Delta Lake/Iceberg transaction log or a Hive Metastore — a small,
centralized metadata service in front of object storage holding the real data. **v1 is a single
coordinator process, an acknowledged SPOF** (§9); Raft-replicated metadata is a named v2 question,
not attempted here ahead of real evidence it's needed.

**Service A — Compile/Ingest MPP** (`ekos compile-worker serve --coordinator <addr>`). Maps to
today's `build/recover/resolve/compile/commit` pipeline, made horizontally distributed. Work unit =
one partition-scoped shard `(dimension_value, time_bucket)`. A worker leases a shard from the
coordinator, becomes its sole writer, runs the **existing, unmodified** recovery/semantic-compile
passes, appends to a locally buffered active segment, seals+uploads at threshold, commits the
manifest through the coordinator. N workers on N shards write fully in parallel.

**Service B — Query/EAV-Assembly MPP** (`ekos query-worker serve --coordinator <addr>`). Stateless
compute, no durable local state. On assignment to a partition, pulls its sealed segments + index runs
+ tantivy index from object storage into a bounded local cache (mmap'd once downloaded — RFC 0016
Phase 6's mmap reads apply unchanged to the cached copy), then runs the **existing, unmodified**
`FactIndexes` EAVT/AEVT/AVET fold and tantivy search locally. Because sealed segments are immutable
and object storage is the one durable copy, **any** query worker can serve **any** partition — no
owned/replica-set concept. Losing a worker loses only its warm cache; the coordinator reassigns.

**Service C — Query Gateway** (single logical, load-balanced). Maps to today's `ekos mcp serve`/
`ekos ask`/`Runtime`. Stateless: any number of interchangeable replicas, since it holds no partition
data itself, only routing/merge logic. Resolves the pruned partition set via the coordinator's
catalog, dispatches parallel sub-queries to whichever Service B workers hold those partitions, merges
results (point reads pass through; full-history reads merge in tx order per §2; full-text search
merges per-partition BM25 top-K per §7). `KnowledgeStore` stays the seam: Service C's core is a
`DistributedLedger` implementing `KnowledgeStore`, its RPC client talking to Service B workers — no
caller of `KnowledgeStore` (Runtime, MCP tool handlers, `docs-gen`) changes at all.

**Failure handling**: because Service A/B workers are stateless compute over shared durable storage,
failover is simple — a crashed compile worker's lease expires, another worker re-leases the shard and
resumes from the last committed manifest (loss bounded at 8 MB per §4); a crashed query worker is
just reassigned, since it held no durable state.

### 7. Distributed search

Only meaningful in Distributed mode (Local mode's single process already has one tantivy index per
hot partition, queried directly, no fan-out design needed). Service C fans out
`search(partition, query, limit)` to the Service B workers holding the pruned partition set, merges
top-K by each worker's local BM25 score. **Stated plainly:** per-partition BM25 uses each partition's
own local term statistics, so the merged ranking is the same well-known "query-then-fetch"
approximation every distributed search engine makes (Elasticsearch's default behavior included) —
not a mathematically global ranking. Accepted for v1; global term-statistics aggregation is a
possible follow-up, not designed here.

### 8. What the merge actually simplifies

Concretely, not just organizationally:

- **No more cross-RFC acceptance dependency.** RFC 0110 could not be Accepted until RFC 0034 shipped
  and fixed its own `entity_id → Set<PartitionId>` gap; RFC 0034 pointed forward at an RFC that
  depended back on it. As one document, that circularity dissolves into an ordinary two-phase
  Acceptance Criteria (§11) within a single RFC — Local mode first, Distributed mode second, both
  gated by the same review, not two.
- **The correctness fix is base design, not a correction.** §2 is written as the *real* model from
  the start; a new reader never encounters the wrong "one partition per entity" claim before being
  told it's wrong, the way the original RFC 0034 → amendment sequence required.
- **One partition model, read once.** `PartitionDimension`, the catalog shape, and tiering are
  described exactly once (§1–§3) and reused by reference from both deployment-mode sections (§5, §6),
  instead of being defined in RFC 0034 and re-explained/amended from RFC 0110.

## Alternatives Considered

- **Change the segment/frame format itself to carry a partition key inline, one global segment
  stream.** Rejected: reintroduces the "everything shares one manifest/one active segment/one writer"
  bottleneck this design exists to remove — partitioning must happen *above* `SegmentStore`.
- **Partition by source only, no time dimension** (and the reverse: time only, no source dimension).
  Both rejected: source-only reintroduces unbounded per-source growth (no tiering boundary); time-only
  can't skip irrelevant sources within a matching time window, defeating RFC 0033's per-channel
  scoping at the storage layer.
- **Consistent-hash sharding over entity id** instead of dimension-keyed partitions. Rejected:
  destroys the scoped-pruning property this design exists for — a hash has no relationship to query
  scope.
- **Node-owned local disks with app-level replication** for Distributed mode (RFC 0110's original,
  pre-revision draft). Rejected: duplicates durability object storage already provides (both S3 and
  ADLS Gen2 give multi-AZ durability natively), and needs a coordinator to track replica sets —
  strictly more moving parts for the same guarantee as reading from one shared durable store.
- **Provider-native locking** (S3 via an external conditional-write lock table; ADLS Gen2 via native
  blob leases) instead of coordinator-brokered leases. Rejected for v1: two separate implementations
  to support both named providers, versus one coordinator-brokered, storage-provider-agnostic
  mechanism, given a coordinator is already needed for the catalog.
- **A hand-rolled S3/Azure SDK integration** instead of `object_store`. Rejected: one crate already
  implements one trait for both named providers plus MinIO (local dev) and local disk — one
  dependency instead of two bespoke SDK integrations, already backing a widely-used ecosystem
  (DataFusion, delta-rs).
- **Raft-replicated coordinator metadata from day one.** Rejected for v1 as engineering weight not
  yet justified by evidence — this project's own storage roadmap (RFC 0080) escalates scope only
  after a real, physical incident demands it (e.g. Phase 1's concurrency fix followed real corruption
  found in `devlog_65`, not a hypothetical). Kept as the named v2 path, not silently dropped.

## Architecture Review (2026-08-27)

Carried forward from both source RFCs' own reviews, reconfirmed against the merged whole — no new
inconsistency introduced by unifying them.

**Validated against `ekos.md` and CLAUDE.md, both deployment modes:** storage-technology
independence (`ekos.md` §"Technology Independent" — `SegmentBackend`'s `LocalFs`/`ObjectStore` split
is exactly the substitution this principle anticipates); single source of semantic truth (`ekos.md`
§5 — in Distributed mode, object storage is the one durable copy every Service B worker reads from,
never a worker's local cache); append-only and evidence-traceable (unchanged — this design moves
*where* bytes live, never mutates them); Runtime read-only (`DistributedLedger`/`PartitionedLedger`
both implement exactly `KnowledgeStore`'s existing contract, no new mutation surface); deterministic,
side-effect-free compiler passes (Service A runs the existing, unmodified passes; Local mode doesn't
touch them at all); dependency injection through traits (`SegmentBackend` follows the same pattern as
`Observer`/`LlmProvider`/`CompilerPass`).

**Resolved** (concrete decisions, not left open):

- Time-bucket granularity — global default + per-scope glob overrides (§1), reusing an existing
  config pattern rather than inventing a second one.
- Entity→partition lookup index needing its own partitioning at scale — no, for realistic scale: it's
  the same AEVT-style run-file index technology already serving the live estate's ~88K entries, two
  orders of magnitude below where this would matter; `merge_runs` already bounds growth if it ever
  does.
- `entity_id → Set<PartitionId>` cap/compaction — not needed; naturally bounded by wall-clock time
  (§2), not write frequency.
- Object-storage backing for cold partitions — resolved by this merge itself: Local mode's cold tier
  stays local-disk (§3); Distributed mode already puts every tier in object storage (§4) — the
  question "should cold partitions get cloud backing" is answered by which deployment mode is chosen,
  not left as a separate v2 maybe.
- Cold-tier recompression level — reuse the existing `BODY_ZSTD_LEVEL = 19`, no new lever.
- Sync vs. async transport (Distributed mode only) — async (tokio) at the coordinator/RPC boundary
  only; Service A workers still run the existing sync compiler passes internally via
  `spawn_blocking`. RFC 0001's sync-pipeline decision is preserved untouched; this is a new edge
  alongside it, not a retrofit through it.
- Write-lease timing (Distributed mode) — short TTL (e.g. 30s) with heartbeat renewal (e.g. 10s); on
  expiry, no attempt to recover a dead worker's local unsealed buffer — the next lease-holder starts
  fresh from the last committed manifest, accepting the already-bounded ≤8MB loss rather than
  promising unreachable-machine recovery.
- Manifest-commit contention (Distributed mode) — fencing tokens: each lease grant carries a
  monotonically increasing token; the coordinator rejects a stale-token commit rather than silently
  applying a race.
- Transport security (Distributed mode) — mutual TLS over a cluster-internal CA (v1 default);
  rotation mechanics deferred to the implementation RFC.
- Default deployment mode — Local, indefinitely, consistent with RFC 0016's own opt-in-through-soak
  precedent; `Distributed` is explicitly opt-in for workspaces that need it.

**Partially resolved:**

- `PartitionDimension::Composite` fan-out cost — the *shape* is settled by reasoning: `Composite`
  partitions by the product of both dimensions' cardinalities, so a *scoped* query is unaffected
  (still one partition) while an *unscoped* query fans out to N×M instead of N or M. The concrete
  N×M threshold at which that matters in practice is not resolved — needs a real multi-dimension
  fixture.

**Deliberately deferred, not avoided:**

- Coordinator consensus (Distributed mode): single (v1, named SPOF) vs. Raft-replicated (v2) — a
  real, named scope decision (Alternatives Considered), revisited only if the v1 SPOF causes a real
  incident, matching this project's bias against speculative engineering ahead of evidence.

## Open Questions

- [ ] **Cold-tier rehydration cost budget** (Local mode) — needs real measurement against a
      realistic multi-partition fixture, not assumed.
- [ ] **`PartitionDimension::Composite`'s concrete fan-out threshold** (both modes) — see "partially
      resolved," above.
- [ ] **Service B cache eviction policy** (Distributed mode; LRU by partition, size-bounded) — needs
      real query-pattern data before a concrete policy is chosen.
- [ ] **Shrinking the bounded unsealed-segment loss window** below 8 MB (Distributed mode; periodic
      partial upload vs. a local WAL survived by lease handoff) — a real durability/complexity
      trade-off needing its own comparison.
- [ ] Coordinator consensus v1→v2 timing (Distributed mode) — deliberately deferred, see above; not
      a blocker for Accepting this RFC.

## Acceptance Criteria

Two phases, gated by the same review, sequenced within this one RFC rather than across two:

**Phase A — Local mode** (matches RFC 0034's original scope):

- [x] `PartitionedLedger` (§5) routes point reads to a single partition and broad reads to a pruned
      partition set. — *point reads: `get_object` → the entity's newest partition only. Pruned broad
      reads: `objects_in_kind` touches only matching-`dimension_value` partitions, resolved from the
      **persisted `PartitionCatalog`** (`<catalog_root>/catalog.json`), so a fresh process sees
      every partition without a prior write (`catalog_and_entities_survive_a_reopen`). Broad reads
      dedup cross-partition entities to current state. Time-range pruning *within* the matched
      dimension set is a later refinement, not a gap in the criterion as written.
      `crates/ledger/src/partitioned.rs`.*
- [x] `entity_id → Set<PartitionId>` (§2) implemented; full-history reads for an entity spanning ≥2
      time-bucket partitions return complete, correctly ordered history via fan-out, while
      `get_object` still resolves to a single partition. — *tested by
      `entity_spanning_two_time_buckets_…` and, across a reopen with an assertion that **no scan
      happens** (only the entity's own partitions are opened), by
      `catalog_and_entities_survive_a_reopen`. The map is now backed by a **persisted AEVT-style
      run-file index** (`entity-index/run-*.jsonl`, RFC Architecture Review): append-only pair
      lines, `merge_runs`-style compaction at `COMPACT_AT` runs (`entity_index_runs_compact_on_open`),
      a self-healing catalog scan only for an entity absent from the index, and
      `rebuild_entity_index()` as the `ekos ledger repair`-style full re-derive
      (`rebuild_entity_index_repairs_a_dropped_pair_line`).*
- [~] `PartitionDimension` (`SourceScope` | `EntityKind` | `Composite`) implemented and configurable
      via `ekos.toml`'s `[storage.partition]`, including time-bucket overrides. — *All three
      dimensions **route**: `EntityKind` off `ObjectKind`; `SourceScope`/`Composite` off a
      caller-supplied `with_source_resolver` closure (a `None` under a source dimension is a hard
      `UnresolvedSource` error, never a misroute — `KirObject` has no source field yet, so the
      closure is the seam). `Composite` value is `"<source>\u{1f}<kind>"`. Tests:
      `source_scope_routes_by_resolver_not_kind`, `composite_partitions_by_source_and_kind`,
      `source_scope_without_a_resolved_source_errors`. `TimeBucket::{Daily,Weekly,Monthly}` +
      `PartitionDimension` both `parse`/`as_str`; the catalog records both and a reopen with a
      changed value is a `DimensionMismatch` error (`reopening_with_a_changed_dimension_or_bucket_errors`).
      Still to do: per-scope glob time-bucket overrides, and the config→`PartitionedLedger`
      wiring (needs `PartitionedLedger` to be a `KnowledgeStore` first).*
- [~] Cold-tier round-trip passes with byte-identical read results. — *`aged_partitions_go_cold_evict_handles_and_rehydrate`:
      `mark_cold_before(cutoff)` demotes past-bucket partitions to `Tier::Cold` (catalog-persisted,
      survives reopen), evicts their open handles, and any read promotes one back to hot returning
      byte-identical data. Still `[~]` because "cold" here is a **policy flag + handle eviction +
      relocate-eligible marker**, not yet the §3 search-index drop + zstd recompression — those
      need `FactLedger` support and land with the `KnowledgeStore`/`SegmentBackend` work. The
      `SegmentBackend` seam (§4, object storage) is Phase B.*
- [x] Concurrent-writer test passes across ≥2 partitions. — *`concurrent_writers_across_two_partitions`:
      two threads append to two entity-kind partitions in parallel (each partition an
      `Arc<FactLedger>`, own lock), all writes land correctly routed.*

**Phase B — Distributed mode** (matches RFC 0110's scope; depends on Phase A shipping first). Its
dated implementation RFC is **RFC 0113** (Draft, 2026-08-29), which sequences §4/§6/§7 into
sub-phases B1–B5 and pins the interface-level decisions this section left at design altitude.
**All of B1–B5 landed 2026-08-29/30** — Distributed mode is feature-complete at v1 scope; the
remaining work is v1 → v1.1 polish (RFC 0113 Open Questions):

- [x] **B1** (2026-08-29) — `SegmentBackend` (§4) + `LocalFsBackend`
      (`crates/ledger/src/backend.rs`); `SegmentStore`'s sealed-object publish/fetch routes through
      it, `LocalFsBackend` is the untouched-behaviour default. RFC 0113.
- [x] **B2** (2026-08-29) — `crates/segment-backend` crate; `ObjectStoreBackend` (`object_store`
      0.14, `object-store` feature) + `MemBackend`; `SegmentStore` round-trips on object storage.
      RFC 0113.
- [x] **B3** (2026-08-29) — `crates/cluster` (`ekos-cluster`): `Coordinator` (catalog, leases,
      fencing tokens, watermarks, entity index; single-JSON-file state) + `serve` (newline-delimited
      JSON-RPC over TCP, the `ekos mcp serve` pattern — not tonic) + `CoordinatorClient` +
      `CompileWorker`/`LeaseGuard` (Service A transport+lifecycle). `ekos coordinator serve`/`status`
      + `ekos compile-worker run`. Multi-service harness: lease contention, expired-lease fencing +
      watermark resume, restart durability. RFC 0113.
- [x] **Service A real pipeline** (2026-08-30) — `ekos compile-worker run` executes the real
      `build → recover → resolve → compile → commit` under a heartbeated, fencing-tokened lease
      (on a blocking thread with its own runtime, so heartbeats keep flowing), then registers every
      partition it wrote and commits the store's entry count as the generation watermark. RFC 0113.
- [x] **Partition self-describing in object storage** (2026-08-30) —
      `FactLedger::open_with_backend` + `PartitionedLedger::with_segment_backend(resolver)`;
      `[storage.partition] segment-backend-url = "s3://…"` routes each partition's sealed segments
      **and** `manifest.json` / `dict.bin` through the `SegmentBackend` (new
      `SegmentBackend::publish`). Only `HEAD` + the active/unsealed segment stay local (writer-only
      crash-recovery state) plus tantivy's `search/` (query worker rebuilds/skips — a follow-on).
      RFC 0113.
- [x] **B4** (2026-08-30) — `crates/distributed` (`ekos-distributed`): `QueryWorker` (Service B) —
      materialises a partition on demand (object storage → bounded local cache, or a co-located
      local dir), opens it read-only, serves every `KnowledgeStore` read for it over NDJSON/TCP;
      `ekos query-worker serve`. `DistributedLedger` (Service C) — `impl KnowledgeStore`, fans
      every read across the workers named by the coordinator catalog and merges; `append_*`
      rejected; `open_store` `[storage.distributed]` branch. Both proven against a real
      `PartitionedLedger` (query-worker reads == direct reads; gateway == in-process
      `PartitionedLedger` over 2 workers). v1 follow-ons (connection pool, parallel fan-out,
      coordinator-index pruning, Local→Distributed registration): RFC 0113. RFC 0113.
- [x] **B5** (2026-08-30) — `FactLedger::find_objects_scored` (BM25 score exposed);
      `DistributedLedger::search(query, k)` fans each object partition's local top-`k`, merge-sorts
      by shard-local score, dedups, truncates; `find_objects` rides on it. Cross-shard IDF is the
      accepted query-then-fetch approximation (§7 — a global term-statistics pass is out of scope).
      RFC 0113.

**Both phases:**

- [x] At least one review completed — Architecture Review (2026-08-27), above.
- [ ] Remaining Open Questions resolved or explicitly re-scoped with the user's sign-off (coordinator
      consensus timing does **not** block acceptance — deliberately scoped v1 decision, not an
      unresolved question).
- [x] Design is consistent with `ekos.md`'s compiler architecture and CLAUDE.md's key invariants —
      confirmed by the Architecture Review above.
- [~] A dated implementation RFC per phase is written before any code (matching RFC 0080's
      precedent), per the Mandatory Development Workflow. — *Phase A: this RFC doubles as it
      (Implementation note above). Phase B: **RFC 0113** (Draft) — Accept it before any B-phase
      code.*

## Amendment (2026-08-29): Phase A — relationships, events, evidence, and the full `KnowledgeStore` surface

The Phase A slice so far (`crates/ledger/src/partitioned.rs`) covers **objects only**. To become a
drop-in `KnowledgeStore` (so `open_store` can serve it and every `Runtime`/MCP/`docs-gen` caller
works unchanged), it needs relationships, events, evidence, point-in-time reads, search, `diff`,
and `vacuum_into`. Object routing (§1) doesn't answer how a *relationship* — which links two
entities that may live in different partitions — routes to **one** partition (§Non-goals: no fact
ever needs an atomic two-partition write). This amendment settles that, per the same
build-incrementally-against-this-RFC direction.

### 1. Relationship routing — by `RelationshipKind`, independent of the object dimension

A relationship routes to `PartitionKey { time_bucket: <bucket of created_at>, dimension_value:
"rel:" + <RelationshipKind> }`. Rationale:

- Relationships have no clean "source" and their `from`/`to` may sit in different partitions —
  neither endpoint is a natural home, and co-locating with one endpoint makes the *other*
  direction's lookup a full fan-out.
- `RelationshipKind` **is** the query axis in practice — impact/neighborhood analysis routinely
  scopes by kind (`DependsOn`, `Calls`, `Extends`).
- The `"rel:"` prefix keeps relationship partitions disjoint from object partitions in the one
  shared catalog, so `objects_in_kind("Table")` never touches a relationship partition and vice
  versa. Under a `SourceScope`/`Composite` *object* dimension, relationships still route by
  `"rel:"+kind` — a deliberate, documented asymmetry, not a bug.
- Each relationship partition is self-contained: it holds the relationship facts **and** the
  `KirEvidence` they cite.

### 2. The unified run-file index (`<catalog_root>/index/run-*.jsonl`)

The `entity-index/` of §5's amendment generalizes to one `index/` subsystem. Each line is
`{ "k": <kind>, "id": <uuid>, "p": <partition-key> }` with `k` one of:

| `k` | meaning | serves |
|---|---|---|
| `obj` (default) | object entity id → an object partition it has a version in | `get_object`, `object_history`, entity fan-out (unchanged) |
| `rel` | relationship id → the relationship partition it lives in | `get_relationship`, `relationship_history` |
| `endpoint` | an endpoint entity id (`from` **or** `to`) → a relationship partition it participates in | **`relationships_for(X)`** — pruned to X's relationship partitions, never a full fan-out |

One `append_relationship` appends three lines (`rel` for the id, `endpoint` for `from`, `endpoint`
for `to`). Load, `merge_runs`-style compaction, torn-tail tolerance, and `rebuild_entity_index`
(now also re-deriving `rel`/`endpoint` from the relationship partitions) all work exactly as for
`obj`. Back-compatible: `k` defaults to `obj`, so pre-amendment run files still load.

### 3. Everything else

- **Events** (`KirEvent`) route by the event's own `created_at` bucket + `"evt:"+<kind-or-fixed>`;
  a dedicated `evt`-tagged index entry keyed by the event's subject id serves subject lookups.
- **Evidence** (`KirEvidence`) is content-addressed and co-located with the fact that cites it
  (§1); a bare `append_evidence` with no citing fact yet routes to `"evidence:"+bucket`.
- **`object_at` / `all_objects_at` / `relationships_at` / `all_relationships_at`** — fan out to the
  relevant partition set (an entity's sites, or all partitions for the `all_*` forms), delegate to
  each `FactLedger`'s own `*_at`, merge keeping the newest-partition row per id.
- **`find_objects(query)`** — fan out to each **hot** object partition's tantivy index, merge
  per-partition top-K by local BM25 (§7's "query-then-fetch" approximation, Local-mode flavour;
  cold partitions are skipped, documented — a query needing them rehydrates first).
- **`diff(from, to)`** — per-partition `diff`, merged into one `LedgerDiff`.
- **`vacuum_into(dest)`** — recursively copy the whole `catalog_root` tree (catalog + index + every
  partition dir).
- **`entry_count`** — sum across partitions.

### 4. Acceptance (folds into the Phase A checklist above)

- [x] `append_relationship`/`get_relationship`/`all_relationships`/`relationship_count`/
  `relationship_history` route by `"rel:"+kind`; `relationships_for(X)` prunes via the `endpoint`
  index. — *done. The `entity-index/` generalised to one unified `index/run-*.jsonl`
  (`{k, id, p}`, `k` ∈ obj/rel/endpoint); `rebuild_entity_index` re-derives all three; tests
  `relationships_route_by_kind_and_relationships_for_is_pruned`,
  `rebuild_also_repairs_the_relationship_index`.*
- [x] Events, evidence, `object_at`/`all_objects_at`/`relationships_at`/`all_relationships_at`,
  `find_objects`, `diff`, `vacuum_into`, `entry_count` per §3. — *done. Events/evidence route to
  `"events"`/`"evidence"` partitions with `evt`/`evid` index kinds (self-healing only; `FactLedger`
  can't enumerate them for `rebuild`). `find_objects` fans out hot object partitions, merges
  per-partition BM25, skips cold. `diff` merges per-partition `LedgerDiff`s. `vacuum_into` writes a
  self-contained copy (rewritten `catalog.json` + `index/` + each partition under `dest/p/<key>/`).*
- [x] `impl KnowledgeStore for PartitionedLedger` (via `From<PartitionError> for LedgerError`,
  tested through a `Box<dyn KnowledgeStore>`); **`open_store` / `open_store_read_only` build it**
  when `[storage.partition]` is enabled on a genuinely fresh workspace (or one that already has
  `partitioned/catalog.json`) — an existing SQLite/fact workspace is never implicitly switched,
  same rule as the fact-engine default. `PartitionedLedger::read_only()` opens each partition via
  `FactLedger::open_read_only` (RFC 0097). Only `entity-kind` wires from config
  (`source-scope`/`composite` need a source resolver `open_store` can't provide yet — clear
  error). Tests: `partitioned_workspace_round_trips_through_open_store`,
  `existing_fact_workspace_is_not_switched_to_partitioned` (`crates/cli/src/commands/store.rs`).

## Testing

**Phase A (Local mode):**

- Fixture with 3+ partitions (mixed hot/cold, mixed dimension values), asserting: a point read
  routes to exactly one partition (no fan-out), a scoped broad query touches only matching
  partitions, an unscoped query correctly fans out to all.
- Entity-spanning-partitions fixture: one entity mutated across ≥2 time-bucket boundaries, asserting
  `get_object` still routes to exactly one partition while `object_history`/`object_at` correctly
  fans out and returns complete, correctly ordered history.
- Configurable-dimension fixture: the same partitioning/pruning tests, run once with
  `dimension = "source-scope"` and once with `"entity-kind"`.
- Cold-tier round-trip: seal a partition, mark it cold, read from it — assert correct rehydration,
  byte-identical to before going cold.
- Concurrent-writer test: two partitions accept concurrent appends without contention, verified via
  the existing crash-recovery/torn-tail tests, run per-partition.
- Compaction-cost test: assert `merge_runs` cost on an N-partition estate scales with one partition's
  size, not total estate size.

**Phase B (Distributed mode):**

- Multi-service local harness: one coordinator, N compile workers, M query workers, one gateway
  replica set, against a local S3-compatible test double (`object_store`'s in-memory backend, or
  MinIO in a container) — no real cloud dependency needed.
- Lease contention test: two compile workers request the same shard, exactly one gets it, the loser
  gets a clear "already leased" error.
- Bounded-loss-on-crash test: kill a compile worker mid-active-segment, assert loss ≤ 8 MB and every
  previously sealed segment is intact and durable.
- Cache-miss-then-hit test: a query worker re-hydrates correctly from object storage on first
  assignment, hits its warm cache on the next request.
- Manifest-commit-race test: exercise the lease-expiry-during-upload race, assert no corrupted or
  lost manifest.
- Distributed search merge test: fixture with matching documents split across ≥2 partitions on
  different query workers, assert the merged top-K is correctly ranked per-shard, exercising the
  cross-shard BM25 caveat.
- Entity-spanning-partitions test (builds on Phase A's fixture): full-history read for an entity
  crossing ≥2 time-bucket partitions correctly fans out to every Service B worker holding a relevant
  partition.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0111-partitioned-tiered-and-distributed-storage.md` | This RFC — merges and supersedes RFC 0034 and RFC 0110 |
| `ekos/docs/rfcs/0034-partitioned-tiered-storage.md` | Marked Withdrawn — superseded by RFC 0111 |
| `ekos/docs/rfcs/0110-horizontal-distribution-and-distributed-search.md` | Marked Withdrawn — superseded by RFC 0111 |
