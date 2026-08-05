# Act 4 transcript

**Prompt:** Have I ever hit FTS5 duplicate-row problems before? How did I fix it?

```
Yes — twice, both real, both slightly different:

**1. FTS5 `INSERT OR REPLACE` doesn't actually replace rows** (devlog_17.md, RFC 0015 work)
FTS5 tables have no unique constraints, so v1's "upsert on reindex" logic silently duplicated a row on every reindex — the live index ended up with 44,796 rows for 22,023 objects, and searches returned duplicates. Fix: don't try to upsert into FTS5 at all. RFC 0015's v2 schema switched to a **contentless FTS5 index keyed by entry rowid** (`contentless_delete=1`), which structurally can't duplicate — delete-then-insert by rowid instead of relying on FTS5's own replace semantics.

**2. FTS5 `MATCH` syntax characters caused query errors, which is adjacent but different** (devlog_6.md, Phase 10)
Not a duplicate-row bug, but the same "FTS5 doesn't behave like a normal table" family: `-`, `:`, `"` are operator characters to FTS5's query parser (NOT, column filter, phrase delimiter), so raw user text sent into `MATCH` could throw a SQL error. Fixed in `Ledger::find_objects` by escaping any query with special characters into a quoted literal phrase, passing plain alphanumeric/`*` queries through unescaped so prefix search still works.

Bottom line if you hit this again: **never treat FTS5 as a normal indexed table** — it has no unique constraints (so upsert-by-reindex duplicates silently) and its query string is a mini query language (so raw text needs escaping). The durable fix that generalizes is going contentless and keying by rowid, not patching the upsert logic.
```
