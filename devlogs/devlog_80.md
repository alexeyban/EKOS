# Devlog 80 — `ekos build` fingerprint cache vs. a cleared ledger (RFC 0077)

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

First item of a broader gap-closing pass (the user asked to work through the full current gap
list surfaced by the last session's TODO.md survey). Root-caused and fixed a previously-found-live,
not-yet-explained bug: clearing `.ekos/ledger/` while keeping the artifact cache reproduced zero
`File` objects on the next `recover`/`compile`/`commit` cycle.

## RFC 0077

`build.rs`'s per-observe-path loop gates BOTH re-scanning the filesystem AND constructing/writing
`File`-kind `KirObject`s behind one source-content fingerprint check. The fingerprint correctly
answers "has anything on disk changed" but says nothing about whether the *ledger* still holds
what a previous scan produced — so a cleared ledger with unchanged source content stayed
File-object-empty indefinitely.

Fix: `ledger.object_count() == 0` (checked once, before the per-path loop) is a cheap,
always-correct "ledger looks freshly cleared" signal. When true, no fingerprint is trusted this
run — a real rescan repopulates everything. Every subsequent run resumes trusting the cache
normally. Deliberately doesn't cover a hypothetical *partial* File-object loss with everything
else intact — that's not what was found live.

Two new tests (`build.rs`'s first test module): the real reproduction (clear ledger, rebuild,
confirm File objects return) and a regression guard the other direction (intact ledger + unchanged
content must still hit the cache, not silently duplicate).

## Knowledge Captured

- **A cache key answering one real question ("did the source change") can silently gate a second,
  different question ("does the destination still have the result") if both happen to live behind
  the same `if`.** The fix here didn't need new cache-replay machinery — just refusing to trust the
  existing cache when the destination looks freshly emptied.
- **`ledger.object_count()` was already a public, cheap primitive** (used elsewhere for the final
  summary line) — the fix needed zero new ledger-layer surface, just calling something that already
  existed at a point in the flow it wasn't being called yet.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0077-build-fingerprint-cache-vs-cleared-ledger.md` | New RFC |
| `ekos/crates/cli/src/commands/build.rs` | `ledger_is_empty` gate; 2 new tests |
| `TODO.md` | Item marked done |
| `devlogs/devlog_80.md` | This file |
