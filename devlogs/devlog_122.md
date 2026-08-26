# Devlog 122 — RFC 0105: Storage Architecture Phase 2 (WAL Recognition + Repair Tool)

**Date:** 2026-08-26
**PRs:** RFC 0105
**Branch:** main (direct)

---

## Summary

The second implementation phase of RFC 0080's storage architecture plan. RFC 0080's own
investigation had already found there's no new WAL to build — `FactLedger`'s existing segment
format already provides real, ledger-level write-ahead-log durability. The real, concrete gap was
narrower: no tool surfaced any of it. `ekos ledger repair` is now that tool — it opens the ledger
(triggering the two free self-heals that already existed but were never exercised outside unit
tests) and reports every sealed segment's integrity individually, replacing `TODO.md`'s previously
accurate "the only recovery option is a full migration rollback" with a real, precise diagnostic.

---

## RFC 0105 — WAL Recognition + Repair Tool

### Problem / motivation

RFC 0080 scoped this phase as "a WAL + repair tool" but its own text already concluded the WAL
half doesn't need building — the real gap is that `SegmentStore::verify_sealed` exists, is
unit-tested, and was never called from any real command.

### What was built

| Component | Change |
|---|---|
| `SegmentStore::verify_sealed_report` | Checks every sealed segment unconditionally, one row per segment, instead of stopping at the first failure |
| `SealedSegmentCheck` | `{ seq, tx_min, tx_max, ok, detail }` — the report row shape |
| `FactLedger::verify_sealed_report` | Thin wrapper exposing the above through the ledger's own public API |
| `ekos ledger repair` | New CLI subcommand: opens the ledger, prints one line per sealed segment, summarizes, fails with a real error naming the affected count when any segment is corrupt |

### Implementation details worth remembering

- **`verify_sealed` (existing, tested) was refactored to sit on top of the new report, not
  duplicated.** `verify_sealed` still fails fast at the first bad segment (its existing contract,
  used by two existing tests that only check `Ok`/`Err(Corrupt(_))`), but now does so by calling
  `verify_sealed_report().into_iter().find(|c| !c.ok)` — one place decides what "a segment fails
  verification" means, not two independently-maintained checks that could silently drift from each
  other over time (the exact bug shape `CLAUDE.md` already calls out by name elsewhere in this
  codebase).
- **A real, live discovery while testing this**: seeding a fixture with sealed segments but never
  querying it (`find_objects`) leaves the search index's own internal `last_tx` marker unwritten —
  RFC 0016's own module doc already says commits are lazy ("on the first query after a write"), but
  the consequence for *this* RFC wasn't obvious until hit directly: reopening such a ledger forces
  a full replay of *every* sealed segment's raw body just to catch the search index up (a
  completely separate mechanism from the index-runs marker, which correctly avoided re-reading old
  segments in the same scenario). A corruption test built without accounting for this failed for
  the wrong reason — `FactLedger::open` itself erroring during the forced full replay, before
  `repair()`'s own segment-by-segment check even got a chance to run — rather than the scenario the
  test was meant to exercise (a clean open, then `repair()`'s own check finding the real problem).
  Fixed by calling `find_objects` once during fixture setup, matching a pattern `fact_ledger.rs`'s
  own tests (RFC 0097) already established for exactly this reason.
- **`repair()`'s error message when `FactLedger::open` itself fails (rather than a clean open plus
  a bad `verify_sealed_report` row) is still honest and actionable** — it surfaces the real
  `LedgerError` text (e.g., "sealed segment N has invalid frames at offset 0") rather than needing
  new machinery to force an open through such corruption. Building that extra resilience was
  considered and deliberately not attempted — RFC 0105's real scope is surfacing existing
  primitives, not inventing new self-healing-through-corruption behavior beyond what already
  exists.

### Decisions (alternatives considered, why this choice)

- **No automatic fix for a genuinely corrupt sealed segment.** There's no redundancy anywhere in
  this format to reconstruct lost bytes from — reporting precisely (which segment, which
  transaction range) so a human can decide (restore from backup, or accept the loss) is this RFC's
  real, honest scope, not a `--fix` flag that would have nothing real to do for the one case that
  actually matters.
- **FactLedger-only, no SQLite equivalent.** Matches every prior phase's precedent (RFC 0102, RFC
  0104) of not doubling scope onto the backend already being phased out — SQLite already has its
  own `PRAGMA integrity_check` for the analogous job.

---

## Knowledge Captured

- **A fixture that "should" be settled (writes done, handle about to be dropped) can still have
  real unflushed state if the code's own durability contract is lazy by design** — RFC 0016's
  "commit lazily on the first query after a write" is documented, but its consequence for a test
  fixture that never queries (a directly plausible thing to write, since nothing about writing
  objects obviously implies you also need to search for one) wasn't obvious until it silently
  changed the shape of a failure the test was meant to produce. Worth checking a system's own
  documented laziness/deferral points specifically before building a fixture meant to exercise a
  *different* failure mode near them.
- **Refactoring an existing, tested function to sit on top of a new, more detailed primitive (`verify_sealed` → `verify_sealed_report`) is often lower-risk than writing a parallel implementation** — it can't drift from the new one by construction, and the existing tests double as regression coverage for the refactor itself for free.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/ledger/src/segment/mod.rs` | `SealedSegmentCheck`; `verify_sealed_report`; `verify_sealed` refactored on top of it; 2 new tests |
| `ekos/crates/ledger/src/fact_ledger.rs` | `FactLedger::verify_sealed_report` wrapper |
| `ekos/crates/cli/src/commands/ledger.rs` | `repair()`; 4 new tests |
| `ekos/crates/cli/src/bin/ekos.rs` | `ekos ledger repair` subcommand wired |
| `ekos/docs/rfcs/0105-storage-repair-tool-phase2.md` | New RFC |
| `ekos/docs/rfcs/0080-storage-architecture-plan.md`, `TODO.md` | Phase 2 marked done, points to RFC 0105 |
