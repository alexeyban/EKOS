# Devlog 17 — Compact Storage (RFC 0015) + Fact-Segment Engine Design (RFC 0016)

**Date:** 2026-07-17
**PRs:** worked on `main` (single session)
**Branch:** main

---

## Summary

Answered "how do we make storage compact and effective — should EKOS have its own
database?" with a two-stage plan agreed with the user. Stage 1 (RFC 0015, fully
implemented this session): compress the existing stack — the live estate's `.ekos`
drops from **326 MB to ~75 MB on disk** (ledger 99→39 MB via dictionary-zstd BLOBs,
artifacts 214→31 MB via an owned packfile format, snapshots/CKM zstd'd with
retention). Stage 2 (RFC 0016, design accepted as draft): a custom fact-segment
engine — EAV facts, immutable content-addressed segments, EAVT/AEVT/AVET LSM
indexes, integrated tantivy, mmap'd reads — built later behind the unchanged
`Ledger` API. Also de-hardcoded all machine-local paths from skills/docs
(`WORKSPACE_ROOT`/`EKOS_BIN` variable convention, `EKOS_WORKSPACE`/`EKOS_CONFIG`
env support in the CLI). Bonus: found and structurally fixed a live FTS
duplication bug.

---

## RFC 0015 — Compact Storage (implemented)

### Problem / motivation

Live estate: 326 MB for 22K objects — 214 MB of it 46,539 loose pretty-printed
JSON files with a *median size of 707 bytes* (each burning a 4 KB ext4 block),
plus a 101 MB SQLite ledger storing uncompressed JSON TEXT rows and a
duplicated FTS shadow table. Evaluated and rejected Parquet/Arrow (columnar
mismatch for heterogeneous JSON + point/graph lookups), RocksDB (reimplements
branching/SQL/FTS at a scale where SQLite is idle), tantivy-now (FTS5 rebuilds
22K objects in seconds *inside the ledger transaction*).

### What was built

| Component | Change | Measured result |
|---|---|---|
| `ekos ledger status --storage` | per-component bytes + `dbstat` per-table breakdown | the before/after instrument |
| Free wins | compact JSON everywhere machine-read; `model.json.zst`; snapshot retention (keep 10) | snapshots 6.6 MB→~1 MB; ckm 4.2 MB→~0.7 MB |
| Ledger schema v2 | payload → BLOB `[dict_version][zstd]` with corpus-trained dictionary in `meta`; sig hex→BLOB(32); id UUID→BLOB(16); timestamps→INTEGER µs; FTS5 `contentless_delete=1` keyed by entry rowid | 99 MB→39.1 MB (2.5×), 80,211 rows migrated in 12 s, round-trip verified per row |
| `ekos ledger migrate` | streams v1→v2 into temp file, verifies, atomic swap, leaves `.bak` | history preserved (migration, not rebuild) |
| EKOS Pack v1 | `pack-NNNN.seg` frames `[u32 len][32B sha256][zstd+checksum]`, index derived by header scan on open, torn-tail truncation, loose-file read fallback | 214 MB disk→31 MB (6.9×), 46,539 files→1 segment, 4.5 s |
| `ekos artifact repack` | pack-all → fsync → delete-loose (crash-safe ordering) | verified read-back per artifact |
| Benches | `storage_compaction.rs` (bytes/1K objects + read latency) | `get_object` 11 µs, `find_objects` 740 µs/1K, `append_object` 2.3 ms |

All construction sites switched to `PackArtifactStore` (build/compile/recover/
resolve/`PassContext::new` default, loose fallback on scan failure).

### Decisions

- **Identity hashes canonical JSON, never stored bytes.** Compression applies
  after hashing; every ArtifactId/content_sig stays byte-identical. Rejected
  postcard/bincode as hashed form — serde binary layouts follow struct field
  order, so a field reorder would silently change every hash.
- **Ledger zstd level 19, pack level 3.** Ledger rows are few per build and the
  reused context makes 19 cost ~µs/row; packing 46K artifacts at 19 took
  *minutes* for ~1.2× more ratio vs 4.5 s at level 3.
- **No pack sidecar index.** Header-only scan on open (36 B/frame) is fast at
  estate scale and cannot go stale.
- **Migration, not rebuild** — `ekos build` from scratch is cheap via
  fingerprints.json but would lose append-only history.

## RFC 0016 — Fact-Segment Engine (design, draft)

The "own database", designed where owning pays: EAV facts (a changed property
stores ~40 bytes, not the whole payload — the estate has 1.9× version churn),
immutable content-addressed segments with manifest-based **O(1) copy-on-write
branches** (vs `VACUUM INTO`), EAVT/AEVT/AVET index runs (graph BFS becomes
ranged scans instead of N queries/hop; relationships get true history),
tantivy committed per sealed segment (unlocks full-content search), mmap'd
sealed segments (formal `unsafe` justification drafted), `content_sig`
redefined as hash of the canonical-JSON *reconstruction* (byte-compatible with
v2 → migration verifiable). Ships behind the existing `Ledger` API only after
passing the entire current test suite; acceptance gate written into the RFC.

## Path de-hardcoding

- Skills (`.claude/skills/{memory,ekos-knowledge}` + synced to
  `~/.claude/skills/`): all paths now derive from
  `WORKSPACE_ROOT` (nearest ancestor with `ekos.toml`, overridable via env),
  `EKOS_ROOT`, `EKOS_BIN` (`command -v ekos` first), `MEMORY_DIR`, `LEDGER_DB`.
- CLI: `EKOS_WORKSPACE` and `EKOS_CONFIG` env vars — `ekos mcp serve` needs no
  path args when spawned by an agent host with env set.
- devlog_14 references rewritten to `$WORKSPACE_ROOT` phrasing; `git ls-files |
  xargs grep` for absolute paths is clean.
- The user-scope MCP registration in `~/.claude.json` still contains machine
  paths — that file *is* machine-local config, the right place for them; the
  env-var support makes even that optional now.

---

## Knowledge Captured

- **FTS5 `INSERT OR REPLACE` does not replace.** FTS5 tables have no unique
  constraints, so v1's reindex-on-append silently duplicated rows: the live
  index held 44,796 rows for 22,023 objects and searches returned duplicates.
  v2's contentless index keyed by entry rowid cannot duplicate. If you think
  you're upserting into FTS5, you're inserting.
- **zstd level-19 context creation costs ~45 ms** (match-table allocation) —
  `zstd::encode_all(data, 19)` per small row is a disaster (measured 45.9 ms/
  append). Reuse a `bulk::Compressor` (or a prepared `EncoderDictionary`,
  which embeds the tables) → 2.3 ms/append, i.e. compression cost vanishes
  under the commit fsync.
- **Row-by-row compression ≠ corpus compression.** The concatenated ledger
  corpus compresses 6.2×; the same rows compressed individually reach ~2.3×
  even with a trained dictionary. Cross-record redundancy needs a structural
  change (RFC 0016 fact decomposition), not a better compressor.
- **Sub-millisecond timestamps matter for history.** v2 initially stored
  unix *milliseconds*; two versions written in one build tick collided and
  `object_at` returned an arbitrary one (caught by an existing test). Fixed
  with microseconds + rowid tiebreak; RFC 0016 replaces wall-clock ordering
  with a monotone TxId — the structural fix.
- **The filesystem was the biggest waste, not the data.** 46,539 files with
  median 707 B ≈ 3.2× block-slack overhead — `du` said 214 MB where file
  bytes were 63.8 MB. Measuring apparent vs on-disk separately (the new
  `--storage` report shows apparent; `du` shows blocks) prevented optimizing
  the wrong thing.
- **Two `PackArtifactStore` instances over one directory is a correctness
  bug** (each caches segment offsets; the other's appends invalidate them).
  Commands must share one store instance with their `PassContext` — done in
  compile/recover; keep this invariant when adding commands.
- Live-estate migration is **not yet run** — everything was validated on
  copies. `ekos ledger migrate` + `ekos artifact repack` on the real
  workspace is a user decision (both leave backups/are verify-first).

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0015-compact-storage.md` | new — accepted, measured results inlined |
| `docs/rfcs/0016-fact-segment-engine.md` | new — draft engine design |
| `ekos/crates/ledger/src/lib.rs` | schema v2 (dict-zstd BLOBs, contentless FTS, µs timestamps), `migrate_to_v2`, `storage_stats`, cached compressor |
| `ekos/crates/artifact/src/pack.rs` | new — Pack v1 segments + repack |
| `ekos/crates/artifact/src/store.rs` | compact JSON, `StoreError::Corrupt` |
| `ekos/crates/common/src/compress.rs` | new — zstd-JSON helpers (`.zst` + legacy fallback) |
| `ekos/crates/semantic/src/lib.rs` | `model.json.zst` writer |
| `ekos/crates/cli/src/commands/{build,compile,recover,resolve}.rs` | pack store adoption; snapshot compression + retention |
| `ekos/crates/cli/src/commands/{ledger,artifact}.rs` | `status --storage`, `migrate`, `repack` |
| `ekos/crates/cli/src/bin/ekos.rs` | new subcommands; `EKOS_WORKSPACE`/`EKOS_CONFIG` env |
| `ekos/crates/compiler-core/src/pass.rs` | pack store default with loose fallback |
| `benchmark/benches/storage_compaction.rs` | new — size + latency instrumentation |
| `.claude/skills/*/SKILL.md`, `devlog_14.md` | path variables instead of absolute paths |
