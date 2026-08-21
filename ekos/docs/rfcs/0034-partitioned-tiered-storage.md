# RFC 0034 — Partitioned, Tiered Fact-Segment Storage for High-Volume Sources

**Status:** Draft
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
  terabytes prove insufficient.
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

## Open Questions

- [ ] Time-bucket granularity — monthly by default, but should genuinely high-volume sources (a
      firehose chat channel) get daily buckets while low-volume sources (a slowly-changing SQL
      schema) stay monthly or even unbucketed? Likely per-source-scope configurable, not global.
- [ ] Cold-tier recompression level and rehydration cost budget — needs real measurement against a
      realistic multi-partition fixture, not assumed.
- [ ] Does the entity→partition lookup index itself need partitioning at sufficiently large scale
      (millions of partitions), or does a single compact index stay small enough indefinitely
      given it stores only ids + partition pointers, not fact content?
- [ ] Object-storage backing for cold partitions (v2) vs. local-disk-only cold tier (v1, simpler,
      still gets the compaction/index-size wins without adding a cloud-storage dependency) — v1
      should probably be local-disk-only cold tier, deferring object storage to a follow-up.

## Testing

- Fixture with 3+ partitions (mixed hot/cold, mixed source scopes), asserting: a point read routes
  to exactly one partition (no fan-out), a scoped broad query touches only matching partitions,
  and an unscoped query correctly fans out to all.
- Cold-tier round-trip: seal a partition, mark it cold (recompress, drop search index), then read
  from it — assert correct rehydration and that the read result is identical to before going cold.
- Concurrent-writer test: two partitions accept concurrent appends without contention, verified via
  the same crash-recovery/torn-tail tests `segment/mod.rs` already has, run per-partition.
- Compaction-cost test: assert `merge_runs` cost on an N-partition estate scales with one
  partition's size, not total estate size — the concrete claim this RFC exists to make true.

## Acceptance Criteria

- [ ] All Open Questions resolved.
- [ ] At least one review completed.
- [ ] `PartitionedLedger` routes point reads to a single partition and broad reads to a pruned
      partition set, tested against the fixture above.
- [ ] Cold-tier round-trip test passes with byte-identical read results.
- [ ] Concurrent-writer test passes across ≥2 partitions.
- [ ] Design is consistent with `ekos.md`'s compiler architecture and `CLAUDE.md`'s key invariants
      (append-only, evidence-backed, read-only Runtime) — partitioning and tiering are purely an
      access-path change, no invariant is weakened.
