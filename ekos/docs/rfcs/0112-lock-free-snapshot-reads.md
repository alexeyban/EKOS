# RFC 0112 — Lock-Free Snapshot Reads for FactLedger (WAL-Style Read/Write Isolation)

**Status:** Accepted — implemented 2026-09-19 (see "Implementation notes" at the end)
**Author:** EKOS team
**Created:** 2026-08-27

---

## Motivation

`FactLedger::open_read_only`'s own doc comment (`crates/ledger/src/fact_ledger.rs`, RFC 0097)
already states the gap this RFC closes, in its own words:

> More broadly, every read on *any* handle — writable or read-only — reflects the ledger's on-disk
> state as of this handle's own `open()` call... not automatically refreshed by a separate process's
> concurrent writes. `Inner`'s `memtable`/`SegmentStore::head.committed_len` are loaded once at open
> and advanced only by this handle's own writes — nothing re-reads the on-disk head segment on each
> query. This is a real, material difference from SQLite's WAL mode, where a *new read transaction*
> on an already-open connection sees the latest committed state without reopening the connection
> object — no such automatic refresh exists here. A caller needing fresh cross-process visibility
> must re-open the handle.

This was found and precisely documented by RFC 0104 Phase 1 (`devlog_121`) — it corrected RFC 0016's
original, false claim of "the same visibility SQLite WAL gives today" to this weaker, real behavior,
and shipped that as an accepted, named limitation rather than closing it. The workaround that exists
today is RFC 0097's `StoreCache` (`crates/cli/src/commands/mcp.rs:48`): it fingerprints the *entire*
store directory via `walkdir` (every file's mtime, `store_fingerprint`) on every `tools/call`, and on
any change, **fully reopens** the handle — re-running `FactIndexes::open`, rebuilding the memtable
from `batches_after(None)`, re-running the search-index catch-up loop. This is correct but coarse:
an unrelated one-batch change anywhere invalidates the whole cache, the fingerprint scan itself costs
O(files in the workspace) rather than O(1), and the "refresh" is a full cold-open, not an incremental
one.

**The user's framing is exactly right and already structurally available in this codebase**: because
the ledger is append-only (sealed segments are immutable forever, CLAUDE.md's key invariants) and the
Runtime never writes, nothing here needs a lock to give readers a consistent, up-to-date snapshot —
the same property that lets SQLite's WAL mode give readers a non-blocking, per-transaction snapshot.
Two concrete pieces of evidence this is cheap to build, not speculative:

- `SegmentStore`'s committed-length watermark (`Head.committed_len`, `segment/mod.rs:117`) is already
  exactly the boundary WAL-style reads need — it's fsync'd on every commit and is what
  `batches_after` already uses to serve *incremental* reads during crash recovery. Nothing new needs
  inventing; the existing recovery-scan logic just needs to run incrementally on the read path
  instead of only once at `open()`.
- Tantivy's `IndexReader::reload()` (`crates/ledger/src/search.rs:234`) is **already used in this
  codebase** — but only from the *writer's* `commit()` path. A read-only handle's `IndexReader` is
  never reloaded after open, because `search.commit()` short-circuits (`if !self.dirty`) and a
  read-only handle's `writer` field is always `None`. The cheap, native refresh primitive already
  exists in the dependency; it's simply never invoked from the read side.

## Scope

- A per-read (not per-handle-open) consistent snapshot for `FactLedger`'s read-only handles: a cheap
  freshness check plus an *incremental* refresh, replacing `StoreCache`'s full reopen.
- Lock-free by construction — no new locking primitive. Writers keep RFC 0104's `write.lock` and
  tantivy's `IndexWriter` lock exactly as today; readers never take either, before or after this RFC.
- Scoped to the single-machine `FactLedger` (RFC 0016/0111's Local mode). RFC 0111's Distributed mode
  (Service B, §6) already anticipates an equivalent per-partition watermark-freshness check at the
  network layer — this RFC is the foundational, single-process version of that same idea; whether
  they end up sharing one implementation is an Open Question, not resolved here.

## Non-goals

- **Changing the SQLite backend.** It already gets real WAL-mode read isolation natively
  (`PRAGMA journal_mode=WAL`, per RFC 0080's Foundation section) — this concern is `FactLedger`
  v3-only.
- **True MVCC with explicit snapshot pinning** (a caller holding a deliberately-stale view across
  several calls while newer writes land). Every refresh in this design always advances to the
  *latest* watermark — never holds an old one on purpose. Genuine point-in-time reads already exist
  via `object_at`/`state_at` (RFC 0047/0106, timestamp-addressed); this RFC is about which watermark
  an *ordinary*, non-timestamped read uses, not about historical queries, which are already solved.
- **Distributed reads.** RFC 0111's territory; this RFC only touches the single-process `FactLedger`.
- ~~**Implementation.** Design only~~ — implemented; see the end of this document.

## Design

### 1. Cheap freshness check — replace the `walkdir` fingerprint with the store's own watermark

`StoreCache::get` (`mcp.rs:65`) currently calls `store_fingerprint`, an O(files-in-workspace)
`walkdir` scan for the newest mtime anywhere under the store root. Replace it with reading the
store's own manifest + `Head` watermark file directly — the exact file `SegmentStore` already
fsyncs on every commit (`segment/mod.rs`) — an O(1) stat/read instead of a full-tree walk:

```rust
/// Cheap: one file read, no directory walk. Returns the committed-length
/// watermark plus the active segment's sequence number, together uniquely
/// identifying "how much of the ledger has this reader seen."
fn current_watermark(root: &Path) -> Option<(u32, u64)> { /* reads Head from segment/mod.rs's on-disk format */ }
```

### 2. Incremental refresh — fold only the delta, don't reopen

When the watermark has advanced since the handle's last-known marker, refresh in place rather than
reopening:

- **Memtable/runs**: call `SegmentStore::batches_after(last_known_marker)` — the *exact* call
  `open()` already makes from `None` (genesis) on a cold open, now made incrementally from wherever
  this handle last left off. Fold the resulting batches into `Inner`'s in-memory memtable the same
  way `open()`'s own loop already does. No new fold logic — the existing logic just needs an
  entry point that isn't only "once, at construction."
- **Search index**: call `self.search.reader.reload()` — already a real, working, cheap tantivy API
  call in this codebase (`search.rs:234`), just never reached from a read-only handle today. No file
  reopen, no new `IndexWriter` lock contention (tantivy's `reload()` is a reader-side operation,
  independent of who holds the writer lock).
- **`StoreCache`'s role shrinks accordingly**: it still holds one long-lived handle across MCP calls,
  but instead of "reopen everything on any detected change," it calls this incremental refresh, which
  itself decides how much work — often none — a given call actually needs.

### 3. Per-query snapshot semantics, matching WAL

Every top-level Runtime/MCP read call triggers the freshness check (§1) before doing any work, and
the incremental refresh (§2) if needed, so that call is guaranteed to observe everything committed
before it started — the same guarantee a new SQLite WAL read transaction gets on an already-open
connection. Concurrently, a writer proceeds without ever waiting on a reader (readers never take its
lock) and without a reader ever waiting on it (readers never block on the writer's in-progress append
— they only ever consult the watermark as of *whenever they last checked*, which is always some
valid, fully-committed, immutable prefix).

## Alternatives Considered

- **Keep `StoreCache`'s current full-reopen-on-any-change design.** Rejected as the long-term
  answer, though it's the working, shipped v1: correct but wasteful — a one-batch change anywhere
  triggers the same cost as a cold `open()` (full `FactIndexes::open`, memtable rebuild from
  genesis, full search catch-up), when the actual delta is usually tiny.
- **Push-based invalidation** (the writer notifies readers via inotify/a socket/a pub-sub channel)
  instead of readers pulling a cheap watermark check on each call. Rejected for v1: solves a problem
  a cheap pull already solves at negligible per-call cost, and adds a new cross-process notification
  mechanism (plus its own failure modes — a missed notification, a reader that was never
  listening) for no measured benefit over polling a single small file.
- **True MVCC with explicit snapshot pinning.** Rejected as unneeded: `object_at`/`state_at` already
  give exact point-in-time reads by timestamp (RFC 0047/0106) when that's genuinely wanted; an
  ordinary "give me current state" read wants freshness, not a pinned old view.

## Architecture Review

- **Runtime stays read-only** (CLAUDE.md key invariant) — this RFC only makes reads observe more
  up-to-date committed data faster; it introduces no write path reachable from any read-only handle.
- **Append-only preserved** — nothing here rewrites or reorders sealed data; the refresh mechanism
  only ever consumes `batches_after`, the same forward-only, immutable-segment read path crash
  recovery already relies on.
- **No new lock, no new `unsafe`** — the "unblocking" property the user named is preserved by
  construction: a reader never acquires `write.lock` or the `IndexWriter` lock, before or after this
  change; `IndexReader::reload()` and `batches_after` are both already-safe, already-used APIs in
  this codebase, just newly invoked from a place they weren't reached from before.
- **Dependency injection / existing seams unaffected** — `KnowledgeStore` callers (Runtime, MCP tool
  handlers, `docs-gen`) see no interface change; this is entirely internal to `FactLedger`'s
  read-only path and `StoreCache`'s refresh policy.

No inconsistency found with `ekos.md` or CLAUDE.md's key invariants.

## Open Questions

All three resolved during implementation (2026-09-19):

- [x] **Refresh cadence** — check on **every** top-level read call. Measured (see Implementation notes):
      the unchanged-case check costs ~27 µs on a 5,000-object, many-segment store, cheaper than the
      `walkdir` fingerprint it replaces (~101 µs), so debouncing would add a staleness window to
      save less than the check itself costs.
- [x] **Reader-side compaction** — none. The incremental fold appends to the memtable exactly as a
      cold `open()` does for the un-flushed tail; a seal (which flushes runs) is handled by a cold
      rebuild instead. The byte-identical test below confirms no divergence.
- [x] **Shared implementation with RFC 0111 Service B** — not shared in v1. The local mechanism lives
      in `SegmentStore::refresh_read_only` and `FactLedger::refresh_snapshot`; Service B's
      network-hop equivalent can call the same `RefreshOutcome` contract later. Left as a follow-up
      rather than coupling this change to distributed mode.

## Acceptance Criteria

- [x] All Open Questions resolved or explicitly re-scoped.
- [x] At least one review completed (self-review against CLAUDE.md invariants; no external reviewer).
- [x] The `walkdir`-based `store_fingerprint` is replaced by the O(1) watermark check, measurably
      cheaper on a realistic multi-file workspace.
- [x] Regression test: a writer process appends N batches while a long-lived read-only handle stays
      open across multiple `find_objects`/`get_object` calls; each call reflects every commit
      completed before that call started, without any full reopen — mirroring RFC 0104's own
      "live-verified with two real, separate `ekos` processes racing the same real scratch
      workspace" precedent.
- [x] Tantivy `reader.reload()` is exercised from the read-only path when the watermark has advanced,
      verified by a live regression test, not just unit-level mocking.
- [x] Concurrency test: a writer holds its lock for the whole test duration; reads still succeed
      throughout and observe progressively fresher data — no read ever blocks on the writer's lock.
- [x] Correctness test: the incremental-fold result is byte-identical to a full cold reopen at the
      same watermark — the refactor must not silently diverge from the existing from-genesis open
      path.
- [x] Design consistent with `ekos.md`'s compiler architecture and CLAUDE.md's key invariants —
      confirmed by the Architecture Review above.

## Testing

- Concurrent writer + long-lived reader fixture (as in Acceptance Criteria) — the core regression
  test this RFC exists to add.
- Cost benchmark (`cargo bench`, reusing the existing `benchmark/` workspace's conventions):
  incremental refresh vs. full reopen vs. today's `walkdir` fingerprint, at realistic workspace
  sizes — the concrete claim this RFC needs to make true, not just assumed.
- Byte-identical correctness test: incremental-fold state vs. full-reopen state at the same
  watermark, across a fixture with multiple interleaved writer batches.
- Staleness-bound test: assert a read call's observed watermark is never older than the most recent
  commit that completed strictly before that call began (the precise WAL-style guarantee this RFC
  makes).

## Files Changed

| File | Change |
|---|---|

## Implementation notes (2026-09-19)

**What shipped**

| Piece | Where |
|---|---|
| `SegmentStore::refresh_read_only` → `StoreRefresh::{Unchanged, Advanced(batches), Reopen}` | `ledger/src/segment/mod.rs` |
| `FactLedger::refresh_snapshot` → `RefreshOutcome::{Unchanged, Incremental{batches}, Reopened, Unsupported}` | `ledger/src/fact_ledger.rs` |
| `KnowledgeStore::refresh_snapshot` (default `Unsupported`; SQLite, partitioned and distributed stores keep their existing invalidation) | `ledger/src/lib.rs` |
| `SearchIndex::refresh_reader` (tantivy `reload`, reports whether the segment set moved) | `ledger/src/search.rs` |
| `StoreCache::refresh` uses it, keeping the RFC 0097 fingerprint only for stores that return `Unsupported` | `cli/src/commands/mcp.rs` |
| Benchmark | `benchmark/benches/ledger_refresh.rs` |

**How it decides.** One `HEAD` read plus one `stat` of the active segment. Same active segment and
file no longer than the watermark → nothing to do. File longer → read only the appended tail from the
old (frame-aligned) watermark, decode it, fold it into the memtable, extend the batch-time map, read
only the new tail of `provenance.jsonl`, and reload tantivy's reader. Anything else — a seal moved the
active segment, the manifest's sealed list or dictionary changed, the file shrank, or `indexes/last_tx`
moved (a run flush) — falls back to the cold rebuild, through the *same* `build_ro_inner` a fresh open
uses, which is what keeps the two byte-identical.

**Ordering detail that matters.** The tail is read *before* the manifest is re-read: the writer persists
a new attribute path in the manifest before appending any fact that references it, so a manifest read
afterwards is always at least as new as every frame in the tail.

**Measured** (`cargo bench --bench ledger_refresh`, 5,000 objects, 64 KB seal threshold, this dev machine):

| Operation | Time |
|---|---|
| old: `walkdir` fingerprint of the store | ~101 µs |
| new: `refresh_snapshot`, nothing changed | ~27 µs |
| new: `refresh_snapshot` after one appended batch | ~106 µs |
| old: full cold reopen (what any change used to cost) | ~851 µs |

The one-batch refresh is ~8× cheaper than the reopen it replaces, and the gap widens with ledger size
because the reopen is O(ledger) and the refresh is O(delta). The unchanged case is ~4× cheaper than the
fingerprint alone.

**Verified.**
- Unit: live writer (holding both locks for the whole test) + long-lived reader — each read sees every
  prior commit; multi-batch delta in one refresh; seal fallback still matches a cold open; search
  reader reload makes a new commit findable; provenance tail folded; writable handle is a no-op.
- `incremental_fold_is_identical_to_a_cold_reopen_at_the_same_watermark` compares `entry_count`, every
  object and every relationship (order-normalised JSON) against a fresh cold read-only open.
- `StoreCache` tests: tracks a writer that stays open; drops RFC 0114's cached tool results exactly when
  the store advances and keeps them when it doesn't.
- **Live, two real processes:** one long-lived `ekos mcp serve` (stdio) while a separate `ekos
  build → recover → resolve → compile → commit` added a file to the workspace. `ekos_status` went
  3 → 6 objects (7 → 17 entries) and `ekos_search "Fresh"` found the new symbol, with no restart.

**Known limits / follow-ups**
- Only frames the writer has made visible on disk are seen; a frame mid-write is picked up by the next
  call (the tail scan stops at the first incomplete frame).
- A backend-served (object-store) read-only partition keeps its previous behaviour: its local `HEAD` is
  a fetched copy, so refresh reports nothing new until that partition is re-fetched — RFC 0111
  Service B's territory.
- Sharing this contract with Service B's per-partition freshness check remains open (see Open Questions).
