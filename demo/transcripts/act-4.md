# Act 4 transcript

**Prompt:** Have I ever hit FTS5 duplicate-row problems before? How did I fix it?

```
Yes — this happened in the EKOS ledger (found via memory search, sourced from `devlog_17.md`, 2026-07-17):

**Problem:** SQLite FTS5 virtual tables have no unique constraints, so `INSERT OR REPLACE` silently just plain-inserts instead of replacing. EKOS's v1 ledger accumulated 44,796 FTS rows for only 22,023 real objects — every reindex added duplicates, which showed up as duplicate search results.

**Fix:** Key the FTS index by `rowid` and do explicit `DELETE` + `INSERT` on update (instead of relying on `INSERT OR REPLACE`), or alternatively use `contentless_delete=1` so deletes are actually supported.

Related gotcha noted in the same lesson: don't call `zstd::encode_all(data, 19)` per small record — level-19 context creation costs ~45ms/record; reuse one `zstd::bulk::Compressor` instead (dropped it to ~2.3ms/record in EKOS's benchmark).
```
