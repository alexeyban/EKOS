# Devlog 18 — RFC 0016 Fact-Segment Engine: accepted, built, gated

**Date:** 2026-07-17
**PRs:** worked on `main` (single session, commits d5482d2…this)
**Branch:** main

---

## Summary

Reviewed and accepted RFC 0016, then implemented the entire fact-segment
engine in one arc: fact model (Phase 1), segment store (Phase 2), index runs
(Phase 3), full `Ledger` API parity (Phase 4), integrated tantivy (Phase 5),
mmap'd reads + run-backed serving (Phase 6), and the finale — `ekos ledger
migrate --v3` plus the `KnowledgeStore` backend seam through runtime, EKL,
CLI, and MCP. The acceptance gate ran against the real estate:
**functional criteria pass** (80,211 versions migrated in 4.5 min, every
signature verified, MCP serves identical answers), **the storage criterion
fails** (98 MB vs the ≤20 MB target) — so the backend flip correctly did
not happen. The live estate stays on the v2 SQLite ledger; the fact engine
is opt-in per workspace via explicit migration.

## The arc, commit by commit

| Commit | Phase | What landed |
|---|---|---|
| d5482d2 | acceptance | RFC review closed 4 gaps: active-segment watermark visibility, numeric fidelity, wall-time→tx mapping, mmap scope |
| afe371e | 1 | `fact.rs`: EAV decompose/reconstruct with byte-parity to `content_signature`; position-indexed evidence refs; dot-escaped paths; semantic diff (1 changed property = 2 facts) |
| d926de2 | 2 | `segment/`: batch frames, fsync-then-HEAD watermark, seal + SHA-256 manifest, torn-tail + stale-watermark recovery |
| 05d177f | 3 | `index.rs`: EAVT/AEVT/AVET runs, order-preserving escaped byte keys, block-pruned prefix scans, LSM merge |
| 81ad1c1 | 4 | `fact_ledger.rs`: full API parity + cross-backend signature-parity test; get_object 3.9× faster than SQLite |
| 02e9438 | 5 | `search.rs`: tantivy with RFC 0014 semantics, lazy group-commit, marker catch-up; find_objects 19× faster than FTS5 |
| 6b95014 | 6 | `segment/map.rs` (the one audited `unsafe`, sealed files only), header-only scans, memtable+runs read architecture |
| (this) | finale | `migrate_to_v3` with per-version signature verification and original timestamps; `KnowledgeStore` trait; runtime/EKL/CLI/MCP backend-agnostic with auto-detection; backend-aware branches |

## Acceptance gate verdict (RFC 0016 §8)

- ✅ Full ledger + runtime + MCP test suites pass on the fact engine
  (parity suite mirrors the SQLite suite case-by-case; 45 workspace suites).
- ✅ Migration preserves complete history with original timestamps; all
  80,211 versions signature-verified byte-for-byte; counts exact.
- ✅ MCP equivalence on the migrated estate copy (status/search/neighborhood).
- ✅ Read latency: get_object 9.5 µs (SQLite 11.2), find_objects 39 µs
  (FTS5 740), append 2.4 ms (parity, fsync-bound).
- ❌ **Storage: 39.1 MB (v2) → 98.2 MB (0.4×)** against a ≥2×-smaller target.
  Breakdown: segments 30 MB, **index runs 59 MB**, tantivy 10 MB. The runs
  store full covering entries — every value, including 600-char excerpts —
  JSON-encoded in *all three* sort orders. This is precisely the RFC §7
  compression work (dictionary-zstd fact batches, prefix/delta-encoded
  index blocks, slim per-order projections) that Phases 2–3 deferred.

**Consequence:** the default backend stays SQLite. `migrate --v3` is an
explicit, reversible opt-in (delete `facts/` to roll back); auto-detection
flips only migrated workspaces. §7 compression is the single outstanding
item before the flip can be reconsidered.

## Knowledge Captured

- **A "covering index ×3 sort orders" triples your values.** EAVT needs
  values for reconstruction; AEVT/AVET reads in practice only consume the
  entity id. Slim per-order projections (and putting the AVET value only in
  the key) are not an optimization, they're the difference between 26 MB
  and 59 MB of indexes. Design indexes by what reads *consume*, not by
  symmetry.
- **The attribute registry must be durable the moment it grows**, not at
  seal time — facts referencing a fresh AttrId orphan on crash-reopen
  otherwise. Found by the reopen test in minutes; would have been a
  production data-loss bug.
- **Lazy group-commit is the tantivy integration pattern**: buffer upserts
  in the writer, commit on first query after a write. Appends pay nothing,
  search stays read-your-writes, and a `last_tx` marker makes open-time
  catch-up incremental.
- **fsync dominates migration** (2 atomic writes per version → 4.5 min for
  80K versions). A batched-fsync migration mode (group N versions per
  commit) is the obvious lever if this ever matters.
- **mmap scoping worked as designed**: sealing gives a natural safe/unsafe
  boundary — one `unsafe` expression in one module, everything else reads
  `&[u8]`. The active segment is `pread`-only and recovery can truncate it
  without invalidating any map.
- Criterion + a size report per phase caught every regression the same day
  it was written (45 ms zstd contexts, the 59 MB indexes). Measurement-first
  is the reason the gate failure is a diagnosis, not a mystery.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/ledger/src/fact.rs` | new — EAV fact model (Phase 1) |
| `ekos/crates/ledger/src/segment/{mod,map}.rs` | new — batch segments, watermark, mmap (Phases 2, 6) |
| `ekos/crates/ledger/src/index.rs` | new — EAVT/AEVT/AVET runs (Phase 3) |
| `ekos/crates/ledger/src/search.rs` | new — tantivy integration (Phase 5) |
| `ekos/crates/ledger/src/fact_ledger.rs` | new — FactLedger, API parity + run-backed reads (Phases 4, 6) |
| `ekos/crates/ledger/src/lib.rs` | KnowledgeStore trait, merge_stores, export_versions, migrate_to_v3 |
| `ekos/crates/runtime/src/lib.rs` | Runtime over `&dyn KnowledgeStore` |
| `ekos/crates/cli/src/commands/store.rs` | new — backend auto-detection |
| `ekos/crates/cli/src/commands/{build,commit,query,ask,ekl,diff,branch,ledger,mcp}.rs` | trait consumption; backend-aware branches/status; `migrate --v3` |
| `benchmark/benches/{fact_model,segment_store,index_runs,fact_ledger}.rs` | new — per-phase gates |
| `docs/rfcs/0016-fact-segment-engine.md` | Accepted + review notes + Phase 1 refinements |
