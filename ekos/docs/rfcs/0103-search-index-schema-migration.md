# RFC 0103 — Self-healing `SearchIndex` schema migration

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-26
**Implemented:** 2026-08-26

---

## Motivation

RFC 0102's live verification found a real, previously-undiscovered production consequence of RFC
0101 (same session, shipped earlier): `SearchIndex::open_impl` builds a fresh tantivy `Schema` on
every open and calls `Index::open_or_create(mmap_dir, schema)`. Tantivy's `open_or_create`
validates the on-disk index's stored schema against the one passed in and returns
`TantivyError::SchemaError("An index exists but the schema does not match.")` on any mismatch —
it does not transparently migrate. RFC 0101 added a new `memory_path` field to that schema; every
`FactLedger` workspace built before RFC 0101 landed (including this repo's own real self-analysis
ledger) now fails to open at all. Confirmed live: `ekos docs generate` against the repo-root
`.ekos` ledger errored with exactly that message.

This is a real, general problem, not specific to RFC 0101's one field — any future `SearchIndex`
schema change (a new indexed field, same as RFC 0100's `ai_overview`/`ai_usage` addition to
`indexed_content()` didn't need a schema change since it flows through the existing `content`
field, but RFC 0101's structural `memory_path` field did) will hit the identical failure against
every pre-existing workspace. A real migration path, not a one-off patch, is the right fix.

## Design

### Why "wipe and rebuild," not a field-by-field migration

The search index is already documented as a **derived, rebuildable** artifact — the module's own
top-of-file doc comment states this as a project invariant, and `FactLedger::
open_with_seal_threshold` already contains a working "wipe and fully reindex" precedent for a
different derived structure (`FactIndexes`/`runs`, self-healed when `runs_clean` is false, lines
120-130: `remove_dir_all` the index directory, reopen fresh, force `runs_marker = None` so the
memtable/replay logic reconstructs everything). `SearchIndex::open`'s own `fresh` case already
does the exact same thing implicitly: a brand-new workspace's first open returns a `None` marker,
and `FactLedger::open_with_seal_threshold`'s existing catchup loop (`batches_after(search_marker)`
with `search_marker = None` matching every batch) re-indexes every object from the ledger's own
EAVT runs/memtable — the ledger facts themselves are the source of truth, the search index is only
ever a queryable projection of them. A real field-by-field tantivy segment migration would be
strictly more complex than "delete the projection, recompute it from the source of truth it was
always derived from" for zero real benefit — the source of truth was never at risk.

### Where the self-heal happens

`SearchIndex::open_impl` catches `Err(TantivyError::SchemaError(_))` specifically (not any other
error — a genuine corruption should still surface as an error rather than being silently papered
over) from `Index::open_or_create`, and only when `writable` is true. On that match: wipe `dir`
(`remove_dir_all` + `create_dir_all` — the directory is dedicated entirely to this one search
index, nothing else lives there), reopen a fresh `MmapDirectory`, retry `Index::open_or_create`
with the current schema (now guaranteed to succeed against an empty directory), and force the
returned marker to `None` regardless of what `last_tx` might have said before the wipe — the same
contract a genuinely fresh workspace already returns, requiring **zero changes** to
`FactLedger::open_with_seal_threshold`'s existing catchup logic, which already knows how to fully
reindex from a `None` marker.

### Read-only opens never self-heal

Matches `FactLedger::open_read_only`'s own existing precedent for the sibling `runs_clean` corruption
case (`"index runs need rebuilding, which a read-only open cannot do — open writable ... to
self-heal, then reopen read-only"`): `SearchIndex::open_read_only` (`writable = false`) hitting a
schema mismatch returns a clear `LedgerError::Corrupt` naming the exact fix, rather than mutating
the directory from a read-only-contracted code path (the same reasoning `open_read_only`'s doc
comment already states for never acquiring the writer lock — a read-only handle must never be the
one doing destructive/writing work, even self-healing work).

## Non-goals

- **Preserving the old search index's content across the rebuild without a full reindex.** Not
  meaningfully possible — a stale schema means the old segments are structurally incompatible with
  the new field set; nothing to salvage. The full-reindex cost is already paid by every genuinely
  new workspace today, so this isn't new cost, just a new trigger for existing cost.
- **A schema version number / explicit migration registry.** Considered — would let a future
  reader distinguish "this schema change added a field" from "this schema change removed one" (the
  wipe-and-rebuild approach handles both identically, since it never inspects what actually
  changed). Not needed today: `TantivyError::SchemaError` alone is a sufficient, already-precise
  trigger, and no other consumer needs to know a migration occurred. Revisit only if a future
  schema change needs different handling than "just rebuild."

## Verification

New `ekos-ledger` regression tests: (1) a `SearchIndex` opened against a directory holding an
index built with an intentionally different (older-shaped) schema self-heals on a writable open —
the new field is queryable afterward, no error; (2) the identical stale-schema directory opened
read-only returns a clear `LedgerError::Corrupt` naming the writable-open fix, and does **not**
mutate the directory (a following writable open still needs to self-heal, proving nothing was
silently half-fixed); (3) a genuine non-schema corruption (malformed `meta.json`) still surfaces
as an error rather than being swallowed by the new catch arm. Full workspace gate clean (`cargo
fmt`, `build --workspace`, `clippy --workspace -D warnings`, `test --workspace`), `tests/
integration` 3/3.

Live-verified against the exact real failure that motivated this RFC: ran `ekos docs generate`
against this repo's own real self-analysis ledger at the repo root (`/home/legion/
PycharmProjects/EKOS/.ekos`, built before RFC 0101, previously failing with the schema-mismatch
error) — it now opens, self-heals (logged reindex), and produces real output, with no manual
intervention (no manual delete of `.ekos`, no separate migration command run).
