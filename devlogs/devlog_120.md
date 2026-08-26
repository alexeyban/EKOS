# Devlog 120 — RFC 0103: self-healing a stale `SearchIndex` schema

**Date:** 2026-08-26
**PRs:** RFC 0103
**Branch:** main (direct)

---

## Summary

RFC 0102's live verification surfaced a real production break: RFC 0101 (earlier the same session)
added a new `memory_path` field to `SearchIndex`'s tantivy schema with no migration path, so every
`FactLedger` workspace built before that change — including this repo's own real 5,533-object
self-analysis ledger — failed to open at all (`Schema error: 'An index exists but the schema does
not match.'`). Asked the user how to handle it; they chose a real migration over a manual rebuild.
`SearchIndex::open_impl` now self-heals a stale on-disk schema transparently on a writable open by
wiping and rebuilding the search index (safe: it's already a documented derived/rebuildable
artifact), while a read-only open still fails clearly rather than mutating anything. Live-verified
against the exact real ledger that was broken: it now opens with zero manual intervention.

---

## RFC 0103 — Self-healing `SearchIndex` schema migration

### Problem / motivation

`Index::open_or_create` validates an on-disk tantivy index's stored schema against the schema
passed in and errors rather than upgrading. Any future `SearchIndex` schema change (not just RFC
0101's one field) will hit the identical failure against every pre-existing workspace without a
fix at the root.

### What was built

| Component | Change |
|---|---|
| `rebuild_stale_schema` | Wipes and recreates a `SearchIndex`'s on-disk directory |
| `SearchIndex::open_impl` | Catches `TantivyError::SchemaError` specifically; self-heals when writable, errors clearly when read-only |

### Implementation details worth remembering

- **The fix needed zero changes outside `search.rs`.** `FactLedger::open_with_seal_threshold`
  already has a working "fully reindex when the search marker is `None`" code path — it's the same
  path a brand-new workspace's first open already takes. Forcing the self-healed open to return
  `marker = None` (the exact same contract) means the existing catchup loop just does the right
  thing with no awareness that a self-heal happened at all. Found by reading the existing "fresh
  workspace" and "`runs_clean` self-heal" (`FactIndexes`, lines 120-130 of the same file) code
  paths before writing anything — both already established the same pattern for a sibling derived
  structure.
- **The self-heal must never fire from a read-only open**, matching the existing, already-shipped
  `open_read_only`/`ReadOnly` convention (RFC 0097) exactly: a read-only handle must never be the
  one performing a write, even a self-healing one, because a concurrent real writer (`ekos
  build`/`commit` in another process) must be free to run unblocked at any point while a read-only
  handle stays open. The match arm is guarded on `writable`, not just on the error variant.
- **Distinguishing "stale schema, self-healable" from "genuinely corrupt, must not be silently
  swallowed" mattered for real.** The fix only catches `TantivyError::SchemaError` specifically —
  any other error (a truly malformed `meta.json`, for instance) still surfaces as a real error. A
  dedicated test (`genuine_corruption_is_not_mistaken_for_a_stale_schema`) writes literal garbage
  bytes over `meta.json` and confirms `Index::open` fails before schema comparison even happens, so
  this can never be mistaken for the self-healable case.

### Decisions (alternatives considered, why this choice)

- **Wipe-and-rebuild, not a field-by-field tantivy segment migration.** The search index was
  already documented as a derived, rebuildable projection of the ledger's own EAVT facts — the
  facts themselves were never at risk, only their search-index projection. A real segment-level
  migration would be strictly more complex for zero benefit over "delete the projection, let the
  existing full-reindex path recompute it from the source of truth it was always derived from."
- **No schema version number / migration registry.** Considered, for future readers to distinguish
  what changed between schema versions. Not needed today — `TantivyError::SchemaError` is already a
  precise-enough trigger, and nothing downstream needs to know a migration specifically occurred
  (vs. a fresh workspace's normal first-open reindex). Revisit only if a future schema change needs
  materially different handling than "just rebuild."

---

## Knowledge Captured

- **A live-verification failure found while shipping one RFC can motivate its own accepted RFC in
  the same session**, rather than being silently worked around or deferred without a plan — RFC
  0102's verification step surfacing this schema break, followed by asking the user how to handle
  it (rather than unilaterally deciding to rebuild or ignore a real production ledger) rather than
  guessing, is the right shape for a finding that's real but outside the current RFC's stated
  scope.
- **`tantivy::TantivyError::SchemaError` is a precise, matchable variant** for "an on-disk index's
  stored schema doesn't match the schema passed to `open_or_create`" — confirmed by reading
  tantivy 0.22.1's own source (`IndexBuilder::open_or_create`, `src/index/index.rs`) rather than
  assuming the error shape from the message text alone. Any other on-disk corruption (a malformed
  `meta.json`) fails earlier, inside `Index::open`, with a different error variant — the two cases
  are structurally distinguishable, not just distinguishable by message-string sniffing.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/ledger/src/search.rs` | `rebuild_stale_schema`; `open_impl` self-heals a stale schema on writable open, errors clearly on read-only; 3 new tests |
| `ekos/docs/rfcs/0103-search-index-schema-migration.md` | New RFC |
| `TODO.md` | RFC 0102's "follow-up needed" schema-migration item marked done |
