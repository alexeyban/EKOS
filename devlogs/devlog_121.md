# Devlog 121 — RFC 0104: Storage Architecture Phase 1 (Concurrency)

**Date:** 2026-08-26
**PRs:** RFC 0104
**Branch:** main (direct)

---

## Summary

The first implementation phase of RFC 0080's storage architecture plan — real, live-evidence-backed
concurrency fixes for both ledger backends. SQLite `Ledger`'s multi-statement writers now run
inside real transactions (the concrete mechanism `devlog_65` traced a live `object_fts` corruption
to). `FactLedger` v3 gets a real, designed cross-process write lock instead of incidentally relying
on tantivy's own `IndexWriter` lock, plus a corrected RFC 0016 and a concurrent-read visibility spec
verified directly against the real implementation rather than assumed. Phases 2-6 remain, tracked
in RFC 0080; Phase 6 stays explicitly blocked on RFC 0034 (Draft, unimplemented).

---

## RFC 0104 — Storage Architecture Phase 1: Concurrency

### Problem / motivation

RFC 0080 (the planning RFC, `devlog` not separately numbered — filed 2026-08-22) sequenced six real
storage gaps by urgency; Phase 1 had the strongest evidence behind it: a corrupted `object_fts`
FTS5 virtual table found live in `analytics/`'s real SQLite ledger, plus a documentation
correction RFC 0080 found in the same investigation (RFC 0016 attributes `FactLedger`'s
single-writer exclusion to "the manifest lock," which doesn't exist in the code).

### What was built

| Component | Change |
|---|---|
| SQLite `Ledger` | `in_transaction` helper; `append`/`append_object`/`append_relationship` now run inside real `BEGIN IMMEDIATE`/`COMMIT` transactions; `PRAGMA busy_timeout=5000` |
| `FactLedger` | `acquire_write_lock` — a real `write.lock` file via `fs4`'s `flock`(2)-backed exclusive lock, acquired first on every writable open |
| `LedgerError` | New `Locked` variant |
| RFC 0016 | Non-goals section corrected to describe the real mechanism |
| `fact_ledger.rs` module docs | Concurrent-read visibility spec'd precisely |

### Implementation details worth remembering

- **`BEGIN IMMEDIATE` under WAL mode was already sufficient cross-process protection — no
  supplementary `flock` needed for SQLite.** RFC 0080 left this as an open design question; the
  answer, confirmed by how SQLite's own WAL-mode locking works, is that `BEGIN IMMEDIATE` already
  uses real cross-process file-level locking to acquire its RESERVED lock. Adding an independent
  advisory lock on top would be redundant complexity solving an already-solved problem. `FactLedger`
  is the opposite case — genuinely needed a new lock, since nothing designed for that purpose
  existed at all.
- **The concurrent-read visibility spec turned out to be a real, previously-unverified gap, not
  just documentation housekeeping.** RFC 0016 claimed `FactLedger` gives "the same visibility SQLite
  WAL gives today." Reading the actual code (`Inner`'s `memtable`, `SegmentStore::
  head.committed_len`, both loaded once at `open()` and advanced only by the same handle's own
  writes) showed this doesn't hold — a long-lived handle's view is frozen as of its own `open()`
  call, not automatically refreshed by a separate process's writes. **Written as a test, not just a
  claim**: `a_long_lived_handle_does_not_see_a_separate_handles_writes_until_reopened` opens a
  read-only handle, has a *separate* handle append an object, and asserts the first handle's
  `object_count()` stays unchanged until it's reopened — proving the spec is true of the real
  implementation, not just plausible from reading the source.
- **The new `write.lock` had to be acquired *before* `SegmentStore`/`SearchIndex` are touched at
  all**, not just added somewhere inside the open path. Before this fix, a second writable process
  discovered the conflict deep inside tantivy's `IndexWriter` lock, several steps into
  `FactLedger::open` — a real but indirect failure mode. Acquiring the new lock as the literal first
  line of `open_with_seal_threshold` means a racing process now fails immediately, before any other
  state (segments, search index) is even opened, with a `LedgerError::Locked` naming the exact file.
  Live-verified with two real OS processes: `cannot write: another writable process already holds
  the ledger's write lock at .../write.lock — only one writable ekos process ... may run against
  this workspace at a time`.
- **`fs4` was already a transitive dependency (via tantivy's own lock implementation) — promoting
  it to direct just needed a `Cargo.toml` line, not a new dependency tree.** Confirmed the exact
  same version (0.8.4) was already resolved in `Cargo.lock` before adding it directly, so this
  couldn't introduce a version conflict.

### Decisions (alternatives considered, why this choice)

- **Real OS-level `flock`, not an in-process `Mutex` or a lockfile-existence check.** `FactLedger`
  already has an in-process `Mutex<Inner>` for the single-process case; the real gap was
  *cross-process* exclusion, which needs OS-level file locking (the same mechanism tantivy's own
  lock already used incidentally). A plain "does a lock file exist" check would be a classic
  TOCTOU race and wouldn't auto-release on a crash the way `flock` does.
- **Kept tantivy's own `IndexWriter` lock rather than removing it now that an explicit lock
  exists.** Redundant, but removing it wasn't asked for and isn't free of risk (it's tantivy's own
  internal invariant, not something to reach into). A harmless second safety net.

---

## Knowledge Captured

- **A documented behavioral claim ("the same visibility as X") is a testable assertion, not just
  prose — and it can be wrong even when the surrounding architecture is otherwise sound.** RFC
  0016's claim about read visibility was never independently checked against the real
  implementation before this RFC. Worth treating any comparative claim like this ("same as
  before," "equivalent to X") as something to verify directly, not inherit from an earlier RFC's own
  unverified assertion.
- **A lock's *position* in an open sequence matters as much as its existence.** The same
  `IndexWriter` lock already existed and already worked before this RFC — but only as a late,
  indirect failure. Moving the new, purpose-built lock to the very front of the open sequence is
  what actually changed the failure from "an eventual tantivy-internal error several steps in" to
  "an immediate, clearly-named error at the very first step."

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/ledger/src/lib.rs` | `in_transaction` helper; `append`/`append_object`/`append_relationship` wrapped; `busy_timeout`; new `Locked` error variant; 3 new tests |
| `ekos/crates/ledger/src/fact_ledger.rs` | `acquire_write_lock`; `FactLedger` gains `_write_lock` field; both open paths wired; visibility spec documented; 4 new tests |
| `ekos/Cargo.toml`, `ekos/crates/ledger/Cargo.toml` | `fs4` promoted from transitive to direct dependency |
| `docs/rfcs/0016-fact-segment-engine.md` | Non-goals correction |
| `ekos/docs/rfcs/0104-storage-concurrency-phase1.md` | New RFC |
| `ekos/docs/rfcs/0080-storage-architecture-plan.md`, `TODO.md` | Phase 1 marked done, points to RFC 0104 |
