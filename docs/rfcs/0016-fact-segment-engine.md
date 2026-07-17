# RFC 0016 — EKOS Fact-Segment Engine

| Field | Value |
|-------|-------|
| **Status** | Accepted (2026-07-17; reviewed against RFC 0001–0015 invariants — see Review notes) |
| **Author** | EKOS team |
| **Created** | 2026-07-17 |
| **Gating** | Ledger v3; supersedes the SQLite backend of RFCs 0005/0011/0015 |
| **Depends on** | RFC 0015 (measurement harness, pack-segment machinery, zstd) |

---

## Motivation

RFC 0015 compressed the SQLite ledger 2.5× and the artifact store ~7× without
changing any semantics. It also identified the ceiling of that approach: the
ledger stores **whole JSON payloads per version**. The live estate holds
42,831 object versions for 22,023 objects — every changed object re-stores
its complete payload, and row-by-row compression cannot see the redundancy
*across* versions and objects (measured: 2.3× per-row vs 6.2× on the
concatenated corpus). Structural problems compression cannot fix:

1. **Version churn duplicates whole payloads.** An object whose `size_bytes`
   property changed re-stores name, kind, path, excerpt, and evidence list.
2. **Graph traversal is N queries per hop.** `load_neighborhood` issues one
   `relationships_for` per node; there is no index whose *sort order* is the
   traversal order.
3. **Temporal queries are second-class.** `object_at` exists, but
   relationships have no per-id history (RFC 0011 limitation); "the estate as
   of T" requires per-object queries.
4. **Search caps at 600-char excerpts.** FTS5 lives inside the ledger's
   transaction, which is elegant, but full-content indexing (RFC 0014
   non-goal) would bloat the SQLite file and its rebuild path.
5. **Branches copy the whole database.** `VACUUM INTO` is correct but O(size);
   diff/merge compare current-state signatures, not history.

This RFC designs the ledger the semantics always wanted: an immutable,
fact-oriented storage engine — the Datomic/XTDB architecture, specialized to
EKOS's four primitives and invariants. It **is** the "own database" the
project's DNA points at, built where owning the format pays: the storage
layout, not transaction folklore. The `Ledger` API is the seam: `runtime`,
`cli`, MCP, diff/merge are consumers and do not change.

## Design overview

```
                    ┌──────────────────────────────────────────────┐
   append_object ──►│ WAL-less commit: seal-and-fsync fact batches │
                    └──────────────┬───────────────────────────────┘
                                   ▼
   .ekos/ledger/
     manifest.json            ← the ONLY mutable file (tiny, atomic-renamed)
     segments/
       seg-000042.facts       ← immutable, content-addressed fact batches
     indexes/
       eavt-0007.idx          ← derived LSM runs (EAVT / AEVT / AVET sorts)
       fts/                   ← tantivy index, committed per sealed segment
```

### 1. Record model — EAV facts

The unit of storage is the **fact**:

```
Fact {
    e:  EntityId      // 16-byte KirId
    a:  AttrId        // u32 — interned attribute path
    v:  Value         // typed scalar | ref(EntityId) | blob-ref
    tx: TxId          // u64 — monotone transaction number
    op: Assert | Retract
}
```

- The four primitives decompose: a `KirObject` becomes facts
  `(e, :object/name, …)`, `(e, :object/kind, …)`, one fact per property
  path; a `KirRelationship` becomes `(e, :rel/from, ref)`, `(e, :rel/to,
  ref)`, `(e, :rel/kind, …)` + property facts; Events likewise. Evidence
  payloads (fragments) stay whole as compressed blobs referenced by
  `(e, :evidence/fragment, blob-ref)` — decomposing prose buys nothing.
- **Nested JSON flattening**: property paths join with `.`
  (`:prop/metrics.rows`); arrays are stored as one composite value (JSON,
  zstd) unless an array of refs — order-preserving decomposition of arbitrary
  arrays is not worth the complexity in v1 of the engine.
- **A new version writes only changed facts.** The build's
  `append_object(obj)` diffs against the current fact set of `e` and emits
  assert/retract pairs. The 1.9× version churn stops costing full payloads —
  a `size_bytes` change is one retract + one assert (~40 bytes before
  compression).
- **Attribute registry**: `a` is a u32 interned via a dictionary persisted in
  the manifest (append-only list; ids never reused).

**Phase 1 refinements** (discovered implementing `ledger/src/fact.rs`; all
are consequences of the byte-parity requirement):

- Facts carry an optional **position** component: the top-level `evidence`
  array decomposes into position-indexed ref facts, because evidence *order*
  is signature-relevant and a bare value set cannot preserve it.
- Attribute paths **escape** literal `.` and `\` in key segments (`\.`,
  `\\`), so `{"a.b": 1}` and `{"a": {"b": 1}}` remain distinct payloads with
  distinct signatures.
- **Empty** objects and arrays are stored as one composite fact — flattening
  them to nothing would erase them from the reconstruction.
- Ref detection is strictly **schema-positional** (`id`, `from`, `to`,
  `subject`, `evidence[*]`) and only for canonical hyphenated-lowercase UUID
  text; anything else in a ref position round-trips verbatim as a plain
  value. Values are never sniffed for UUID-ness.

### 2. Transactions & identity

- `TxId` is a monotone u64. Each commit batch carries `(tx, wall_time_us)` —
  wall time is metadata; ordering authority is `tx` (fixes the RFC 0015
  microsecond-tie problem structurally).
- **`content_sig` survives unchanged**: the signature of an object version is
  the SHA-256 of its canonical JSON *reconstruction* (fact set → JSON object
  with sorted keys, `created_at` stripped — byte-compatible with today's
  `content_signature`). Idempotent append and branch merge semantics are
  therefore identical, and v2→v3 migration can verify signatures byte-for-byte.
  A cache `(e → sig)` of current signatures lives in the manifest's
  current-state index so idempotence checks don't reconstruct JSON per append.
- **Numeric fidelity is signature-critical.** Byte-compatibility requires JSON
  numbers to survive decompose → recompose *exactly*: scalar values store
  `serde_json::Number` semantics (i64/u64/f64 distinction and float lexical
  form via the same serializer), never a lossy f64-only encoding. The fact
  round-trip property tests must include the edge cases — `1` vs `1.0`,
  `u64::MAX`, negative zero, sub-normal floats — before any other phase
  builds on the fact model.
- **As-of queries map wall time to tx.** The public API keeps
  `object_at(id, at: DateTime)`; the engine resolves `at` to the greatest
  `tx` whose batch `wall_time_us ≤ at` via the (tiny, in-manifest) batch time
  index, then reads by `tx`. Ties are impossible by construction — `tx` is
  the ordering authority.
- Determinism: fact decomposition is a pure function of the payload;
  reconstruction is a pure function of the fact set. Property order never
  matters (canonical JSON sorts keys). Every compiler-pass invariant holds.

### 3. Storage — immutable, content-addressed segments

- A **commit batch** = all facts of one `append_*` call (or one build
  transaction). Batches append to the active segment file; the segment seals
  at ~8 MB or on graceful close: `fsync`, then its SHA-256 goes into the
  manifest. Sealed segments never change — the same argument that justified
  Pack v1 (RFC 0015), now for the ledger itself.
- **Crash safety**: the active (unsealed) segment may have a torn tail —
  recovery truncates at the last valid batch boundary (checksummed batch
  frames, same frame discipline as Pack v1). Sealed segments are verified by
  manifest hash. The manifest itself is updated by write-temp + atomic rename.
- **Read-your-writes across processes** (the MCP server reads while a build
  writes): each committed batch fsyncs the active segment and then publishes
  a **committed-length watermark** (a tiny `HEAD` file, atomically renamed).
  Readers see sealed segments plus the active segment *up to the watermark* —
  the same visibility SQLite WAL gives today, without waiting for a seal.
  A crash between fsync and watermark publish loses nothing: recovery scans
  the active segment past the watermark and republishes it.
- **Branches are manifests.** A branch is a new manifest listing the same
  sealed segments plus its own divergent ones — copy-on-write, O(1) instead
  of `VACUUM INTO`'s O(database). Merge = fact-set comparison per entity
  (more precise than today's whole-object signature conflict) with the same
  no-auto-resolve policy (RFC 0011).
- **Compaction** (optional, later): rewrite old segments dropping retracted
  facts *older than a retention horizon* — explicitly a policy decision the
  RFC of that feature must justify against append-only; v1 of the engine
  never deletes.

### 4. Indexes — derived LSM runs in three sort orders

Segments are the truth; indexes are derived and rebuildable (a project
invariant since RFC 0002):

| Sort | Answers |
|------|---------|
| **EAVT** | entity → its facts (object reconstruction, `object_at`) |
| **AEVT** | attribute → entities (EKL `WHERE kind = 'Table'`) |
| **AVET** | attribute+value → entities (name lookup, `:rel/from = e` — graph hops) |

- Each sealed segment's facts sort into per-segment index runs; a background
  (build-time, not daemon) merge compacts runs LSM-style. Lookups
  merge-read across runs (few, bounded by merge policy).
- Blocks are prefix/delta-encoded (sorted runs share long prefixes) then
  dict-zstd'd — the cross-payload redundancy row compression couldn't reach.
- Graph BFS becomes ranged scans of AVET (`:rel/from = X`, `:rel/to = X`)
  instead of N SQL queries; `relationships_at` gets true history via T in the
  sort key, closing the RFC 0011 gap.

### 5. Search — integrated tantivy

- FTS5 dies with SQLite here; tantivy (segment-based, BM25, real tokenizer
  control) indexes name/kind/excerpt per sealed fact segment and commits in
  lockstep — a sealed segment and its tantivy commit are atomic-enough
  (rebuildable index: on mismatch, reindex the segment).
- Weights preserved (name 10×, kind 4×, content 1×). Full-content indexing
  becomes feasible (tantivy's segment merges handle large text corpora); it
  stays opt-in per RFC 0014's cost argument.

### 6. Execution — mmap'd sealed segments

- Sealed segments and index runs are opened via `memmap2`; the **active**
  segment is read with plain `pread` up to the committed watermark — mmap is
  never applied to a file that can still grow or be truncated by recovery.
  **The sealed-file maps require `unsafe`** (the map's validity depends on
  the file not being truncated concurrently); justification per the
  zero-unsafe rule:
  - maps cover only **sealed, content-addressed** files that EKOS never
    mutates or truncates (the manifest referencing them is immutable
    history);
  - a hostile external truncation is a SIGBUS — the same failure class as
    SQLite's own mmap mode, accepted for the same reason;
  - the `unsafe` surface is one audited constructor in one module
    (`ledger/src/segment/map.rs`), wrapped in a safe API.
- Payoff: the open-per-`tools/call` MCP pattern gets OS-page-cache-warm reads
  with zero daemon state, and cold `ekos_search`/`ekos_state` stop paying
  SQLite page decoding.

### 7. Compression

- Fact batches in segments: dict-zstd (dictionary trained per estate, stored
  in the manifest chain, version-byte per frame — RFC 0015's scheme).
- Index blocks: prefix/delta encoding + dict-zstd.
- Evidence blobs: plain zstd frames (they're prose; the fact dictionary
  doesn't help).
- Expected effect (to be measured, not promised): current-state bytes
  approach the *entropy* of the estate rather than payload-count × size;
  version churn cost drops from ~full payload to ~changed facts. The RFC 0015
  harness (`ekos ledger status --storage`, `storage_compaction` bench) is the
  scoreboard.

### 8. Migration & acceptance gate

1. Engine lands behind the existing `Ledger` API (`ekos-ledger` crate keeps
   its public surface; the SQLite backend remains until parity).
2. `ekos ledger migrate --v3` streams v2 rows → facts, preserving tx order
   from rowids and verifying every object's reconstructed canonical JSON has
   an **identical `content_sig`** to its v2 row. History (all versions), not
   just current state, migrates.
3. Gate to switch the default backend: the full ledger + runtime + MCP test
   suite passes on the fact engine; `storage_compaction` shows ≥2× over the
   RFC 0015 ledger at equal-or-better read latency; the agent-session
   integration test is green; branch/diff/merge behave per RFC 0011 tests.
4. Rollback story: v2 `.bak` retained by the migration, same as RFC 0015.

## Alternatives considered

- **Stay on SQLite, add an EAV table** — keeps transactions for free but puts
  a fact-shaped workload on B-tree row storage; the wins (manifest branches,
  mmap reads, prefix-coded index runs, tantivy alignment) all live below the
  SQL layer where SQLite cannot cede control.
- **redb/rocksdb as the fact store** — a KV engine stores sorted runs but
  brings its own compaction/WAL policies (RocksDB: heavyweight C++,
  background threads); EKOS needs deterministic, build-time-only maintenance.
- **Datomic/XTDB themselves** — JVM services; EKOS is a local-first Rust CLI.
  Their architecture is the borrowed asset, not their runtime.
- **Keep FTS5** — no SQLite, no FTS5. Tantivy is the natural fit for the
  segment lifecycle and unlocks full-content search.

## Review notes (acceptance, 2026-07-17)

Checked against every standing invariant: append-only (retract is a fact,
nothing is deleted; compaction explicitly deferred to its own RFC), evidence
traceability (evidence entities and their blobs survive as facts), determinism
(decomposition/reconstruction are pure; no wall-clock ordering authority),
content-addressing (signatures byte-compatible with v2 by construction, so
migration is verifiable), derived-index rebuildability (all three sorts and
tantivy rebuild from segments), runtime read-only (unchanged `Ledger` seam),
and the zero-unsafe rule (single audited mmap constructor over sealed files).

Four gaps were found in the draft and resolved in this revision:

1. **Active-segment visibility** — readers would have gone stale until a seal;
   fixed with the fsync-then-publish committed-length watermark (§3).
2. **Numeric fidelity** — a lossy number encoding would silently change every
   signature; fixed by mandating `serde_json::Number` semantics plus edge-case
   property tests gating Phase 1 (§2).
3. **Wall-time → tx resolution** for `object_at` was implied but unspecified;
   now explicit (§2).
4. **mmap scope** — the active segment is `pread`-only; maps cover sealed,
   immutable files exclusively (§6).

## §7 measured outcome and gate status (2026-07-17, post-implementation)

All §7 levers were implemented and measured on the live estate (80,211
versions): dictionary-zstd batch bodies (manifest dictionary, level 19),
prefix-delta-encoded binary index blocks (level 19), slim AEVT/AVET bodies,
and AVET restricted to **ref values only** (the only value-shaped lookup any
read path issues; indexing scalar values had put 600-char excerpts inside
keys). Result: 98 → **65 MB** (segments 25, indexes 30, tantivy 10) against
the v2 ledger's 39 MB — ratio 0.60× vs the §8 gate's ≥2×.

**The size gate is structurally unreachable for this architecture**: the
segment truth (~25 MB, already dict-zstd — on par with v2's entries table)
plus any usable index set (≥15 MB even with a pointer-EAVT redesign) plus
tantivy's ~10 MB floor cannot go below ~50 MB, because v2 stores exactly one
compressed copy of everything and derives search in-file. The fact engine
buys **capability** — 19× faster search, ~4× point reads, true relationship
history, O(1) copy-on-write branches, semantic deltas — at ~1.7× the bytes,
not fewer.

Standing resolution until amended: the default backend remains SQLite;
`ekos ledger migrate --v3` is an explicit, reversible per-workspace opt-in.
Amending the gate (e.g. "≤2× of the RFC 0015 ledger, given the capability
set") or pursuing the pointer-EAVT redesign are decisions for a future
revision, to be taken with these numbers on the table.

## Phasing (each phase = its own tests + benchmarks, per the workflow)

1. **Fact model crate** (`ledger/src/fact.rs`): decomposition ↔
   reconstruction with property-based round-trip tests + signature-parity
   tests against `content_signature`.
2. **Segment format**: batch frames, seal/verify, torn-tail recovery —
   generalizing Pack v1's proven code.
3. **Index runs + merge**: EAVT/AEVT/AVET builders, ranged scans, LSM merge.
4. **Ledger API parity**: implement every `Ledger` method; run the existing
   suite against both backends in CI.
5. **tantivy + mmap**: search parity, then the `unsafe`-justified mmap read
   path.
6. **Migration + flip**: `migrate --v3`, acceptance gate, default switch.

## Non-goals

- Multi-writer concurrency (EKOS has one writer by design; the manifest
  lock enforces it).
- Distributed/replicated operation.
- Query language changes — EKL and the MCP tools are unchanged consumers.
- Compaction/retention policies (explicitly a later RFC; v1 never deletes).
- Embedding/vector search (still deferred; tantivy adoption is orthogonal).
