# Devlog 83 — Storage architecture: a real, grounded plan (RFC 0080)

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Asked to save a plan for the "Storage architecture: six real gaps, none started" TODO item before
implementing any of it. No code shipped this entry — the deliverable is RFC 0080, a plan grounded
in the actual current implementation rather than the TODO's one-line-per-gap summary, plus one real
documentation correction found while researching it.

## RFC 0080

Investigated all six sub-gaps against the real code (both ledger backends, RFC 0015/0016/0034,
and the two devlogs that originally surfaced this):

- **Phase 1 (highest priority)**: concurrency is two distinct real problems, one per backend. The
  SQLite `Ledger`'s `append_object` runs 3-4 statements with no transaction wrapping — a real,
  plausible mechanism for the actual corrupted FTS5 table `devlog_65` found live in `analytics/`'s
  ledger. `FactLedger` v3's real single-writer enforcement is tantivy's own lock, not "the
  manifest lock" RFC 0016's own text claims — that claim doesn't match the code at all (no lock
  exists in `segment/mod.rs`; that module's own comment says single-writer is the caller's
  responsibility).
- Phases 2-6 (WAL+repair, snapshot+compaction, retention, materialized views, horizontal
  distribution) each got a real current-state check and a concrete dependency ordering — e.g.,
  Phase 6 is blocked on RFC 0034 itself shipping, which TODO's prior phrasing ("beyond RFC 0034")
  obscured, since RFC 0034 is Draft and unimplemented, not a completed foundation.

## Knowledge Captured

- **An RFC's own "Non-goals" section can be wrong, and only checking the code catches it.** RFC
  0016 confidently attributes concurrency safety to "the manifest lock" — a mechanism that simply
  isn't in the code. What actually provides it (tantivy's `IndexWriter` lock) is real but
  incidental, not designed for this purpose. Worth remembering: a design doc's own stated
  guarantees are a claim like any other, not proof, even when they read as settled fact.
- **"Real evidence of a problem" and "the mechanism behind it" are different claims** — devlog_65
  correctly found corruption and correctly guessed at concurrent writes as the cause, but hadn't
  traced it to the actual unprotected multi-statement write sequence. Worth the extra investigation
  pass before writing an implementation RFC, so Phase 1 fixes the real mechanism, not just "adds
  some locking somewhere."

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0080-storage-architecture-plan.md` | New planning RFC — no implementation |
| `TODO.md` | Storage architecture item updated to reference the plan and its phase ordering |
| `devlogs/devlog_83.md` | This file |
