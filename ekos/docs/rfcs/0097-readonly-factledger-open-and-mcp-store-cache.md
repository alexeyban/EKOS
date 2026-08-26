# RFC 0097 — Read-only `FactLedger` open, and a safe `ekos mcp serve` store cache

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-26
**Implemented:** 2026-08-26

---

## Motivation

`docs/GAP_ANALYSIS.md`'s "Runtime/Retrieval" backlog named ledger read caching as an open item,
scoped to `ekos mcp serve` (a genuinely long-running process, unlike every other `ekos` command,
which is one-shot). The first attempt at this — a `StoreCache` decorator wrapping the normal,
writable `open_store`, invalidated by an on-disk mtime fingerprint — passed the full workspace gate
and was ready to ship, until a new regression test (written specifically to simulate a concurrent
external write, the exact scenario the cache exists to serve well) failed with a real tantivy
`LockBusy` error.

Root cause, confirmed by reading `crates/ledger/src/search.rs`: `SearchIndex::open` always calls
`Index::writer(..)`, which acquires tantivy's on-disk `IndexWriter` lockfile — **exclusive for the
whole handle's lifetime**, not just while a commit is in flight. Caching a writable `FactLedger`
handle across `tools/call`s (the entire premise of the first design) meant the MCP server would
hold that lock indefinitely while idle between calls, blocking any real `ekos build`/`commit`
running in a separate process from ever acquiring it for as long as the server stayed up — a write-
starvation regression, strictly worse than the read-latency problem the cache was meant to fix. Not
shipped; reverted (the fingerprinting logic itself was salvaged, see below).

This is the same root cause already named as the top-priority item in `docs/GAP_ANALYSIS.md` §11
(Storage Architecture Phase 1): *"`FactLedger` v3's actual single-writer enforcement is tantivy's
own `IndexWriter` lock — an incidental side effect, not a designed mechanism."* This RFC is a
narrowly-scoped fix for the one concrete consequence blocking MCP caching, not the full concurrency
rework RFC 0080 describes (a real WAL/repair tool, snapshot+compaction, a designed cross-process
lock) — that remains its own, larger, separate future increment.

## Design

### `SearchIndex::open_read_only` — never acquires the writer

`SearchIndex`'s `writer` field becomes `Option<IndexWriter>`. A new `open_read_only` constructor
(sharing `open`'s schema/mmap-directory setup via a common `open_impl(dir, writable: bool)`) skips
`Index::writer(..)` entirely when `writable` is `false` — only `index.reader()` is created, which
is always safe to hold indefinitely and share across readers/processes; tantivy's lock is a writer
concern only. `upsert`/`commit` become no-ops when `self.writer` is `None` (defense in depth —
`FactLedger`'s own write guard, below, already rejects every write before reaching here).

### `FactLedger::open_read_only` — the storage-layer read-only open

A new constructor mirroring `open_with_seal_threshold`, but: fails with `LedgerError::NotFound` if
`root` doesn't exist (never creates a fresh store, unlike a writable open); refuses to run the
existing self-heal path if index runs are found unreadable (`LedgerError::Corrupt` — self-heal
rebuilds runs from scratch, a write, which a read-only open must not attempt); and — the one real,
honest limitation this design accepts — **skips the search-index catchup step entirely**. A
writable open re-indexes any entity committed past the search index's own watermark as part of
opening; a read-only open cannot write, so it doesn't. Consequence: `find_objects` (bm25 search)
via a read-only-opened handle may lag behind objects committed by a separate writer *after* the
search index's on-disk state was last written by some write-capable process. Every other read
(`get_object`, `all_objects`, `object_at`, EKL queries not using free-text search, …) reads directly
from the EAVT runs/memtable and is always fully current — unaffected by this, and this is the read
path `StoreCache`'s fingerprint check (below) keeps fresh regardless.

A new `Inner.read_only: bool` field, checked once at the top of `append_inner` (the single funnel
every `append_object`/`append_evidence`/`append_relationship`/`append_event` call goes through) —
`Err(LedgerError::ReadOnly)` instead of a silent no-op, so a caller that mistakenly tries to write
through a read-only handle gets a clear, correct error rather than losing data quietly.

### `open_store_read_only` (`crates/cli/src/commands/store.rs`)

Mirrors `open_store`'s three-way backend dispatch. SQLite: unchanged, just `Ledger::open` — SQLite
has no analogous whole-handle-lifetime exclusive lock, so a normal open is already safe to cache.
Fact engine: `FactLedger::open_read_only`. The one real design wrinkle: `open_store` itself
*silently creates* an empty on-disk store for a genuinely-fresh (neither backend ever written)
workspace, and several pre-existing tests correctly depend on every MCP tool working gracefully
against a brand-new workspace (empty results, not an error). `open_store_read_only` preserves this
by bootstrapping — a short-lived writable `FactLedger::open` that's opened and *immediately
dropped*, never returned or held, followed by the real read-only open (which now succeeds since the
store exists). This narrow writable-open window is not a new race: two processes calling the
original `open_store` on a truly fresh workspace at the same moment already had this exact
bootstrap race before this RFC.

### `StoreCache` (`crates/cli/src/commands/mcp.rs`) — safe to hold indefinitely now

The fingerprinting design salvaged from the first attempt, now built on `open_store_read_only`
instead of `open_store`: a cheap on-disk mtime fingerprint (newest mtime under the store root,
metadata-only — no segment/index rebuild) recorded *after* a successful open (not before — the
first attempt's fingerprint-before-open timing bug, since fixed at its root by this RFC skipping
the writable catchup step read-only opens never had cause to trigger). `StoreCache::get` reopens
only when the fingerprint changes, or on first use, or after a prior open failed.

**The one write-capable MCP tool, `ekos_identity_review`, bypasses the cache entirely** — extracted
into its own `identity_review` function that opens a fresh, short-lived, writable store directly,
exactly matching this whole module's pre-RFC-0097 pattern for every tool. No explicit cache
invalidation after a write is needed: the write changes on-disk mtimes as a side effect, so the
*next* `StoreCache::get` call's fingerprint check naturally detects the change and reopens.

## Non-goals

- **The full Storage Architecture concurrency rework** (RFC 0080 Phase 1: a real designed
  cross-process lock correcting RFC 0016's own text, which incorrectly attributed single-writer
  enforcement to "the manifest lock" — a mechanism that doesn't exist in the code; a WAL/repair
  tool; snapshot+compaction). This RFC fixes the one concrete lock-contention consequence blocking
  MCP caching, not the underlying concurrency model.
- **Read-only open also catching up the search index.** A real, future extension (would need a
  read-only-safe way to detect and surface staleness, or a separate non-exclusive catchup
  mechanism) — not attempted here; the honest limitation is documented instead of silently
  papered over.
- **MCP streaming, multi-turn history, or the other Runtime/Retrieval backlog items** — unrelated
  to this RFC, tracked separately in `TODO.md`.

## Verification

Storage layer: 4 new `FactLedger` tests (`open_read_only` on a never-built workspace fails cleanly;
reads data a writable open committed; rejects every write method without corrupting state; **the
core regression** — a read-only handle staying open never blocks a concurrent writable open, the
exact scenario that failed with `LockBusy` before this fix) plus 3 new `open_store_read_only` tests
in `crates/cli` (never-built workspace bootstraps an empty store rather than erroring; reads a
fact-engine workspace `open_store` built; reads a pre-existing SQLite workspace). Full workspace
gate clean (`cargo fmt`, `build --workspace`, `clippy --workspace -D warnings`, `test --workspace`
— all green, zero regressions across the 8 pre-existing MCP tests that depend on a fresh workspace
working gracefully), `tests/integration` 3/3.

Live-verified against this repo's own real, already-committed `.ekos/` ledger (5529 objects): ran
`ekos mcp serve --workspace .`, sent two `ekos_status` `tools/call` requests over its real stdio
protocol (both returned identical cached results, confirming handle reuse), then ran a genuine
concurrent `ekos build` in a separate process **while the MCP server's cached read-only handle
stayed open** — the build completed successfully (`Build complete. ... Total objects in ledger:
5533`), no lock error, no blocking. This is the exact scenario that failed under the first,
unshipped design.
