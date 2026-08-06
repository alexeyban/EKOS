# RFC 0014 — Content Indexing & Ranked Search

| Field | Value |
|-------|-------|
| **Status** | Accepted |
| **Author** | EKOS team |
| **Created** | 2026-07-17 |
| **Gating** | Memory OS (v0.8) |

---

## Motivation

Full-text search currently covers object **names and kinds** only
(`object_fts(object_id, name, kind)`). Two failures follow, both hit live while
building the agent-memory workflow (devlog 15/16):

1. **Content is invisible.** A memory note whose body explains a quadratic
   coupling blowup is findable only if its *filename* carries the word
   "quadratic". The filename-as-index convention works, but it makes recall
   depend on slug discipline instead of on what the note actually says. The
   same applies to READMEs, devlogs, and every text file in the estate.
2. **No relevance ranking.** `find_objects` returns the first 20 rows in
   rowid order. A search for a common term ("mcp", "session") returns 20
   arbitrary project files and drowns the one memory note that should rank
   first.

Both block the Personal Memory OS scenarios (`next_steps.md` §1, §5): "have I
solved this before?" must match the *substance* of prior knowledge, and the
best match must surface first.

## Design

### 1. Excerpts are observation facts

`FileObserver` already reads every file's bytes (for the SHA-256). For files
whose content is valid UTF-8, it now also records an `excerpt` — the first
`EXCERPT_MAX_CHARS = 600` characters, truncated on a char boundary — in the
observation data. Binary files get no excerpt. This stays within the
observation layer's contract: an excerpt is a fact about the file, not an
interpretation.

`ekos build` copies the excerpt into `KirObject.properties["excerpt"]`, so it
travels with the object into the ledger like `path` and `size_bytes` do.

### 2. FTS schema v2: a `content` column

```sql
CREATE VIRTUAL TABLE object_fts USING fts5(
    object_id UNINDEXED, name, kind, content
);
```

`append_object` indexes `properties["excerpt"]` (empty string when absent) as
`content`. Any pass that wants an object's substance searchable — a future
note parser, a SQL analyzer storing `CREATE TABLE` fragments — has one
convention to follow: put text in `properties["excerpt"]`.

### 3. Ranked, wider results

`find_objects` orders by `bm25(object_fts, 10.0, 4.0, 1.0)` — name matches
weigh 10×, kind 4×, content 1× — and the cap rises from 20 to 50 rows. A
name hit ("the note titled X") always beats a body mention, but body-only
matches now exist at all.

### 4. Migration

On `Ledger::open`, if `object_fts` lacks the `content` column (pragma
`table_info`), the table is dropped, recreated with the v2 schema, and
repopulated from `current_objects` payloads in one transaction. This is a
derived index — rebuilding it loses nothing (same argument as devlog 14's
"`current_*` tables are pure accelerators"). ~22K objects repopulate in
seconds, once.

Excerpts for *existing* file objects appear as projects are re-observed. The
build fingerprint cache means unchanged projects skip re-observation; a
one-time backfill is `rm .ekos/fingerprints.json` before the next refresh.

## Non-goals

- Embedding/semantic search — a later RFC; bm25 keyword ranking first.
- Indexing full file contents — 600 chars captures headings/preamble where
  intent lives, without bloating a 22K-object index with megabytes of code.
- Boosting `memory/` paths structurally — bm25 name-weighting plus the
  keyword-slug convention already privileges notes; revisit with real usage.

## Testing

- File observer: text file → excerpt present and truncated; binary → absent.
- Ledger: content-keyword search finds an object whose *name* lacks the
  keyword; name match ranks above content match; v1→v2 migration preserves
  searchability of pre-existing objects.
- Agent session integration test gains a content-search turn.
