# RFC 0229 — zstd advisories: tantivy 0.22 → 0.27, parquet 53 → 60

**Status:** Accepted
**Date:** 2026-10-26
**Repos:** EKOS (all three workspaces); the extensions follow through their lockfiles

## 1. Problem

Three RustSec advisories published on 2026-10-16 fail `cargo audit` (and the push-triggered Audit workflow) since
2026-10-26:

| Advisory | Crate | Fixed in |
|---|---|---|
| RUSTSEC-2026-0391 — data from an abandoned stream leaks into the next stream on a reused context | `zstd` 0.13.3 | ≥ 0.14.3 |
| RUSTSEC-2026-0403 — uninitialised memory exposed when streaming decompression of a legacy frame fails | `zstd-safe` 7.2.1 | ≥ 8.2.0 |
| RUSTSEC-2026-0410 — the WebAssembly allocator shim corrupts memory on allocation failure | `zstd-sys` 2.0.13 | ≥ 2.1.2 |

EKOS uses `zstd` directly (artifact store, ledger segments, home backups), and two dependencies pin the old line:
`parquet` 53 (the crypto plugin's export) and `tantivy` 0.22 (the ledger's search index, through
`tantivy-sstable`). `zstd-sys` links a native library, so only one version can be in a build: all three must move.

## 2. Design

- `zstd` 0.13 → 0.14.3 (workspace). The compressed format is zstd's own frame format, unchanged: everything already
  written (artifacts, segments, backups) decompresses as before.
- `parquet` 53 → 60 in `plugins/crypto` (`default-features = false`, `snap` + `zstd`); the API it uses is unchanged.
- `tantivy` 0.22 → 0.27. One API change: `TopDocs::with_limit(n)` is a builder; the score-ordered collector is
  `.order_by_score()`.

### The search index on disk

tantivy 0.27 writes index format 7 and reads formats 4–7; 0.22 wrote format 6. So:

- **Upgrading needs nothing.** An existing ledger's index opens, answers the same queries with the same ranking,
  and takes new segments; merges rewrite old segments in the new format over time. `CONTENT_VERSION` is unchanged
  (what is indexed did not change), so no replay is triggered.
- **Downgrading loses search, not data.** An older EKOS opening an index that 0.27 wrote cannot read it; it logs a
  warning and runs without search (counts, object reads and EKL keep working). Rebuilding the index with the older
  binary restores it.

## 3. Verification

| Check | Result |
|---|---|
| a copy of a ledger written by 0.22 (`docsonly`, 26,989 entries, 6 segments), four queries, old vs new binary | identical output, ranking included |
| the full pipeline on that copy with the new binary, one new page in the source | `build`…`commit` ok, 0 model calls; the new page found, the old pages still found (39 results before and after); 27,001 entries |
| `ekos ledger repair` on the upgraded copy | clean |
| the old binary on the upgraded copy | warning, search unavailable, no crash |
| `scripts/audit.sh` (all three workspaces) | passes |
| workspace build, clippy `-D warnings`, tests | clean; 3,189 passed in 182 test binaries, integration 5 passed, benchmarks build |
| `cargo bench --bench fact_ledger`, same machine, before → after | `find_objects` 61.1 µs → 35.9 µs (−41%); `get_object` −7%; `append_object` −1% |
