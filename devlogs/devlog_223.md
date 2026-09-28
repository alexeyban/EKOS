# Devlog 223 — An audit gate that runs even under `[skip ci]`, and where point reads actually go

**Date:** 2026-09-28
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

Two follow-ups from the devlog_222 audit. First, `cargo audit` is now a CI workflow (push, PR and
weekly) and a local script that checks all three Cargo workspaces plus lockfile freshness,
negative-tested against the lockfiles that had actually gone bad. Second, the slow graph reads were
profiled down to their cause, which was not the one devlog_222 guessed. The fix changes the on-disk
index format, so it is written up as RFC 0168 (Draft) instead of coded.

---

## Audit gate

| Piece | What it does |
|---|---|
| `scripts/audit.sh` | For `ekos/`, `tests/integration/`, `benchmark/`: `cargo metadata --locked` (stale lockfile) + `cargo audit` (fails on vulnerabilities; unmaintained warnings reported, not fatal) |
| `.github/workflows/audit.yml` | Runs the script on push, PR, **weekly** and on demand. The schedule catches new advisories against unchanged code. `cargo-audit` is installed from crates.io at a pinned version (`0.22.2 --locked`), not through a third-party action. |

**Negative-tested:** with the pre-devlog_222 lockfiles put back temporarily, the script failed. It
named RUSTSEC-2026-0285 in two workspaces and the stale `tests/integration` lockfile. The lockfiles
were then restored.

`ci.yml` was **not** edited. The maintainer's standing instruction is to leave it intact, and a
`--locked` change there would duplicate what the script already checks. An edit to add `--locked`
was made, noticed against that instruction, and reverted before commit.

---

## Where a point read goes

devlog_222 bounded MCP traversal and blamed per-edge *reconstruction*. Profiling says otherwise.
`perf` is unavailable here (`perf_event_paranoid = 4`), so a scratch harness outside the repo called
the public `ekos_ledger` API on a copy of EKOS's own index runs:

| Measurement (release) | Value |
|---|---|
| `get_relationship` / `get_object`, warm | ~1.47 ms each |
| EAVT entity scan, 8 runs | 1.50 ms |
| Same data merged to 1 run | **0.19 ms** warm |
| `merge_runs(Eavt)` 8 → 1 | **151 s** |

The cost is about 0.19 ms **per run probed**. Entity ids are random UUIDs, so some block in every
run straddles any id. That block is opened, read, zstd-decoded and decoded (512 entries) whether or
not the run holds the entity. Nothing is cached, which is why warm equals cold. RFC 0016 specified
mmap'd runs; the code reads through a fresh file handle per block.

**RFC 0168** proposes: a per-run entity Bloom filter (an optional directory field, so old runs keep
working, just unfiltered), blocks read through the existing audited map, a decoded-block cache,
binary search over the block directory, and a separate look at the 151 s merge.

---

## Knowledge Captured

- **The second hypothesis needed an experiment too.** The first guess (quadratic dedup) was fixed
  and measured as irrelevant in devlog_222. The second guess (reconstruction) was plausible from
  the code and wrong. Only the merge-a-copy experiment separated "per run" from "per entity".
- **A writable `FactLedger::open` on a copy without `indexes/` does not rebuild runs.** It replays
  everything into the memtable, and every lookup then scans the memtable linearly (3.5 ms here).
  To test a runs hypothesis, drive `FactIndexes` directly.
- **Test a gate against the failure it exists for.** An audit script that has only ever exited 0
  proves nothing; restoring the bad lockfiles did.
- **`[skip ci]` needs a local twin for every check CI would have done.** The saved workflow memory
  now lists `scripts/audit.sh` in the local gate, and the stale "don't use `clippy --all-targets`"
  note was corrected; `--all-targets` is clean and is what CI runs.

---

## Files Changed

| File | Change summary |
|---|---|
| `scripts/audit.sh` | New — RustSec + stale-lockfile check over all 3 workspaces |
| `.github/workflows/audit.yml` | New — push / PR / weekly / manual |
| `ekos/docs/rfcs/0168-index-run-point-lookups.md` | New — Draft, with measurements |
| `CLAUDE.md` | `scripts/audit.sh` in Commands; RFC 0168 / devlog_223 status |
| `TODO.md` | Audit gate and lockfile-drift items ticked; point-read item rewritten around RFC 0168 |
