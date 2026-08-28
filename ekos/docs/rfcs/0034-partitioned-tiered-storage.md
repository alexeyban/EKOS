# RFC 0034 — Partitioned, Tiered Fact-Segment Storage for High-Volume Sources

**Status:** Withdrawn — superseded by **RFC 0111** (2026-08-27), which merges this RFC and RFC 0110
into one conformed partitioned/tiered/distributed storage design, per explicit user direction. Kept
on disk as the historical record of how that design was reached (Architecture Review, the
`entity_id → Set<PartitionId>` correctness fix, and the `PartitionDimension` amendment below all
carried forward into RFC 0111 unchanged in substance). Do not implement against this file — read
RFC 0111 instead.
**Author:** EKOS team
**Created:** 2026-08-07

---

## Motivation

Follow-up to RFC 0033 (Discord/Slack connector): how does EKOS hold and process terabytes of
messages or events effectively? Grounding this against the current fact-segment engine (RFC 0016,
Accepted, implemented, running the live estate at ~216 MB / 88K entries today) surfaces a concrete,
code-verified gap rather than a hypothetical one.

`FactLedger::open(root)` (`ekos/crates/ledger/src/fact_ledger.rs:101`) composes exactly **one**
`SegmentStore` (`ekos/crates/ledger/src/segment/mod.rs`) rooted at `root`, **one** `FactIndexes`
at `root/indexes`, and **one** tantivy `SearchIndex` at `root/search` — for the *entire* workspace.
Every fact from every connector (SQL, Git, GitHub, and any future chat connector) shares one
segment stream, one set of order-preserving indexes, one full-text index. `SegmentStore` is
explicitly single-writer ("Single writer (the caller ensures it, as with the SQLite ledger
today)", `segment/mod.rs:112`) and appends to one active segment file at a time. `batches_after`
(`segment/mod.rs:339`) already skips whole sealed segments outside a `tx` cutoff cheaply, and
`FactIndexes::merge_runs` already compacts — but both operate over the *whole* ledger's
index/segment set, not a bounded slice.

So "terabytes of messages" isn't a storage-format problem — RFC 0016's segment/frame/compression
design is sound and log-structured, the right foundation, the same pattern the industry already
uses to hold terabytes (Kafka/LSM-tree-style immutable segments). It is a **missing partition
dimension** and a **missing hot/cold tiering policy**, both entirely absent today. This RFC
proposes both, as a layer composing multiple existing `FactLedger` instances rather than changing
the segment/frame format itself.

## Scope

- A partitioning layer (`PartitionedLedger`) routing writes and reads across multiple `FactLedger`
  instances, keyed by `(source_scope, time_bucket)`.
- A hot/cold tiering policy operating on whole partitions: cold partitions are recompressed, drop
  their search index, and are lazily rehydrated on read.
- The concrete throughput/compaction/search-index-size wins this unlocks for high-volume sources
  like RFC 0033's chat connector or RFC 0032's on-chain event stream.

## Non-goals

- Distributing storage across multiple machines. This is vertical partitioning for I/O/index
  locality on one machine, not horizontal scale-out — a distinct, larger RFC if single-machine
  terabytes prove insufficient. **That RFC now exists: RFC 0110 (Under Review, 2026-08-27)**,
  which treats this RFC's `PartitionMeta` as the unit of physical node placement, amends
  `source_scope` into a configurable `PartitionDimension`, and names a real correctness gap this
  RFC's implementation must resolve — an entity whose facts cross a time-bucket boundary can span
  more than one partition, so `entity_id → partition_id` must become `entity_id → Set<PartitionId>`
  (RFC 0110 §1). RFC 0110's own acceptance is blocked on that resolution.
- Retention or deletion policy. This RFC is purely about access/compaction efficiency for data
  that is kept. RFC 0033's per-channel opt-in remains the actual lever for not ingesting unwanted
  volume in the first place; any future "delete data older than N" policy is a distinct RFC given
  the ledger's append-only invariant (`CLAUDE.md`'s key invariants).

_Both multi-machine distribution and retention/deletion are tracked under `TODO.md` → "Storage
architecture: none of this is implemented yet"._
- Changing the segment/frame format. Every existing guarantee (checksummed frames, crash recovery,
  manifest verification) is reused unchanged, once per partition.

## What already exists and is reused as-is

- Frame/segment format, seal-on-8MB-threshold, manifest+SHA-256 verification, crash recovery
  (torn-tail truncation, stale-watermark catch-up) — `segment/mod.rs`. Unchanged; each partition
  gets its own instance of exactly this.
- `FactIndexes`/`merge_runs` compaction, order-preserving byte-key runs — `index.rs`. Unchanged in
  mechanism; scoped per-partition instead of globally.
- Tantivy `SearchIndex`, BM25, mmap'd reads — `search.rs`. Unchanged in mechanism; one instance per
  *hot* partition.
- RFC 0033's per-channel allowlist — the primary noise-reduction lever, still the first line of
  defense before storage engineering even matters.

## Design

### Partitioning: one `FactLedger` per partition, routed by a thin catalog layer

```rust
pub struct PartitionMeta {
    pub id: PartitionId,
    pub root: PathBuf,
    pub source_scope: String,       // e.g. "discord:#governance", "sql", "chain:eth"
    pub wall_time_range: (i64, i64),
    pub tx_range: (TxId, TxId),
    pub tier: Tier,                 // Hot | Cold
}

pub struct PartitionCatalog {
    pub partitions: Vec<PartitionMeta>,
}

pub struct PartitionedLedger {
    catalog: PartitionCatalog,
    open: HashMap<PartitionId, FactLedger>,  // lazily opened
}
```

**Partition key = `(source_scope, time_bucket)`** — e.g. `source_scope = "discord:#governance"`,
`time_bucket = "2026-08"` (monthly; daily for very high-volume sources, configurable). This mirrors
the exact boundary RFC 0033 already established at the connector-config level (per-channel
opt-in) — partitioning by the same dimension the noise-reduction policy already uses, rather than
inventing a second, inconsistent axis.

- **Writes**: route to the partition matching the fact's source + current time bucket; open (or
  create) that partition's `FactLedger` on demand. This is the concrete fix for the single-writer
  bottleneck too — N partitions admit N concurrent writers (one per source/bucket), instead of
  every connector serializing through one global `SegmentStore`.
- **Point reads** (`ekos_state` on a known object id): a compact `entity_id → partition_id`
  lookup (itself an order-preserving index, small relative to full fact volume) routes directly to
  one partition — no fan-out.
- **Broad reads** (`ekos_search`, `ekos_ekl` range/full-text queries): fan out only to partitions
  whose `source_scope`/`wall_time` range could match, using the catalog's metadata to prune — e.g.
  "recent #governance messages" touches a handful of monthly partitions, not the whole estate.
  This is the same pruning principle `batches_after` already applies at the segment level (skip
  whole sealed segments outside a tx cutoff), lifted one level up to whole partitions.

### Hot/cold tiering

A partition's tier is a property of the catalog entry, not a different storage format:

- **Hot**: full `FactLedger` (segments + indexes + tantivy search) available, mmap'd.
- **Cold**: a sealed partition past a configurable age (e.g. 90 days with no new writes) is
  recompressed at a higher zstd level (segment frames already carry a dictionary-version byte —
  `FRAME_VERSION`/`SegDict`, `segment/mod.rs:120` — a cold-tier dictionary generation is an
  additive, backward-compatible change, not a format break), its tantivy `SearchIndex` is dropped
  (rebuildable on demand from the segments — `search.rs`'s existing rebuild-from-scratch capability
  applies unchanged), and the partition directory is eligible to move to cheaper backing storage
  since `SegmentStore::open` only needs a directory, not a specific filesystem.
- **Promotion back to hot** happens automatically on any read that touches a cold partition (lazy
  rehydration) — no separate "unfreeze" operation needed.

### What this buys, concretely

- **Ingestion throughput**: N concurrent single-writer partitions instead of one global
  single-writer segment store.
- **Compaction cost bounded per-partition**: `merge_runs` on a monthly partition, not the whole
  multi-terabyte history — compaction cost stops growing with total ledger age.
- **Search index size bounded**: tantivy only indexes hot partitions; a multi-year chat history
  doesn't force one ever-growing search index to stay resident.
- **Query cost matches query scope**: "what happened in #governance this week" touches one
  partition; it never pays for the other 99% of history, hot or cold.

### Honest limits

- **Not a distributed system** — see Non-goals. All partitions still live under one `.ekos` root
  on one machine.
- **Cross-partition queries still cost more than single-partition ones.** A query with no
  source/time scope ("find every mention of X anywhere, ever") still fans out to every hot
  partition and pays to rehydrate every cold one it touches — partitioning bounds the *common*
  case, not the worst case.
- **Retention/deletion is still not addressed.** Deletion of a whole cold partition's directory is
  at least a *clean* unit of work this design enables, but deciding to allow it is a separate
  policy question, not a storage one.

## Amendment (2026-08-27): Configurable Partition Dimension, Entity→Partition-Set Correction

Prompted by RFC 0110 (Under Review — Storage Architecture Phase 6, horizontal distribution),
authored against this RFC before it has shipped. Two changes to this RFC's own Design, both still
Draft/unaccepted like the rest of it — recorded here as their own dated, reviewable unit rather than
rewritten silently into the original section above.

### 1. `source_scope: String` becomes a configurable `PartitionDimension`

The original Design (above) hardcodes the non-time partition axis to `source_scope`. Per explicit
user direction while scoping RFC 0110, this is amended to a pluggable dimension:

```rust
/// Supersedes PartitionMeta.source_scope: String above.
pub enum PartitionDimension {
    /// The original behavior — e.g. "sql", "discord:#governance".
    SourceScope(String),
    /// Partition by KIR object/entity kind instead — e.g. Table, Module,
    /// Symbol, Custom("Risk"). Not part of the original design; added
    /// because a workspace's query load may skew toward "all Tables"
    /// rather than "everything from one connector."
    EntityKind(String),
    /// Both axes at once. Finer-grained placement, more partitions for an
    /// unscoped query to fan out to.
    Composite(Box<PartitionDimension>, Box<PartitionDimension>),
}

pub struct PartitionMeta {
    pub id: PartitionId,
    pub root: PathBuf,
    pub dimension: PartitionDimension,   // was: source_scope: String
    pub wall_time_range: (i64, i64),
    pub tx_range: (TxId, TxId),
    pub tier: Tier,
}
```

Configured via a new `[storage.partition]` `ekos.toml` block, following this project's existing
`[section]` convention (`[llm-description]`, `[document-semantics]`, …):

```toml
[storage.partition]
dimension = "source-scope"   # "source-scope" | "entity-kind" | "composite"
time-bucket = "monthly"
```

Every mechanism this RFC built on top of `source_scope` — routing, pruning, and the Alternatives
Considered rejection of "time only, no source dimension" below — applies unchanged to whichever
`PartitionDimension` variant is configured. The dimension is an opaque routing key throughout the
rest of this RFC's design, never pattern-matched on internally.

### 2. Real correctness gap: `entity_id → partition_id` must be `entity_id → Set<PartitionId>`

Found while scoping RFC 0110, not previously named in this RFC. The "Partitioning" subsection above
states writes route "to the partition matching the fact's source + **current time bucket**" —
meaning later versions of a long-lived entity can land in a *different* partition than earlier
versions once a time-bucket boundary is crossed. The Design section's "Point reads" bullet claims a
single `entity_id → partition_id` lookup routes directly to one partition, no fan-out. **That claim
is only true for an entity that has never crossed a time-bucket boundary — it is wrong in general.**

**Correction:** the entity→partition lookup index must map `entity_id → Set<PartitionId>`, not a
single id. Point reads for current state (`get_object`) still resolve to exactly one partition — the
entity's *most recent* partition, since current state always lives in the newest one, so this RFC's
central "no fan-out for point reads" claim survives for that specific case. But any full-history read
— `object_history`, or `object_at`/`relationships_at` for a timestamp that could predate the entity's
most recent time-bucket partition — must fan out to every partition in that entity's set, not one.

This does not change the RFC's core architecture (routing above `SegmentStore`, one `FactLedger` per
partition) — it changes one index's cardinality and adds a fan-out path for one class of read the
original design assumed was always single-partition. **It must be resolved in this RFC's own
implementation before RFC 0110's distributed fan-out design — which reuses this same partition set
to decide which network nodes to contact — can be implemented against real code.**

## Alternatives Considered

- **Change the segment/frame format itself to carry a partition key inline, one global segment
  stream.** Rejected: this reintroduces the exact "everything shares one manifest/one active
  segment/one writer" bottleneck this RFC exists to remove — partitioning must happen *above*
  `SegmentStore`, via multiple instances, not inside one.
- **Partition by source only, no time dimension.** Simpler, but an always-growing single partition
  per source (e.g. one `#governance` partition forever) reintroduces unbounded compaction/index
  growth for any long-lived channel — the time dimension is what makes tiering (age out old
  buckets) possible at all.
- **Partition by time only, no source dimension.** Rejected because RFC 0033's noise-reduction
  policy is source-scoped (per-channel allowlist) — a query for "#governance only" should be able
  to skip irrelevant sources within the same time window too, not just irrelevant time ranges.

## Architecture Review (2026-08-27)

Validated against `ekos.md`'s stated principles and CLAUDE.md's key invariants: no inconsistency
found. Partitioning/tiering is purely an access-path change over the existing `FactLedger`/
`SegmentStore`/`FactIndexes`/tantivy stack — no format, no invariant, no `KnowledgeStore` caller is
touched. Append-only, evidence-traceability, and Runtime read-only-ness are all preserved by
construction, since this RFC composes existing per-partition `FactLedger` instances rather than
introducing any new write path.

Of the six Open Questions below, four are resolved by reasoning grounded in the existing,
already-measured implementation; one is resolved by RFC 0110 now existing (it was previously an
open-ended "someday" question, now a named, designed follow-up); one is only partially resolved,
with the unresolved half stated precisely rather than left vague.

**Resolved:**

- **Time-bucket granularity → resolved: global default + per-scope overrides, reusing an existing
  config pattern.** This codebase already has exactly this shape for a different concern —
  `[[recover.sql.dialect-rules]]`'s `path-glob`-keyed override list (`compiler-core/src/config.rs`).
  Reuse it rather than inventing a second override mechanism:
  ```toml
  [storage.partition]
  dimension = "source-scope"
  time-bucket = "monthly"        # global default

  [[storage.partition.time-bucket-overrides]]
  scope-glob = "discord:*"
  time-bucket = "daily"
  ```
  A firehose source opts into daily buckets by name/glob; everything else stays monthly by default.
- **Entity→partition lookup index needing its own partitioning at scale → resolved: no, for v1.**
  The lookup (post-amendment: `entity_id → Set<PartitionId>`) is structurally identical in shape to
  an existing AEVT-style order-preserving run-file index (RFC 0016 Phase 3) — the same technology
  already serving the live estate's ~88K entries (this RFC's own Motivation section) two orders of
  magnitude below the "millions of partitions" scale the question worried about. If real growth ever
  approaches that range, RFC 0016's own `merge_runs` compaction already bounds the cost — no
  bespoke partitioning of this one index is warranted ahead of evidence it's actually needed,
  consistent with this project's general bias against speculative engineering.
- **`entity_id → Set<PartitionId>` cap/compaction → resolved: no cap needed; the set is naturally
  bounded by wall-clock time, not write frequency.** An entity's partition set grows by one entry
  only when its writes cross into a *new* time-bucket partition — not per version, since most
  versions of an actively-mutated entity land in the same (current) partition between boundaries.
  The set size is therefore bounded by `workspace_age ÷ time_bucket_granularity`: even a 10-year-old,
  monthly-partitioned, continuously-mutated entity accumulates at most ~120 entries. A high write
  *frequency* does not by itself grow this set — only calendar time does. No compaction policy is
  needed for v1.
- **Object-storage backing for cold partitions → resolved: v1 stays local-disk-only, exactly as
  this RFC originally leaned — and the "follow-up" is no longer vague.** RFC 0110 (Under Review) now
  exists and fully specifies object storage (S3/ADLS Gen2, via the `object_store` crate) as the
  ledger's durable backing store for anyone who opts into distributed mode — generalized to *all*
  tiers there, not cold-only. This RFC's own single-machine scope (Non-goals) correctly stays
  local-disk-only; the open-ended "v2, someday" is now a named, designed RFC rather than deferred
  indefinitely.
- **Cold-tier recompression level → resolved: no new lever, reuse the existing level.** Segment
  bodies already compress at `BODY_ZSTD_LEVEL = 19` (`segment/mod.rs`) via the existing dict-zstd
  mechanism (RFC 0016 §7). A cold-tier dictionary generation (already designed as additive via
  `dict_version`/`SegDict`) reuses the same level — introducing a *second*, higher compression level
  for cold data adds a new, unmeasured lever for marginal gain past level 19's already-steep
  diminishing returns. Not designed further.

**Partially resolved — the qualitative shape is settled, a quantitative threshold is not:**

- **`PartitionDimension::Composite` fan-out cost.** The *shape* is resolved by reasoning, not
  measurement: `Composite` partitions by the product of both dimensions' cardinalities (N scopes ×
  M kinds, vs. N or M alone), so a *scoped* query (naming both dimensions) is unaffected — still
  exactly one partition — while an *unscoped* query fans out to N×M instead of N or M. The common
  case (scoped queries) doesn't get worse; the worst case (fully unscoped) does, proportionally to
  the product. What's still open: the concrete N×M threshold past which that worse worst-case
  actually matters in practice — needs a real multi-dimension fixture, not resolved here.

**Still open, unchanged:**

- [ ] Cold-tier **rehydration cost budget** (distinct from the recompression-level question above,
      now resolved) — needs real measurement against a realistic multi-partition fixture.
- [ ] `PartitionDimension::Composite`'s concrete fan-out threshold (see "partially resolved," above).

## Testing

- Fixture with 3+ partitions (mixed hot/cold, mixed source scopes), asserting: a point read routes
  to exactly one partition (no fan-out), a scoped broad query touches only matching partitions,
  and an unscoped query correctly fans out to all.
- **(Added by the 2026-08-27 amendment)** Entity-spanning-partitions fixture: one entity mutated
  across ≥2 time-bucket boundaries, asserting `get_object` (current state) still routes to exactly
  one partition, while `object_history`/`object_at` for a timestamp before the most recent boundary
  correctly fans out to every partition in the entity's set and returns complete, correctly ordered
  history — the concrete regression test for the gap the amendment found.
- **(Added by the 2026-08-27 amendment)** Configurable-dimension fixture: the same partitioning/
  pruning tests above, run once with `dimension = "source-scope"` and once with `"entity-kind"`,
  asserting identical routing/pruning behavior modulo which axis is used.
- Cold-tier round-trip: seal a partition, mark it cold (recompress, drop search index), then read
  from it — assert correct rehydration and that the read result is identical to before going cold.
- Concurrent-writer test: two partitions accept concurrent appends without contention, verified via
  the same crash-recovery/torn-tail tests `segment/mod.rs` already has, run per-partition.
- Compaction-cost test: assert `merge_runs` cost on an N-partition estate scales with one
  partition's size, not total estate size — the concrete claim this RFC exists to make true.

## Acceptance Criteria

- [x] At least one review completed — Architecture Review (2026-08-27), above: no inconsistency
      with `ekos.md`/CLAUDE.md found; 4 of 6 Open Questions resolved, 1 partially (shape resolved,
      threshold open), 1 (rehydration cost budget) remains fully open pending real measurement.
- [ ] Remaining open items resolved or explicitly re-scoped: cold-tier rehydration cost budget
      (needs real measurement); `PartitionDimension::Composite`'s concrete fan-out threshold (needs
      a real multi-dimension fixture).
- [ ] `PartitionedLedger` routes point reads to a single partition and broad reads to a pruned
      partition set, tested against the fixture above.
- [ ] Cold-tier round-trip test passes with byte-identical read results.
- [ ] Concurrent-writer test passes across ≥2 partitions.
- [ ] Design is consistent with `ekos.md`'s compiler architecture and `CLAUDE.md`'s key invariants
      (append-only, evidence-backed, read-only Runtime) — partitioning and tiering are purely an
      access-path change, no invariant is weakened.
- [ ] **(Added by the 2026-08-27 amendment)** `entity_id → Set<PartitionId>` implemented; the
      entity-spanning-partitions fixture (Testing, above) passes — full-history reads for an entity
      that crosses ≥2 time-bucket partitions return complete, correctly ordered history via fan-out,
      while `get_object` still resolves to a single partition.
- [ ] **(Added by the 2026-08-27 amendment)** `PartitionDimension` (`SourceScope` | `EntityKind` |
      `Composite`) implemented and configurable via `ekos.toml`'s `[storage.partition]`; the
      configurable-dimension fixture (Testing, above) passes for both `SourceScope` and `EntityKind`.
