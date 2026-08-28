# RFC 0110 — Storage Architecture Phase 6: Horizontal Distribution and Distributed Search

**Status:** Withdrawn — superseded by **RFC 0111** (2026-08-27), which merges this RFC and RFC 0034
into one conformed partitioned/tiered/distributed storage design, per explicit user direction. Kept
on disk as the historical record of how the storage/compute-separated, three-service architecture
was reached (§7 below records the revision from node-owned-disk replication to object storage as
ground truth — carried forward into RFC 0111 unchanged in substance). Do not implement against this
file — read RFC 0111 instead.
**Author:** EKOS team
**Created:** 2026-08-27
**Revised:** 2026-08-27 (same day) — replaced the original node-owned-disk +
segment-shipping-replication design with a storage/compute-separated, three-service architecture
per explicit user direction (§7 records what changed and why).

---

## Motivation

RFC 0080 named "Phase 6 — horizontal distribution" as the last item on the storage roadmap and
left it explicitly unscheduled pending RFC 0034 or an explicit re-scope. This RFC is that re-scope,
authored as a **design-only** document at the user's direction — no code ships here.

Today, EKOS knowledge storage is single-process and single-machine in every real sense:

- `FactLedger::open` (`crates/ledger/src/fact_ledger.rs:226`) composes exactly one
  `SegmentStore`, one `FactIndexes`, one tantivy `SearchIndex`, all rooted at one local directory.
- `SegmentStore::open` (`crates/ledger/src/segment/mod.rs:157`) takes a `PathBuf` and calls
  `std::fs::create_dir_all` directly — there is no storage-backend abstraction today, local disk is
  the only option.
- `SegmentStore` enforces single-writer via a real, designed cross-process `write.lock`
  (`fs4`/`flock`, RFC 0104 Phase 1) — a filesystem-local mechanism with no cross-machine meaning.
- `ekos mcp serve` (`crates/cli/src/commands/mcp.rs:110`) opens one `KnowledgeStore` per workspace
  process and answers every tool call from it directly; there is no routing layer of any kind.

RFC 0034 (Under Review, not implemented) designs the layer immediately below this one: `PartitionedLedger`,
routing reads/writes across multiple `FactLedger` instances keyed by `(source_scope, time_bucket)`,
explicitly scoped to one machine. This RFC treats RFC 0034's partition as the unit of placement
across a distributed system — it does not redesign the segment/index/tantivy formats (RFC 0016),
which are reused byte-for-byte, only where they physically live and how they're reached.

**This revision's core decision, specified directly by the user:** rather than EKOS-owned nodes
replicating segment bytes to each other, object storage (S3, ADLS Gen2) becomes the ledger's single
durable copy, and three independently-scalable, mostly-stateless services are built around it —
mapping directly onto this project's existing "Enterprise Systems → Observation Layer → Knowledge
Compiler → CKM/Ledger → Runtime (read-only) → AI/Apps" pipeline (CLAUDE.md), just each stage made
horizontally distributed instead of single-process.

## Scope

- **Configurable partition dimension** (unchanged from the first draft, still amends RFC 0034) —
  §1.
- **Object storage as the ledger's durable backing store** (S3, ADLS Gen2, and free of it, any
  S3-compatible store) — §2.
- **Service A** — a distributed MPP cluster that scans enterprise sources and compiles them into
  the object-storage-backed ledger — §3.
- **Service B** — a distributed MPP cluster that scans the compiled, object-storage-resident data,
  resolves entities by attribute, and reconstructs EAV chains (`FactIndexes`' existing
  EAVT/AEVT/AVET fold) — §4.
- **Service C** — a single logical, horizontally load-balanced query gateway that turns a query into
  search requests against Service B and compiles the result into an answer — §5.
- A coordinator holding partition metadata and write leases (no node-placement/replica-set state —
  §6, and why that's simpler than the first draft's design, §7).

## Non-goals

- **Implementation.** Design only, per CLAUDE.md's Mandatory Development Workflow — an
  implementation RFC follows once this is Accepted.
- **Distributed transactions across partitions.** Every write is already scoped to exactly one
  partition (unchanged from the first draft) — no fact ever needs atomic writes to two partitions.
- **Changing the append-only invariant, the segment/frame format, or `FactIndexes`.** The bytes
  produced are identical to today's; only where they're stored and how they're reached changes.
- **Replacing the SQLite backend.** Distribution applies to the fact engine only.
- **A general-purpose cloud-storage abstraction.** This RFC scopes to what one existing crate
  (`object_store`, from the Apache Arrow/DataFusion ecosystem) already provides — S3, Azure (ADLS
  Gen2), GCS, S3-compatible (MinIO), and local disk behind one trait — not a bespoke SDK layer.
- **Multi-cloud portability beyond what that crate gives for free.** Not designing around providers
  it doesn't support.

## Design

### 1. Configurable partition dimension + entity→partition-set correction (amends RFC 0034)

Unchanged in substance from this RFC's first draft; storage-backend-agnostic, so this revision does
not touch it. RFC 0034 hardcodes the non-time partition axis to `source_scope`. Per explicit user
direction, this is amended to a pluggable dimension:

```rust
/// Supersedes RFC 0034's PartitionMeta.source_scope: String.
pub enum PartitionDimension {
    SourceScope(String),   // RFC 0034's original — e.g. "sql", "discord:#governance"
    EntityKind(String),    // e.g. Table, Module, Symbol, Custom("Risk")
    Composite(Box<PartitionDimension>, Box<PartitionDimension>),
}
```

Configured via `ekos.toml`'s `[storage.partition]` (`dimension = "source-scope" | "entity-kind" |
"composite"`). Every routing/pruning mechanism in this RFC treats the dimension as an opaque
routing key — never pattern-matched on internally.

**Real correctness gap this RFC found in RFC 0034, unresolved by this revision (still RFC 0034's
implementation's problem):** because writes route by *current* time bucket, a long-lived entity's
facts can span more than one partition. RFC 0034's `entity_id → partition_id` lookup must become
`entity_id → Set<PartitionId>`. Current-state reads (`get_object`) still resolve to exactly one
partition (the newest); full-history reads (`object_history`, `object_at` before the most recent
boundary) must fan out to the whole set. This RFC's Service B/C fan-out (§4, §5) directly consumes
that same partition set to decide which workers to contact.

### 2. Object storage as the ledger's durable backing store

`SegmentStore` gains a backend seam, following this project's existing dependency-injection
convention (`Observer`, `LlmProvider`, `CompilerPass` are all traits selected by config):

```rust
/// New seam beneath SegmentStore. LocalFsBackend wraps today's std::fs
/// calls unchanged; ObjectStoreBackend is new, built on the `object_store`
/// crate's ObjectStore trait (AmazonS3 / MicrosoftAzure / S3-compatible /
/// local, one dependency covers both providers the user named).
trait SegmentBackend {
    fn put(&self, path: &str, bytes: Bytes) -> Result<(), SegmentError>;   // sealed objects only
    fn get(&self, path: &str) -> Result<Bytes, SegmentError>;
    fn list(&self, prefix: &str) -> Result<Vec<String>, SegmentError>;
}
```

Layout mirrors today's local directory 1:1, rooted at an object-store prefix instead of a filesystem
path: `<bucket>/<workspace-id>/<partition-id>/segments/seg-<seq>.bin`, `.../indexes/<order>/run-*.bin`,
`.../search/*` (tantivy files).

- **Sealed segments map cleanly onto object storage**: immutable once sealed (RFC 0016) — one `PUT`,
  many `GET`s, exactly the access pattern object stores are built for. No read-modify-write, ever,
  for sealed data.
- **The active (unsealed) segment does not** — object stores have no append operation. It stays
  buffered on whichever Service A worker currently holds that partition's write lease (§3), exactly
  like today's local `SegmentStore`, and only becomes durable in object storage once it seals at
  `SEGMENT_SEAL_BYTES` (8 MB, `segment/mod.rs:53`, unchanged) and uploads as one immutable object.
  **Stated precisely, not glossed over:** a Service A worker crash loses at most that one partition's
  current unsealed segment — bounded at 8 MB, since everything before it is already sealed and
  durable. Shrinking that bound further (partial uploads, or a WAL survived by lease handoff) is an
  Open Question, not designed here.
- **Manifest commits need atomicity object stores don't natively give** (no atomic rename). Rather
  than depend on a provider-specific primitive (S3 needs an external lock table for
  compare-and-swap; ADLS Gen2 has native blob leases but that's Azure-only), the coordinator — which
  already grants write leases (§6) — also arbitrates manifest commits: a worker seals a segment,
  uploads it, then tells the coordinator "commit this new manifest," and the coordinator performs
  the visible-pointer flip. One mechanism, portable across both named providers.

### 3. Service A — Distributed Ingest/Compile MPP cluster

Maps to today's `ekos build/recover/resolve/compile/commit` pipeline (`compiler-core`'s
`PassManager`/`Scheduler` driving `recovery`'s analyzers, then `semantic`'s compiler pass), made
horizontally distributed. New CLI surface, a subcommand alongside the existing ones in
`crates/cli/src/commands/` (one `ekos` binary, matching how `mcp serve`/`simulate`/`replay` are
already subcommands — not a new binary):

```
ekos compile-worker serve --coordinator <addr>
```

- **Work unit** = one partition-scoped shard: `(dimension_value, time_bucket)` per §1 — e.g.
  `("sql", "2026-08")` or `("Table", "2026-08")` depending on the configured dimension.
- A worker **requests a lease** on a shard from the coordinator (§6). Holding the lease makes it
  that shard's sole writer — the same single-writer invariant RFC 0104 already established for one
  machine, now brokered centrally instead of via local `flock`.
- Once leased, the worker runs the **existing, unmodified** recovery + semantic-compile passes
  scoped to that shard's sources, appends to its local buffered active segment (today's
  `SegmentStore` code, unmodified), seals+uploads at threshold (§2), commits the manifest through
  the coordinator.
- N workers holding leases on N different shards write fully in parallel — this is the concrete
  "distributed MPP engine to scan sources and compile into storage" the user asked for, composing
  directly with RFC 0034's existing partition scheme rather than inventing a new one.

### 4. Service B — Distributed Query/EAV-Assembly MPP cluster

```
ekos query-worker serve --coordinator <addr>
```

- **Stateless compute, no durable local state.** On assignment to serve a partition, a worker pulls
  that partition's sealed segments + index runs + tantivy index files from object storage into a
  bounded local cache, mmap'd once downloaded (RFC 0016 Phase 6's mmap reads apply unchanged to the
  cached copy), then runs the **existing, unmodified** `FactIndexes` EAVT/AEVT/AVET fold
  (`crates/ledger/src/index.rs`) and tantivy search (`search.rs`) against it locally.
- This is precisely "search entities with attributes and build long EAV chains" in the user's own
  words: `FactIndexes`' existing EAVT fold plus RFC 0106's checkpoint-accelerated `state_at`, reused
  verbatim — just running against cached-from-object-storage bytes instead of always-local ones.
- Because sealed segments are immutable and object storage is the one durable copy, **any** query
  worker can serve **any** partition. There is no owned/replica-set concept — a real simplification
  versus this RFC's first draft (§7). Losing a worker loses only its warm cache; the coordinator
  reassigns the partition, the next worker re-hydrates from object storage.
- Partition→worker assignment for cache locality/load balancing (consistent hashing vs.
  least-loaded) is a placement policy, not a correctness concern — Open Questions, not designed here.

### 5. Service C — Query Gateway (single logical, load-balanced service)

Maps to today's `ekos mcp serve` / `ekos ask` / `Runtime` (`crates/runtime`,
`crates/cli/src/commands/mcp.rs`). "Single, balanced server" = one logical, **stateless** service:
any number of interchangeable replicas behind a load balancer, since (per §4) it holds no partition
data itself — only routing/merge logic — so any replica answers any request.

- Receives an EKL query / MCP tool call / `ask` question, resolves the pruned partition set via the
  coordinator's catalog (RFC 0034's existing scoping logic, unchanged), dispatches parallel
  sub-queries to whichever Service B workers currently hold those partitions, merges results — point
  reads pass through a single result; full-history reads merge in tx order (§1); full-text search
  merges per-partition BM25 top-K (§8's caveat) — then, for `ask`, runs the existing grounding+
  citation pipeline (`docs-gen`'s `--prose` path) unchanged on the merged result.
- `KnowledgeStore` stays the seam: Service C's core is a `DistributedLedger` implementing
  `KnowledgeStore`, its RPC client now talking to Service B workers rather than node-owned
  `FactLedger`s directly (the change this revision makes). No caller of `KnowledgeStore` — Runtime,
  MCP tool handlers, `docs-gen` — changes at all.

### 6. Coordinator: metadata + write leases, no node-placement/replica-set state

Because object storage is the durable copy, the coordinator tracks only:

- the partition catalog (id, dimension value, time range, tier, object-storage location) — RFC
  0034's `PartitionCatalog`, essentially unchanged in shape;
- who, if anyone, currently holds a shard's write lease (§3) — short-lived and renewable, not a
  permanent assignment;
- the tx watermark per partition, so a Service B worker reading a cached copy knows how stale it is.

This is the same shape as a Delta Lake/Iceberg transaction log or a Hive Metastore: a small,
centralized metadata service in front of object storage holding the real data — a well-understood
pattern, deliberately not a bespoke one. **v1 remains a single coordinator process, the same
acknowledged SPOF as the first draft** — Raft-replicated metadata is still a v2 question (Open
Questions), unchanged by this revision.

### 7. What this revision removes from the first draft, and why

- **Node-to-node segment-shipping replication is gone.** Object storage's own multi-AZ durability
  (both S3 and ADLS Gen2 provide this natively) replaces it — EKOS replicating segment bytes between
  its own nodes would duplicate durability the storage layer already gives for free.
- **`PartitionLocation::Distributed { primary, replicas }` is gone.** Replaced by a plain
  object-storage location plus an ephemeral write-lease holder (§3, §6) — no replica set for Service
  B to read from, because any query worker can materialize any partition on demand.
- **Failover is simpler, not harder**, because Service A/B workers are stateless compute over shared
  durable storage: a crashed compile worker just means its lease expires and another worker re-leases
  the shard, resuming from the last committed manifest (recovery bounded by §2's 8 MB, not
  unbounded); a crashed query worker means simple reassignment, since it held no durable state to
  begin with. The first draft's "automatic failover/leader election" Open Question is substantially
  de-risked by this shape, though lease-expiry/reassignment timing is still open (below).

### 8. Distributed search

Unchanged in mechanism from the first draft, restated against the new roles: Service C fans out
`search(partition, query, limit)` to the Service B workers holding the pruned partition set, merges
top-K by each worker's local BM25 score. **Stated plainly, not glossed over:** per-partition BM25
uses each partition's own local term statistics, so the merged ranking is the same well-known
"query-then-fetch" approximation every distributed search engine makes (Elasticsearch's default
behavior included) — not a mathematically global ranking. Accepted for v1; global term-statistics
aggregation is a possible follow-up, not designed here.

## Alternatives Considered

- **Provider-native locking** (S3 via an external DynamoDB-style conditional-write lock table; ADLS
  Gen2 via native blob leases) instead of coordinator-brokered leases. Rejected for v1: this project
  already needs a coordinator for the partition catalog, and provider-native locking would mean two
  separate implementations to support both providers the user asked for — a single coordinator-
  brokered lease is simpler and storage-provider-agnostic.
- **A hand-rolled S3/Azure SDK integration** instead of the `object_store` crate. Rejected: one crate
  already implements one `ObjectStore` trait for `AmazonS3`, `MicrosoftAzure` (ADLS Gen2), S3-
  compatible stores (MinIO — useful for local dev/testing), and local disk — one dependency instead
  of two bespoke SDK integrations, and it already backs a widely-used ecosystem (DataFusion,
  delta-rs), not a niche choice.
- **Node-owned local disks with app-level replication** (this RFC's first draft). Rejected on
  revision, per §7: duplicates durability the storage layer already provides, and needs the
  coordinator to track replica sets — strictly more moving parts for the same guarantee.
- **Consistent-hash sharding over entity id**, unchanged from the first draft's rejection: destroys
  RFC 0034's scoped-pruning property, since a hash has no relationship to query scope.
- **Raft-replicated coordinator metadata from day one** (e.g. `openraft`) instead of a single
  coordinator process. Rejected for v1 as engineering weight not yet justified by evidence: this
  project's own storage roadmap (RFC 0080) only escalates scope when a real, physical incident
  demands it (e.g. Phase 1's concurrency fix followed real corruption found in `devlog_65`, not a
  hypothetical). No comparable evidence exists yet for the coordinator SPOF specifically. Kept as
  the named v2 path (Open Questions) rather than silently dropped.

## Architecture Review (2026-08-27)

Validated against `ekos.md`'s stated principles and CLAUDE.md's key invariants before further open
questions are resolved below:

- **"Technology Independent" / "storage engines [are] implementation details"** (`ekos.md`
  §"Technology Independent") — directly supports §2's `SegmentBackend` seam: local disk and object
  storage are two implementations behind one trait, exactly the kind of substitution this principle
  anticipates. No tension found.
- **"The ledger is the single source of semantic truth"** (`ekos.md` §5) — preserved: object storage
  is the one durable copy every Service B worker reads from (§4); no worker's local cache is ever
  treated as authoritative, and the coordinator's tx watermark (§6) exists precisely so staleness is
  detectable, not silently assumed away.
- **Append-only, evidence-traceable** (CLAUDE.md key invariants) — unchanged: this RFC moves *where*
  segment bytes live, never mutates them in place, and evidence/object/relationship/event semantics
  are untouched by any part of this design.
- **Runtime stays read-only** — `DistributedLedger` (§5) implements exactly `KnowledgeStore`'s
  existing read/append contract; no new mutation surface introduced anywhere in Service C.
- **Deterministic, side-effect-free compiler passes** (CLAUDE.md coding rules) — Service A (§3) runs
  the *existing, unmodified* `recovery`/`semantic` passes per shard; distribution changes only which
  process executes a pass and where its output is durably written, not the pass logic itself.
- **Dependency injection through traits** (CLAUDE.md coding rules) — `SegmentBackend` (§2) follows
  the same pattern as `Observer`/`LlmProvider`/`CompilerPass`, not a bespoke mechanism.

No inconsistency with the compiler architecture or key invariants found. The review below resolves
several Open Questions from the prior revision with concrete decisions; items genuinely blocked on
real data or on RFC 0034's own implementation stay open rather than being forced to a premature
answer.

**Resolved:**

- **Sync vs. async transport → resolved: async at the RPC boundary only, sync everywhere else.**
  RFC 0001 decided the *compiler pipeline* is sync end-to-end specifically to avoid retrofitting
  async through `CompilerPass`/`Observer`. That reasoning doesn't extend to this RFC's coordinator↔
  worker and gateway↔worker RPC, which sits *alongside* the pipeline, not inside it: each Service A
  worker still runs the existing sync passes for its leased shard unchanged, invoked from an async
  RPC handler via `tokio::task::spawn_blocking` (or equivalent) rather than making `CompilerPass`
  itself async. Adopt tokio at the coordinator, `PartitionRpc` client/server, and Service C's gateway
  layer only — zero existing trait signatures change.
- **Write-lease timing → resolved: short TTL lease with heartbeat renewal; no cross-machine recovery
  of a dead worker's unsealed buffer.** A lease expires (e.g. 30s TTL, 10s heartbeat renewal —
  concrete defaults, tunable, not load-bearing to the design) if its holder stops renewing. On
  expiry, the coordinator does **not** attempt to recover the dead worker's local unsealed segment
  (unreachable by definition) — the next lease-holder simply starts a fresh active segment from the
  last *committed* manifest. This is the same bounded loss §2 already names (≤ `SEGMENT_SEAL_BYTES`,
  8 MB), not a new risk — resolving lease timing this way keeps the loss bound honest rather than
  promising a recovery mechanism that can't actually reach a dead machine's disk.
- **Manifest-commit contention → resolved: fencing tokens.** Each lease grant carries a monotonically
  increasing token (standard distributed-lease pattern for exactly this race — a late write from a
  worker that *thinks* it still holds the lease). The coordinator accepts a manifest commit only if
  its token matches the latest token issued for that shard; a stale-token commit is rejected, not
  silently applied. Closes the race named in the prior revision precisely, not just "the coordinator
  arbitrates."
- **Transport security → resolved (v1 default): mutual TLS over a cluster-internal CA.** Coordinator,
  compile workers, query workers, and gateway replicas all authenticate via mTLS against a shared
  cluster CA (operator-provided or coordinator-issued). Certificate rotation mechanics are an
  implementation detail for the eventual implementation RFC, not designed further here.
- **Opt-in only, `Local` stays the default → resolved: yes.** Consistent with RFC 0016's fact engine,
  which stayed opt-in through a real soak period before any default switch — distributed mode
  (`PartitionLocation::Distributed`) is explicitly opt-in for workspaces that need it;
  `PartitionLocation::Local` remains the indefinite default otherwise.

**Still open** (genuinely needs data this review can't manufacture, or is blocked on other work):

- [ ] **Service B cache eviction policy** (LRU by partition, size-bounded) — needs real query-pattern
      data before a concrete policy is chosen, not a judgment call this review can make responsibly.
- [ ] **Shrinking the bounded unsealed-segment loss window** below 8 MB (periodic partial upload vs.
      a local WAL survived by lease handoff) — a real design trade-off (more durability vs. more
      write amplification/complexity) that needs its own comparison, not resolved here.
- [ ] **Coordinator consensus**: single (v1, accepted SPOF) vs. Raft-replicated (v2) — deliberately
      deferred, not avoided: v1 is a real, named scope decision (see Alternatives Considered), not an
      oversight; revisit only if the v1 SPOF causes a real incident, matching this project's general
      bias against speculative engineering ahead of evidence.
- [ ] `entity_id → Set<PartitionId>` cap/compaction for very long-lived entities (§1) — belongs to
      RFC 0034's implementation, not this RFC.
- [ ] `PartitionDimension::Composite` fan-out cost (§1) — needs real measurement against a realistic
      multi-dimension fixture.
- [ ] RFC 0034's entity-spanning-partitions correction must still be resolved in RFC 0034's own
      implementation first — unchanged hard dependency, not this RFC's to resolve.

## Open Questions (superseded by the Architecture Review above — kept for the historical record of
what the design faced before review)

- [x] Sync vs. async transport — resolved above.
- [x] Write-lease timing — resolved above.
- [x] Manifest-commit contention — resolved above.
- [x] Transport security — resolved above.
- [x] Opt-in only, `Local` default — resolved above.
- [ ] Service B cache eviction policy — still open, see above.
- [ ] Shrinking the bounded unsealed-segment loss window — still open, see above.
- [ ] Coordinator consensus (v1 vs. v2) — deliberately deferred, see above.
- [ ] `entity_id → Set<PartitionId>` cap/compaction — belongs to RFC 0034, see above.
- [ ] `PartitionDimension::Composite` fan-out cost — still open, see above.
- [ ] RFC 0034's entity-spanning-partitions correction — hard dependency, see above.

## Acceptance Criteria

- [x] At least one review completed — Architecture Review (2026-08-27), above: validated against
      `ekos.md`'s principles and CLAUDE.md's key invariants, no inconsistency found; 5 of 11 Open
      Questions resolved with concrete decisions, 1 deliberately deferred with named rationale
      (coordinator consensus), 5 remain genuinely open (real data needed, or blocked on RFC 0034).
- [ ] Remaining open items either resolved or explicitly re-scoped with the user's sign-off: cache
      eviction policy, unsealed-segment loss-window reduction, `Composite` fan-out cost,
      `entity_id → Set<PartitionId>` cap (the last belongs to RFC 0034, not this RFC). Coordinator
      consensus (single vs. Raft) does **not** block acceptance — it is a deliberately scoped v1
      decision, not an unresolved question.
- [ ] RFC 0034 accepted and its entity-spanning-partitions correction resolved (§1's hard
      dependency).
- [x] Design is consistent with `ekos.md`'s compiler architecture and CLAUDE.md's key invariants:
      append-only preserved (object storage holds the same immutable segment bytes, never rewritten
      in place); Runtime stays read-only (`DistributedLedger` only ever implements the existing
      read/append `KnowledgeStore` contract); the storage-format seam is preserved (no
      `KnowledgeStore` caller changes) — confirmed by the Architecture Review above.
- [ ] `SegmentBackend` abstraction (§2) implemented for both `LocalFsBackend` (wraps today's
      unmodified code) and `ObjectStoreBackend` (new), with byte-identical segment contents in both.
- [ ] A dated implementation RFC (or RFCs, one per service, matching RFC 0080's own precedent) is
      written before any code.

## Testing (for the eventual implementation RFC(s), not this one)

- Multi-service local harness: one coordinator, N compile workers, M query workers, one gateway
  replica set, all against a local S3-compatible test double (`object_store`'s in-memory backend, or
  MinIO in a container) — no real cloud dependency needed for the test suite.
- Lease contention test: two compile workers request the same shard, exactly one gets it, the loser
  gets a clear "already leased" error, not a silent conflict.
- Bounded-loss-on-crash test: kill a compile worker mid-active-segment, assert loss is ≤
  `SEGMENT_SEAL_BYTES` (8 MB) and every previously sealed segment is intact and durable.
- Cache-miss-then-hit test: a query worker assigned a partition it's never seen re-hydrates
  correctly from object storage, and a second request to the same worker hits its warm cache.
- Manifest-commit-race test: exercise the lease-expiry-during-upload race named in Open Questions,
  assert no corrupted or lost manifest.
- Distributed search merge test: fixture with matching documents split across ≥2 partitions on
  different query workers, assert the merged top-K is the union ranked by each shard's own BM25
  score, and that the cross-shard ranking caveat (§8) is exercised, not just the happy path.
- Entity-spanning-partitions test (depends on RFC 0034's fix, §1): full-history read for an entity
  crossing ≥2 time-bucket partitions correctly fans out to every Service B worker holding a relevant
  partition.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0110-horizontal-distribution-and-distributed-search.md` | This RFC (revised same day per user direction) |
