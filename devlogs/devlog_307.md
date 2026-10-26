# Devlog 307 — zstd advisories: tantivy 0.27, parquet 60 (RFC 0229)

**Date:** 2026-10-26
**PRs:** none (direct to `main`)
**Branch:** `main`

---

## Summary

Three RustSec advisories against the zstd crates (published 2026-10-16) turned the Audit workflow red on the
RFC 0218 push. `zstd-sys` links a native library, so one version per build: `zstd` 0.14.3 needed `parquet` 53 → 60
(crypto plugin) and `tantivy` 0.22 → 0.27 (the ledger's search index, through `tantivy-sstable`). One source change.
An existing ledger's index opens and answers identically; search got 41% faster on the benchmark.

---

## RFC 0229 — the upgrade

### What was built

| Component | Change |
|---|---|
| `ekos/Cargo.toml` | `zstd` 0.14.3, `tantivy` 0.27 |
| `plugins/crypto/Cargo.toml` | `parquet` 60 |
| `ledger/src/search.rs` | `TopDocs::with_limit(n).order_by_score()` |
| `benchmark/`, `tests/integration/` lockfiles | the same versions (`scripts/audit.sh` checks all three) |
| `docs/deployment.md` | the downgrade note |

### Decisions
- **No index rebuild.** tantivy 0.27 reads formats 4–7 (0.22 wrote 6); `CONTENT_VERSION` stays, since what is
  indexed did not change. Old segments are rewritten as merges happen.
- **Verified on a real ledger copy**, not only on fixtures: a 26,989-entry ledger written by 0.22 gave identical
  search output (ranking included) to both binaries, then took a full pipeline run with the new one.

---

## Knowledge Captured

- **`zstd-sys` is a `links` crate**: two `zstd` major lines cannot coexist in one build, so a zstd advisory pulls
  every dependency that pins it (here parquet and tantivy) along.
- **tantivy ≥ 0.25: `TopDocs::with_limit` is a builder** — `.order_by_score()` gives the old collector. It is the
  only break EKOS hit from 0.22 to 0.27.
- **An older EKOS on a newer index** does not crash: it logs "search index unavailable on a read-only filesystem"
  (wrong reason — it is the format) and runs without search.
- `tantivy` has no `INDEX_FORMAT_VERSION` in `meta.json`; it is in each segment file's footer.
- Benchmark before → after (`fact_ledger`): `find_objects` 61.1 → 35.9 µs, `get_object` 9.36 → 8.88 µs,
  `append_object` 2.48 → 2.45 ms.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/Cargo.toml`, `ekos/Cargo.lock`, `ekos/plugins/crypto/Cargo.toml` | versions |
| `ekos/crates/ledger/src/search.rs` | `order_by_score()` |
| `benchmark/Cargo.lock`, `tests/integration/Cargo.lock` | versions |
| `ekos/docs/rfcs/0229-zstd-advisories-tantivy-parquet-upgrade.md` | new RFC |
| `docs/deployment.md`, `TODO.md`, `CLAUDE.md` | downgrade note, tick, latest devlog/RFC |
