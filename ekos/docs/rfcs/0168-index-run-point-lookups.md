# RFC 0168 — Fact-index point lookups: skip runs that cannot match, and stop decoding what is thrown away

**Status:** Draft
**Date:** 2026-09-28
**Related:** RFC 0016 (fact-segment engine, §4 index runs), RFC 0080 (storage plan), RFC 0106
(checkpoints), devlog_222 (the MCP traversal bound this makes less necessary)

---

## Summary

Every point read in the fact engine (`get_object`, `get_relationship`, `relationships_for`, every
graph hop) costs about **0.19 ms per index run**. It pays that for runs that do not contain the
entity at all. With 8 EAVT runs on EKOS's own ledger, a single entity lookup costs **1.5 ms** in a
release build. A 3-hop neighborhood around one hub took 28 s.

This RFC adds a per-run entity filter so a lookup reads only runs that can match. It also makes
three smaller changes: read blocks through the mapping RFC 0016 already specified, cache decoded
blocks, and bound the merge that currently stalls a `commit` for minutes.

## Measurements (EKOS's own ledger, 177 MB, release build, 2026-09-28)

A scratch harness called the public `ekos_ledger` API against a copy of the real index runs, using
the 133 relationship ids around one hub object.

| What | Result |
|---|---|
| `get_relationship` / `get_object`, warm | 1.47 / 1.48 ms each |
| `relationships_for(hub)` (133 edges) | 200 ms warm; the candidate AVET scan is negligible, the per-edge rebuild is not |
| EAVT entity scan, **8 runs** | **1.50 ms** per entity |
| EAVT entity scan, same data merged to **1 run** | **0.19 ms** warm, 0.72 ms cold |
| `merge_runs(Eavt)`, 8 → 1 | **151 s** |
| Rebuilding from the scanned facts (`fold_state` + `reconstruct`) | negligible next to the scan |

So the cost is linear in runs probed, about 0.19 ms each. Where that time goes, from
`index.rs::IndexRun::scan` / `read_block_raw`:

1. Entity ids are random UUIDs, so in **every** run there is some block whose key span straddles
   the id. That block is read whether or not the run holds any fact about the entity. An entity
   written in one build lives in one or two runs; the other six or seven probes are pure waste.
2. Each probe does `File::open` + `seek` + `read_exact` + `zstd::decode_all` + `decode_block` of a
   **512-entry** block, then keeps the handful of entries matching the prefix.
3. The block directory is walked linearly with hex-string comparisons on every scan.
4. Nothing is cached, so a warm lookup costs the same as a cold one (1.498 vs 1.493 ms).

RFC 0016 §4 specified that runs are opened via `memmap2`. The implementation reads through a fresh
file handle per block instead, so the design and the code have diverged.

## Design

### 1. Per-run entity filter (the main fix)

At write time (`write_run_raw`), each EAVT run gets a **split-block Bloom filter over the entity
ids it contains**, stored in the run directory as an optional field. `IndexRun::scan` for an
`Entity` prefix checks the filter first and returns empty without reading any block when the
filter says no.

- **False positives only cost time.** A filter that says "maybe" falls through to today's read, so
  correctness never depends on the filter.
- **Size:** about 10 bits per distinct entity (1% false-positive rate). EKOS's ledger has on the
  order of 10⁵ entities, so tens of KB per run.
- **Backward compatible:** the field is `Option`. A run written before this RFC has no filter and
  is always probed, exactly as today. No migration is needed; runs acquire filters as they are
  rewritten by a flush or merge.
- AVET (`relationships_for`'s candidate scan) gets the same treatment keyed on `(attr, value)` only
  if measurement shows it matters. Today it does not.

**Expected effect:** a lookup probes about 1 run instead of 8, so roughly **1.5 ms → about 0.2
ms**, the merged-run number, *without* paying for the merge.

### 2. Read blocks through the mapping RFC 0016 specified

Map each run once at open, as `ledger/src/segment/map.rs` already does for segments (the crate's
one audited `unsafe` surface; reuse it, add none). A block read becomes a slice. This removes the
per-probe `open`/`seek`/`read`.

### 3. Decoded-block cache

A small LRU of decoded blocks per run, bounded by entry count, in the long-lived read handles
(`ekos mcp serve`, the console). It helps repeated and nearby lookups. It is not the main fix,
because a traversal touches mostly new entities.

### 4. Binary search over the block directory

Blocks are sorted, so replace the linear walk with `partition_point` on `last_key`. This is a small
constant-factor gain, and it matters more as runs grow.

### 5. Bound the merge

151 s for an 8 → 1 EAVT merge is paid inline by whichever `commit` crosses `MERGE_RUNS_AT`. Profile
it separately before changing policy: the suspects are `all_raw` decoding every block,
`Vec<RawRecord>` sorting by owned byte keys, and hex-encoded directory keys. The remedies are a
streaming k-way merge (the inputs are already sorted) instead of collect-then-sort, or a size-tiered
policy. With (1) in place, run count stops mattering for reads, so merges can happen less often.

## Non-goals

- No change to facts, segments, the manifest, or any `KnowledgeStore` semantics.
- No new `unsafe` (item 2 reuses the existing audited map).
- MCP traversal bounds (devlog_222) stay; they bound response size, not just time.

## Test plan

- **Equivalence:** for random entity ids, including absent ones, a filtered scan returns exactly
  what an unfiltered scan returns, across 1, 2 and 8 runs and with pre-RFC runs (no filter) mixed in.
- **Filter hygiene:** a planted false negative (a filter built without one entity) must be caught
  by the equivalence test. That is the test of the test.
- **Benchmark:** a new `benchmark/benches/index_point_lookup.rs` measuring entity scans over 1 vs 8
  runs, with and without filters, on a synthetic ledger sized like EKOS's own. The acceptance gate
  is that the 8-run filtered scan comes within 1.5× of the 1-run scan.
- **Live:** re-run the devlog_222 harness on EKOS's ledger. Target: `get_relationship` under 0.3
  ms warm, and depth-3 neighborhood (still bounded) at least 5× faster.

## Open questions

1. Bloom filter implementation: an existing crate (a new dependency, which then needs `cargo audit`
   coverage) or about 60 lines in-tree? The in-tree version is deterministic and trivially
   serializable, so it is the leaning.
2. Should `ekos ledger repair` offer an explicit "rewrite runs with filters" so an existing large
   ledger benefits immediately rather than at its next merge?
