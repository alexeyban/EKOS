# RFC 0104 — Storage Architecture Phase 1: Concurrency

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-26
**Implemented:** 2026-08-26

---

## Motivation

RFC 0080's plan named Phase 1 as the highest-priority storage gap, with real live evidence behind
it: `devlog_65` found a corrupted FTS5 virtual table in `analytics/`'s real SQLite ledger, and the
SQLite `Ledger::append_object` code path is a real, concrete, fixable mechanism that could produce
exactly that corruption shape (multiple unwrapped statements per logical write). RFC 0080 also
found `FactLedger` v3's documented single-writer story doesn't match its actual code (RFC 0016
attributes exclusion to "the manifest lock," which doesn't exist), and left two real open
questions unresolved: whether SQLite's `BEGIN IMMEDIATE` needs a supplementary explicit lock, and
what a concurrent reader actually observes mid-write. This RFC answers both and ships the fix.

## Design

### SQLite `Ledger`: real transactions around every multi-statement write

`append_object` runs INSERT `entries` → SELECT `current_objects` (old rowid) → INSERT OR REPLACE
`current_objects` → FTS update — 3-4 separate statements with no transaction. `append_relationship`
is two statements (`append_versioned`'s own SELECT+INSERT, then `current_relationships`
INSERT OR REPLACE); `append` (used by `append_evidence`/`append_event`) is `append_versioned`'s
SELECT+INSERT alone, itself a check-then-act pair with no atomicity today. Any crash, error, or
racing writer partway through any of these sequences can leave `entries` inconsistent with
`current_objects`/`current_relationships`/`object_fts` — a real, concrete mechanism for the
`object_fts` corruption `devlog_65` found live.

Fix: a new `Ledger::in_transaction` helper wraps a closure in `BEGIN IMMEDIATE` / `COMMIT`, rolling
back on any error path. Applied to the full bodies of `append` (covering `append_evidence`/
`append_event`), `append_object`, and `append_relationship` — every multi-statement writer, not
just the one already known to have bitten in practice. `BEGIN IMMEDIATE` (not the default deferred
`BEGIN`) acquires SQLite's RESERVED lock at the very start of the critical section, so two
concurrent writers serialize there rather than one discovering a conflict mid-sequence, partway
through an already-half-applied write.

**Resolving RFC 0080's open question**: does `BEGIN IMMEDIATE` need a supplementary explicit
cross-process lock (e.g. `flock`)? No. `PRAGMA journal_mode=WAL` is already set
(`Ledger::open`), and SQLite's own file-level locking under WAL mode is exactly the mechanism
`BEGIN IMMEDIATE` uses to acquire its RESERVED lock across processes — this is SQLite's own
well-established, battle-tested cross-process write-serialization primitive, not something this
project would improve on by adding a second, independent advisory lock on top. A `PRAGMA
busy_timeout` is added alongside (5000ms) so two writers racing a few milliseconds apart block
briefly and both succeed, rather than the loser getting an immediate `SQLITE_BUSY` under completely
ordinary contention (the default busy timeout is 0ms — instant failure on any conflict).

### `FactLedger`: a real, designed cross-process write lock

RFC 0080's finding, re-confirmed here: `segment/mod.rs`'s own doc comment says single-writer
exclusion is "the caller ensures it" — there is no manifest lock anywhere in that module. What
actually stops a second writable process today is tantivy's own `IndexWriter` lock inside
`SearchIndex`, an incidental side effect of using tantivy (confirmed live in `devlog_67`: a second
`open_store` handle hits `LockBusy` at open, before any write), not a mechanism this project
designed for this purpose — nothing guarantees a future tantivy version keeps behaving this way,
and the failure surfaces deep inside tantivy's own error type rather than a clear, ledger-level
message.

Fix: a new dedicated `write.lock` file at the `FactLedger` root, acquired via `fs4`'s
`FileExt::try_lock_exclusive` (already a transitive dependency via tantivy itself — the exact same
`flock`(2)-backed mechanism tantivy's own `IndexWriter` lock already uses, promoted to a direct,
first-class dependency here) as the *first* step of `FactLedger::open`/`open_with_seal_threshold`
(the writable path), before `SegmentStore`/`SearchIndex` are touched at all. A second writable
process now gets one clear, fast, ledger-level `LedgerError::Locked` naming the lock file, instead
of an eventual `LockBusy` surfacing from deep inside tantivy. The OS releases the lock automatically
when the file handle drops — including on a crash — so there is no separate cleanup step, matching
the same "no manual unlock tool needed" property tantivy's own lock already has. `open_read_only`
deliberately never acquires this lock (unchanged from RFC 0097's own reasoning: a read-only handle
must never be the one doing writer-only work). Tantivy's own `IndexWriter` lock inside `SearchIndex`
still gets acquired too, now as a redundant second safety net rather than the sole mechanism —
removing it isn't necessary or worth the risk, and RFC 0080 didn't ask for its removal.

**RFC 0016 documentation correction** (named in RFC 0080 as part of this phase): the Non-goals
section's *"the manifest lock enforces it"* is corrected to describe the real mechanism —
initially incidental (tantivy's `IndexWriter` lock), now also a real, designed, ledger-level
`write.lock` file (this RFC).

### Concurrent-read visibility: a real, verified spec

RFC 0016 claimed multi-writer exclusion made cross-reader visibility "the same as SQLite WAL gives
today" — never independently verified. It does not hold, verified here directly against the real
code: `Inner`'s `memtable`/`SegmentStore::head.committed_len` are loaded once at `open()` time (or
advanced only by this same handle's own writes) — nothing re-reads on-disk state on each query. A
`FactLedger` handle's view of the ledger is **frozen as of its own open() call** (plus whatever it
has itself appended since), not automatically refreshed by a separate process's concurrent writes.
This is a real, material difference from SQLite's WAL mode, where a *new read transaction* on an
already-open connection sees the latest committed state without needing to reopen the connection
object at all.

**Concrete consequence, stated as a real spec, not silently assumed correct**: a long-lived
`FactLedger` handle (the exact scenario RFC 0097 built read-only caching for — `ekos mcp serve`)
does not see writes committed by a separate `ekos build`/`commit` process after this handle's own
`open()` call, for *any* read method (`get_object`, `all_objects`, `object_at`, …) — not just the
already-documented search-index catchup gap RFC 0097 named. A caller needing fresh cross-process
visibility must re-open the handle; no automatic invalidation or refresh exists today. Documented
in `fact_ledger.rs`'s own module doc comment so this is discoverable from the code, not only this
RFC.

## Non-goals

- **A live-refresh/invalidation mechanism for a long-lived `FactLedger` handle.** Named as a real
  gap above, not attempted here — RFC 0080 scoped Phase 1 as "write the spec," not "build automatic
  cross-process cache invalidation," which is real, separate design work (likely relevant to a
  future WAL-based Phase 2, not this phase).
- **Removing tantivy's own `IndexWriter` lock now that an explicit lock exists.** Redundant, not
  harmful — kept as a second safety net.
- **Changing SQLite's default busy behavior beyond the new 5000ms timeout.** A configurable timeout,
  retry/backoff policy, etc. would be new scope beyond "make the existing failure mode graceful
  under ordinary contention."

## Verification

New `ekos-ledger` tests: (1) SQLite — two concurrent `append_object` calls (simulated via two
`Ledger` handles against the same file) never leave `current_objects`/`object_fts` referencing a
different version than `entries`' latest; an injected mid-sequence error rolls back cleanly (no
partial `current_objects` row); the four multi-statement writers are confirmed to run inside a real
transaction (a test connection observing `PRAGMA journal_mode` and lock state during the call). (2)
`FactLedger` — a second writable `open()` against an already-open writable ledger fails fast with
`LedgerError::Locked` (not a tantivy-internal error); a read-only `open_read_only` against an
already-open writable ledger still succeeds (never blocked by the new lock); dropping the first
handle releases the lock for a subsequent writable open. (3) The concurrent-read visibility spec
itself, made concrete as a regression test: a `FactLedger` handle opened, then a *second* handle
appends an object, then the first handle's `all_objects()`/`get_object()` are asserted to **not**
see it — proving the documented limitation is real and matches the spec, not just plausible from
reading the code. Full workspace gate clean (`cargo fmt`, `build --workspace`, `clippy --workspace
-D warnings`, `test --workspace`), `tests/integration` 3/3.

Live-verified against `FactLedger` (the RFC 0016 default backend, so the one every genuinely fresh
workspace picks up with no config): a real scratch workspace run through the full
`init`/`build`/`recover`/`resolve`/`compile`/`commit` pipeline, then (1) a sequential second
`commit` confirms ordinary back-to-back CLI usage is completely unaffected by the new lock (it
releases cleanly between commands — `Objects written: 0, Objects skipped: 2` on the re-run, no
spurious lock error); (2) two real, separate OS processes launched to run `ekos commit` against the
same workspace at the same time — one completed normally, the other failed immediately with exactly
the new error: `cannot write: another writable process already holds the ledger's write lock at
.../write.lock — only one writable ekos process ... may run against this workspace at a time`,
confirmed as a clean, ledger-level failure rather than a tantivy-internal one.

The SQLite backend has no CLI-level equivalent live check today — `open_store` auto-selects
`FactLedger` for any genuinely fresh workspace with no way to force SQLite from the CLI, so exercising
it live would need hand-constructing a pre-existing SQLite-backed workspace first. Verified instead
via the real, non-mocked mechanism directly: the new `ekos-ledger` unit tests open two real
`rusqlite::Connection`s to the same on-disk file from two real OS threads and write concurrently
(`concurrent_writers_never_corrupt_current_objects_or_fts`) — genuine cross-connection SQLite
locking under `BEGIN IMMEDIATE`/WAL, the identical mechanism two real OS processes would use, not a
simulation of it.
