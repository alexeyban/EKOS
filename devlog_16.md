# Devlog 16 — RFC 0014: Content indexing & ranked search

**Date:** 2026-07-17
**PRs:** —
**Branch:** main

---

## Summary

Implemented RFC 0014 — the first Memory-OS (v0.8) feature. Full-text search now covers file
*content* (a 600-char excerpt captured by the file observer), and results are ranked by bm25
relevance instead of returned in rowid order. Both changes were motivated by failures hit live
while building the agent-memory workflow in devlog 15: memory notes were findable only by
filename, and common search terms drowned the relevant hit below an arbitrary 20-row cutoff.

---

## What was built

| Component | Change |
|---|---|
| `plugins/file` | Text files (valid UTF-8) carry `excerpt` — first 600 chars, char-boundary safe — as an observation fact; binary files don't |
| `crates/cli/build.rs` | Excerpt copied into `KirObject.properties["excerpt"]` |
| `crates/ledger` | FTS schema v2 (`content` column); `index_object_fts` helper indexes the excerpt; `find_objects` ranked by `bm25(object_fts, 0.0, 10.0, 4.0, 1.0)` with the cap raised 20 → 50; automatic v1 → v2 migration on open (drop, recreate, repopulate from current objects) |
| `docs/rfcs/0014-content-indexing.md` | The RFC (accepted) |
| Tests | Excerpt capture/truncation/binary-skip; content-keyword search; name-over-content ranking; v1→v2 migration; integration turn 8 (phrase in README body, not filename) |

## Implementation details worth remembering

- **bm25 weights are positional over *all* FTS columns, including UNINDEXED ones.** The first
  draft passed 3 weights for 4 columns and silently weighted `object_id` 10× and `content` at the
  default — the intended `(name 10, kind 4, content 1)` needs the explicit leading `0.0`.
- **The FTS table is a derived index, so migration is drop-and-rebuild** — same reasoning as
  devlog 14's "`current_*` tables are pure accelerators". No data can be lost; ~22K objects
  repopulate in seconds, once, on first open.
- **Excerpt backfill needs a fingerprint reset.** The build cache skips unchanged projects, so
  existing file objects keep excerpt-less payloads until re-observed:
  `rm .ekos/fingerprints.json` before one full refresh.
- **`properties["excerpt"]` is now the convention** for making any object's substance searchable —
  a future note-frontmatter pass or SQL-fragment indexer has one field to fill, and the ledger
  needs no further schema changes.

## Verification

Against the freshly backfilled 44-project ledger (fingerprint reset + full pipeline, 6m06s):
`ekos_search "stale CKM"` finds `EKOS--lesson--phase13-cache-inputs-...md` and
`"protocolVersion handshake"` finds the MCP-recipe note — in both cases the words exist **only in
the note bodies**, not the filenames. Name matches still rank first (`"orders"` returns the table
objects before body mentions). One sharp edge worth recording: the first verification attempt used
`"quadratic"`, which also appears in the note's *filename* — a false confirmation; body-only terms
are the honest test. Natural-language questions return nothing (FTS terms are ANDed) — the memory
skill now says "keywords, not questions" explicitly. The "filename is the index" workaround is now
a nicety instead of a requirement.

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0014-content-indexing.md` | New RFC (accepted) |
| `ekos/plugins/file/src/lib.rs` | `text_excerpt()` + observation `excerpt` field + test |
| `ekos/crates/cli/src/commands/build.rs` | Excerpt → object property |
| `ekos/crates/ledger/src/lib.rs` | FTS v2 schema, migration, bm25 ranking, 3 tests |
| `ekos/crates/ledger/Cargo.toml` | `tracing` dependency |
| `ekos/crates/cli/tests/mcp_session.rs` | Turn 8: content search |
