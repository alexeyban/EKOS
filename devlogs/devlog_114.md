# Devlog 114 — RFC 0097: a caching bug caught by its own regression test, fixed at the real root

**Date:** 2026-08-26
**PRs:** RFC 0097
**Branch:** main (direct)

---

## Summary

Second item in this session's six-RFC gap-closure plan (`devlog_113`'s RFC A was the first): ledger
read caching for `ekos mcp serve`. The first design shipped-then-unshipped within the same session
— it passed the full workspace gate, then a new regression test caught a real, serious bug
(caching a writable `FactLedger` handle would block any concurrent `ekos build`/`commit` from ever
acquiring tantivy's exclusive writer lock, for as long as the MCP server stayed up) before it ever
reached a user. Rather than patch around it, this entry covers going to the real root: a genuine
read-only open mode for `FactLedger`/`SearchIndex` that never touches that lock at all, which is
what actually makes safe caching possible.

---

## RFC 0097 — Read-only `FactLedger` open, and a safe `ekos mcp serve` store cache

### Problem / motivation

`ekos mcp serve` is the one long-running `ekos` process — every other command opens the store once,
does its work, and exits. Its module doc already documented a deliberate choice: reopen the store
on every single `tools/call`, specifically so a separate `ekos build`/`commit` process's changes
are always picked up without a restart. That's real, correct, and expensive — for the fact engine
(RFC 0016's default backend), a fresh open re-scans segment headers, rebuilds index runs, and
catches the tantivy search index up. Caching the open handle across calls is the obvious fix for
the repeated-work cost of an AI agent asking many small questions in one session.

### What was built, in the order it was actually discovered

1. **First attempt**: a `StoreCache` wrapping the existing writable `open_store`, invalidated by an
   on-disk mtime fingerprint. Full gate passed. A new regression test — written specifically to
   simulate a concurrent external write, since that's the scenario the whole feature exists to
   serve — failed with a real tantivy `LockBusy` error. Root-caused: `SearchIndex::open` always
   acquires tantivy's `IndexWriter` lock, held exclusively for the handle's **whole lifetime**, not
   just during a commit. A cached writable handle held that lock indefinitely while idle between
   calls. **Not shipped** — reverted via `git stash`, documented in `TODO.md` as a real, valuable
   negative result rather than silently discarded.
2. **The real fix**: `SearchIndex::open_read_only` (skips `Index::writer(..)` entirely, only ever
   creates an `IndexReader` — always safe to hold indefinitely), `FactLedger::open_read_only`
   (fails cleanly on a never-built workspace, refuses to self-heal corrupt index runs read-only,
   skips the write-requiring search-index catchup step), a new `LedgerError::ReadOnly` write guard
   in `append_inner`, and `open_store_read_only` (`crates/cli/src/commands/store.rs`) dispatching
   to it. `StoreCache` was rebuilt on top of this — the fingerprinting logic from the first attempt
   was salvaged, not thrown away, since it was correct, just built on the wrong primitive.
3. `ekos_identity_review` (the one write-capable MCP tool) was extracted out of `call_tool`'s big
   match block into its own function that opens a fresh writable store directly, bypassing
   `StoreCache` entirely — matching the module's original per-call pattern for exactly the one case
   that still needs it.

### Implementation details worth remembering

- **A "read-only" open still has to bootstrap a genuinely fresh workspace, or it breaks a real,
  intentional existing contract.** `open_store` (the original function) silently creates an empty
  on-disk store the first time anything opens a brand-new workspace — several pre-existing tests
  correctly assert every MCP tool works gracefully (empty results) against a workspace that's never
  been built. A first cut of `open_store_read_only` just errored in that case ("run `ekos build`
  first") and broke 8 passing tests. Fixed by bootstrapping: a short-lived writable
  `FactLedger::open`, opened and *immediately dropped* (never cached, never returned), followed by
  the real read-only open. Not a new race — two processes calling the original `open_store` on a
  truly fresh workspace at the same instant already had this exact narrow window before this RFC.
- **`manifest.json` is written lazily, only after a real write** — a fact already documented in
  `store.rs`'s own existing comments, and one of my own new tests got this wrong on the first try
  (asserted `manifest.json` exists right after the bootstrap open+drop, which never writes anything
  since nothing was ever appended). Fixed to check for `segments/` instead, matching the established
  pattern the pre-existing `fresh_workspace_defaults_to_the_fact_engine` test already used.
- **The fingerprint-timing bug inside the first, unshipped attempt** is worth remembering on its
  own even though the whole design it lived in got replaced: `FactLedger::open` (the writable
  version) re-indexes stale entities and commits the search index as a side effect of opening,
  changing on-disk mtimes *after* the open completes. Recording the fingerprint *before* opening
  meant the very next call always saw a spurious "changed" fingerprint and tried to reopen — while
  the first handle, still cached and alive, still held the lock. The general lesson (recompute a
  cache-invalidation fingerprint *after* the operation whose result you're caching, not before, if
  that operation itself has side effects) generalizes beyond this one bug.

### Decisions (alternatives considered, why this choice)

- **Skipping the search-index catchup on read-only open, rather than finding a way to do it
  safely, was a deliberate scope cut, not an oversight.** A read-only handle genuinely cannot
  write, so it cannot re-index anything past its watermark — the honest choice was to document this
  as a real, bounded limitation (`find_objects`/bm25 search may lag; every other read stays fully
  current) rather than inventing a workaround. A non-exclusive catchup mechanism is real future
  work, not needed to ship a correct, safe v1.
- **Not attempting RFC 0080's full Storage Architecture concurrency rework.** The same root cause
  (tantivy's lock as *incidental* single-writer enforcement, not a designed mechanism) is already
  the top-priority item in `docs/GAP_ANALYSIS.md` §11 — this RFC fixes the one concrete consequence
  blocking MCP caching, deliberately not the underlying model (a real cross-process lock, WAL,
  snapshot+compaction). Scope creep into that would have turned a one-day fix into a multi-week one
  for a problem this RFC doesn't need solved to ship real, safe value.

---

## Knowledge Captured

- **A regression test that simulates the exact scenario a feature exists to serve is worth writing
  before shipping, not after a user hits it.** The `LockBusy` bug would have been a real, painful,
  hard-to-diagnose production incident (an MCP session silently making `ekos build` hang or fail
  for as long as the agent stayed connected) — caught here in seconds by a test built specifically
  around "what does this feature need to be safe under," not just "does it pass the workspace gate."
- **tantivy's `IndexWriter` lock is acquired at `Index::writer(..)` call time and held for that
  writer's entire lifetime — not scoped to individual `.commit()` calls.** Any future code holding
  a `FactLedger`/`SearchIndex` handle open longer than one short operation needs to reason about
  this explicitly; it's not visible from the `SearchIndex` API surface alone without reading
  tantivy's own docs or, as here, hitting the lock contention live.
- **A negative result (a design built, gated, then found unsafe and reverted) is worth documenting
  with the same rigor as a shipped feature** — `TODO.md`'s entry for the abandoned first attempt is
  what let this session pick up the *real* problem (the missing read-only open primitive) instead
  of re-deriving the same dead end, and it's now also a concrete, real data point for
  `docs/GAP_ANALYSIS.md` §11's existing concurrency concern.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/ledger/src/search.rs` | `writer: Option<IndexWriter>`; new `open_read_only`; `upsert`/`commit` no-op when writer-less |
| `ekos/crates/ledger/src/fact_ledger.rs` | New `open_read_only`; `Inner.read_only` write guard in `append_inner`; 4 new tests |
| `ekos/crates/ledger/src/lib.rs` | New `LedgerError::ReadOnly` variant |
| `ekos/crates/cli/src/commands/store.rs` | New `open_store_read_only` (bootstraps a fresh workspace, dispatches per backend); 3 new tests |
| `ekos/crates/cli/src/commands/mcp.rs` | `StoreCache` (fingerprint-invalidated, built on `open_store_read_only`); `ekos_identity_review` extracted to its own writable-open function; new `StoreCache` tests including the core regression |
| `ekos/crates/cli/tests/mcp_session.rs`, `transformation_benchmark.rs` | Threaded a shared `StoreCache` through the existing multi-turn session tests |
| `ekos/docs/rfcs/0097-readonly-factledger-open-and-mcp-store-cache.md` | New RFC |
