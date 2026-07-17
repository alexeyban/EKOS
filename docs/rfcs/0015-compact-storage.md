# RFC 0015 — Compact Storage

| Field | Value |
|-------|-------|
| **Status** | Accepted |
| **Author** | EKOS team |
| **Created** | 2026-07-17 |
| **Gating** | Storage efficiency (v0.x); groundwork for RFC 0016 |

---

## Motivation

The live 44-project estate compiles into a **326 MB** `.ekos` workspace for
~22K objects / ~5.2K relationships — roughly **5× the logical content size**.
Measured breakdown (2026-07-17):

| Component | On disk | Logical | Waste mechanism |
|-----------|---------|---------|-----------------|
| `artifacts/` | 214 MB | ~67 MB | 46,539 loose files, median ~707 bytes — each burns a 4 KB filesystem block (~3.2×), plus `to_string_pretty` whitespace |
| `ledger/ledger.db` | 101 MB | ~42 MB payload | uncompressed JSON as TEXT rows; FTS5 content duplication (~18 MB); 64-char hex signatures, RFC 3339 timestamps and 36-char UUIDs stored as TEXT; full payload copy per version (42,831 object versions for 22K objects) |
| `snapshots/` + `ckm/` | 11 MB | — | pretty-printed JSON, unbounded snapshot accumulation |

Measured compressibility of the real data (zstd level 3): ledger payloads
**6.2×**, small artifacts **~10×**. Individual ledger rows average 335–689
bytes — too small for standalone zstd frames to compress well, so a **trained
dictionary** is the load-bearing detail for the ledger.

RFC 0002 explicitly reserved this work: *"Binary formats are a Phase 13
optimization — the current design must not assume binary."* This RFC is that
optimization's first stage. The second stage — a purpose-built fact-segment
engine — is RFC 0016; this RFC's changes are deliberately scoped so nothing
here is thrown away by it (the artifact pack format survives as-is, and the
ledger changes are internal to the `ekos-ledger` crate behind its public API).

**Target: ~326 MB → ≤55 MB (≥6×) with no read-path regression >10%.**

## Invariants preserved

Every RFC 0002/0005/0011/0014 invariant survives unchanged:

- **Append-only** — compression changes how bytes are stored, never what is
  stored; no version row is dropped or rewritten in place.
- **Content-addressing & determinism** — identity continues to be the SHA-256
  of *canonical JSON* (`ArtifactId::compute`, `content_signature`).
  Compression is applied **after** hashing, on the storage side only. All
  existing IDs and signatures remain byte-identical. This deliberately rejects
  hashing a binary serialization (postcard/bincode derive byte layout from
  struct field order — an innocent field reorder would silently change every
  hash).
- **Derived indexes rebuildable** — the FTS index and the pack sidecar index
  can both be dropped and rebuilt from primary data.
- **Runtime read-only**; **evidence traceability** — untouched.
- **Zero `unsafe`** — the `zstd` crate is a C library behind safe bindings,
  the same precedent as `rusqlite` with `bundled`.

## Design

### 1. Measurement first: `ekos ledger status --storage`

`ekos ledger status` gains a `--storage` flag reporting per-component bytes:
artifacts (file count, bytes), ledger (total, plus per-table breakdown via
`dbstat`), snapshots, ckm. A criterion bench `storage_compaction.rs` records
bytes-per-1K-objects and `get_object` / `find_objects` latency. Every later
phase is validated against these numbers.

### 2. Free wins (no format break)

- All machine-read JSON switches `to_string_pretty` → `to_string`: the
  artifact store, `ckm/model.json`, build snapshots. (Human-facing CLI output
  stays pretty.) Artifact files are located by hash-derived *path*, not by
  file bytes, so this breaks nothing.
- `ckm/model.json` → `ckm/model.json.zst` (zstd level 3); snapshots likewise.
- Snapshot retention: keep the most recent N (default 10) snapshot files;
  the full history remains in the content-addressed store via IndexArtifacts.

### 3. Ledger schema v2 (`PRAGMA user_version = 2`)

All changes are internal to `ekos-ledger`; the crate's public API (String ids,
`DateTime<Utc>`, `serde_json::Value` payloads) is unchanged.

| Column | v1 | v2 |
|--------|----|----|
| `entries.payload` | JSON TEXT | BLOB: `[dict_version: u8]` + zstd frame of compact JSON |
| `entries.content_sig` | 64-char hex TEXT | 32-byte BLOB |
| `entries.written_at` | RFC 3339 TEXT | INTEGER unix microseconds (millis would collide within one build tick and break `object_at` ordering) |
| `entries.id` | 36-char UUID TEXT | 16-byte BLOB |

- **Dictionary:** trained once (zstd `--train`-equivalent via the `zstd`
  crate) on the workspace's own payload corpus at migration time, stored in a
  new `meta` table. Each frame is prefixed with the dictionary version byte
  (`0` = no dictionary, for corpora too small to train on), so a future
  retraining never requires rewriting old rows.
- **FTS5 goes contentless** (`content=''`, `contentless_delete=1` — supported
  by the bundled SQLite ≥3.46): eliminates the ~18 MB shadow copy of
  name/kind/excerpt. The FTS rowid becomes the current version's
  `entries.rowid`; `find_objects` resolves ranked rowids back to id/name
  through the payload (≤50 rows, one join). bm25 weights (name 10×, kind 4×,
  content 1×) and the unicode61 tokenizer are unchanged.
- **Migration:** `ekos ledger migrate` streams every v1 row in `rowid` order
  into a new v2 file (same transaction), verifies counts and per-row
  round-trip (decode → canonical JSON → signature equality), then atomically
  swaps files. Migration — not rebuild — is required: `ekos build` from
  scratch is cheap thanks to `fingerprints.json`, but it would lose
  append-only *history*. Opening a v1 ledger without migrating keeps working
  (version sniff on open); writes to a v1 ledger stay v1 until migrated.
- **Branch/diff/merge untouched:** `VACUUM INTO` has whole-file semantics
  (format-agnostic); `merge_branch`/`diff_ledger` already compare
  `content_signature` over decoded JSON values.

Measured on the live estate (80,211 entries, 12 s migration): 99 MB →
**39.1 MB (2.5×)**. Row-by-row compression of sub-KB payloads lands well
below the 6.2× concatenated-corpus figure even with the dictionary — the
per-row limit is redundancy *within* one payload; cross-payload redundancy
is exactly what RFC 0016's fact decomposition targets. Reads get *faster* —
fewer pages to traverse; dictionary-zstd decompression of sub-KB frames is
single-digit microseconds. Payload frames use zstd level 19 (writes are
~150 µs/row, once per version; decompression speed is level-independent).

Bonus correctness fix: the v1 FTS accumulated duplicate rows on every
re-append (`INSERT OR REPLACE` cannot replace in FTS5 — no unique
constraint), inflating the live index to 44,796 rows for 22,023 objects and
returning duplicated search results. The v2 contentless index is keyed by
the current version's rowid and cannot duplicate.

### 4. EKOS Pack v1 — the artifact store's own binary format

The loose-object directory becomes packed segments, mirroring git's
loose→packfile evolution. This is also the deliberate de-risking spike for
RFC 0016's segment machinery, on the simplest possible workload
(write-once, content-addressed, never queried).

```
.ekos/artifacts/
  pack-0000.seg          append-only segment, rolls at 64 MB
  pack-0001.seg
```

The frame index is held in memory and **derived on every open by a
header-only segment scan** (36 bytes read per frame — sub-100 ms at estate
scale). No sidecar index file exists in v1: an index that cannot go stale
beats one that must be validated. A persisted sidecar remains a documented
optimization if open-time scanning ever shows up in profiles.

Frame layout inside a segment (all integers little-endian):

```
[u32 frame_len][32-byte artifact id (raw SHA-256)][zstd(compact canonical JSON)]
```

- Each zstd frame carries its own checksum (`include_checksum`), verified on
  every read; the embedded id guards against misindexed reads. zstd level 3:
  level 19 was measured to take minutes over the estate for ~1.2× more ratio
  — artifact writes happen during builds, so write speed wins. (The *ledger*
  keeps level 19: far fewer rows per build, and the reused compression
  context makes it ~µs/row — a fresh level-19 context costs ~45 ms to
  initialize, so contexts must be cached, not created per row.)
- **Crash safety:** frames are appended then fsynced per write batch. On open,
  if the last frame is torn (length prefix exceeds file end, or id/length
  mismatch), the tail is truncated — everything before it is intact by
  construction. The sidecar index is derived: deleted or stale, it is rebuilt
  by a linear segment scan.
- **Concurrency:** one writer (`ekos build` is already the only writer),
  enforced with a lock file; readers open segments read-only. The MCP
  server/runtime read only the ledger, not artifacts, so there is no reader
  contention today.
- `PackArtifactStore` implements the existing `ArtifactStore` trait — the
  trait was designed exactly for this swap ("what makes the v1.0 backend swap
  a single-crate change"). `FileSystemArtifactStore` remains for
  export/debugging.
- `ekos artifact repack` migrates a loose-object directory into segments and
  removes the loose files after verifying every id reads back identically.
  ArtifactIds are unchanged, so nothing referencing an id notices.

Measured on the live estate: 46,539 loose files → 1 segment + manifests in
4.5 s; **214 MB on disk → 31 MB (6.9×)**; apparent bytes 63.8 MB → 28.4 MB.
The dominant win is block-slack elimination, exactly as the median-707-byte
file size predicted.

## Alternatives considered

- **RocksDB / redb / sled for the ledger** — rejected: reimplements
  branching, SQL, and FTS at a scale (≤1M rows) where SQLite is idle;
  RocksDB adds a heavyweight C++ dependency with nondeterministic background
  compaction.
- **Parquet/Arrow as primary store** — rejected: columnar formats mismatch
  heterogeneous nested JSON and point/graph lookups; immutable Parquet files
  under an append-only log would force a manifest layer (a homegrown
  Delta/Iceberg). Parquet remains a candidate *export* format.
- **Tantivy now** — rejected for this stage: FTS5 rebuilds 22K objects in
  seconds and participates in the ledger's own transaction. Tantivy is the
  designated successor when full-content indexing lands (RFC 0016 §search;
  RFC 0014 non-goal).
- **postcard/bincode as stored form** — rejected: after zstd the marginal
  gain is ~10–20%, not worth a dual-format schema-evolution problem; and it
  must never be the *hashed* form (field-order fragility).
- **Artifacts as blobs in SQLite** (`blobs(id BLOB PK, body BLOB)`) — the
  honest fallback: ~90% of the artifact win in ~50 lines. Rejected in favour
  of Pack v1 because the pack keeps artifacts decoupled from SQLite, streams
  appends without page churn, produces rsync-friendly immutable segments, and
  builds the exact segment machinery RFC 0016 needs.
- **SQLite page-size tuning** — rejected: `dbstat` shows the waste is
  content-level (payload bytes, duplication), not page-level.

## New dependencies

`zstd = "0.13"` (workspace-wide). C library via safe bindings — same
precedent as `rusqlite` `bundled`. No `unsafe` in EKOS code.

## Testing

- Ledger v2: every existing ledger test passes against v2; v1→v2 migration
  round-trip (decoded payloads value-equal, signatures identical, counts
  equal); dictionary version byte honored (frames written with dict 0 and
  dict 1 both readable); FTS searchability preserved across migration.
- Pack v1: write/read/exists/list round-trip; verify-on-read catches a
  corrupted frame; index deleted → rebuilt by scan → identical; torn tail
  (segment truncated mid-frame) → recovered on open with prior frames intact;
  repack of a loose store preserves every id.
- Benchmarks: `storage_compaction.rs` gates — ledger ≥2.5× smaller (measured
  2.5×), artifacts ≥6× smaller on disk (measured 6.9×), total ≥4× on disk
  (measured: 326 MB → ~75 MB); read latency within 10% of v1 (measured:
  `get_object` 11 µs, `find_objects` 740 µs/1K objects, `append_object`
  2.3 ms — commit-fsync-bound, as in v1).
- End-to-end: `ekos build` + MCP `ekos_search`/`ekos_neighborhood`/`ekos_diff`
  return identical results before and after migration on a copy of the real
  estate.

## Non-goals

- Replacing SQLite under the ledger — that is RFC 0016 (fact-segment engine),
  which this RFC's pack format and measurement harness de-risk.
- Semantic/embedding search (RFC 0014 non-goal stands).
- Compressing `fingerprints.json` (8 KB, deliberately human-readable).
