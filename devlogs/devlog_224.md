# Devlog 224 — RFC 0168: point reads 26× faster on an unmerged index, and a 150 s write that was a zstd level

**Date:** 2026-09-28
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main (the 20 commits through devlog_223 were pushed first, at the maintainer's request)

---

## Summary

RFC 0168 was accepted with the in-tree filter option and implemented the same day. Three changes
to the fact-index read and write paths:

- a per-run entity Bloom filter;
- a lazily decoded block scan;
- zstd level 9 instead of 19 for run blocks.

Together they cut an 8-run entity scan from 1,083 µs to 41 µs in the benchmark, which is as fast as
a fully merged index. Rewriting runs went from 150 s to 4.5 s. No semantics changed; the on-disk
change is one optional directory field that older binaries ignore.

---

## What was built

| Change | Effect | Format |
|---|---|---|
| `EntityFilter` — 10 bits/entity, 7 probes, SplitMix64 over UUID bytes, double hashing | An EAVT scan skips a run whose filter excludes the entity, with no block read | Optional `entity_filter` in the run directory; absent = always probe |
| `decode_block_where` + `Keep::{Yes,No,Stop}` | Values are parsed only for records inside the prefix; decoding stops past it | None — speeds up existing ledgers immediately |
| `partition_point` over the block directory | Jumps to the first candidate block instead of walking from the start | None |
| `RUN_ZSTD_LEVEL` 19 → 9 | Run writes 33× faster, +3% size | Readers are level-agnostic |
| `index_eavt_entity_scan_8_runs` bench | The RFC's gate, measured in-tree | — |

Deferred, because the gate was met without them: mmap'd blocks (§2) and a decoded-block cache (§3).

## Measurements

| | Before | After |
|---|---|---|
| Bench: entity scan, 1 run | 137 µs | 40 µs |
| Bench: entity scan, 8 runs | 1,083 µs | **41 µs** (1.01× the merged scan; gate ≤1.5×) |
| Bench: AVET ref lookup | 81 µs | 43 µs |
| EKOS ledger, 8 runs, existing unfiltered | 1.43 ms | 0.50 ms |
| EKOS ledger, 8 runs, rewritten with filters | — | 0.171 ms |
| Rewrite 8 EAVT runs | 150 s | 4.5 s |
| MCP `ekos_neighborhood` depth 2, release | 3.56 s | 1.45 s (≈0.4 s of that is startup) |

The "before" benchmark numbers came from the same new bench file run against the original
`index.rs` (stashed), not from memory.

---

## Tests (written before the implementation was run)

- No false negatives in 20,000 present ids, fewer than 2% false positives in 20,000 absent ids.
- Deterministic: the same entities produce identical filter bits, and the filter survives a
  directory round trip.
- **Equivalence and skipped reads:** 8 filtered runs vs the same 8 unfiltered, for present and absent
  ids, give identical results; the filtered set reads at least 4× fewer blocks. A test-only
  `blocks_read` counter proves the skip, not just the answer.
- Pre-RFC and filtered runs mixed in one set scan correctly.
- **The test of the test:** a filter planted with one entity missing makes the equivalence check
  fail. The plant always takes, because the test searches for a victim that isn't a false positive
  rather than returning early.
- A malformed filter disables filtering instead of failing the open.

---

## Knowledge Captured

- **The draft missed the biggest single win.** RFC 0168 was written around "skip runs". Measuring
  the filter alone gave 2.9×, not the predicted 8×, because entities re-written across builds live
  in about 2.6 runs, and each probe still parsed 512 JSON values. Lazy decode was found by asking
  what one probe costs after the filter landed.
- **The "slow merge" was never the merge.** Rewriting runs without merging took the same 150 s. The
  comment "written once, spend effort there" was wrong: a merge is a rewrite, paid inline by a
  `commit`.
- **Benchmark noise here is about ±8% between identical consecutive runs.** Compare against a real
  baseline (stash the change), never against criterion's "change" line from the previous run.
- **Filters arrive with the next write of each run.** Existing ledgers get lazy decode now and
  filters at their next seal or merge; an explicit rewrite command is open question 2.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/ledger/src/index.rs` | `EntityFilter`, `decode_block_where`/`Keep`, binary-searched directory, zstd 9, test-only read counter, 7 tests |
| `benchmark/benches/index_runs.rs` | `build_indexes(runs)`; new `index_eavt_entity_scan_8_runs` |
| `ekos/docs/rfcs/0168-index-run-point-lookups.md` | Accepted; Implementation section with measurements |
| `TODO.md`, `CLAUDE.md` | RFC 0168 ticked with follow-ups; devlog pointer |
