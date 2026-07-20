# Act 4 transcript

**Prompt:** Have I ever hit FTS5 duplicate-row problems before? How did I fix it?

```
Yes — you hit this on 2026-07-17 while building EKOS's ledger (source: `devlog_17.md`).

**The problem:** `INSERT OR REPLACE` doesn't work on FTS5 virtual tables because they have no unique constraints — it just plain-inserts every time. This silently accumulated 44,796 FTS rows for only 22,023 real objects, so searches returned duplicate results.

**The fix:** key the FTS index by rowid and do DELETE+INSERT instead of relying on `INSERT OR REPLACE` (or alternatively use `contentless_delete=1`).

(Side note recorded in the same lesson, unrelated but from the same debugging session: `zstd::encode_all(data, 19)` was also costing ~45ms/record from reallocating match tables — fixed by reusing one `zstd::bulk::Compressor`, dropping it to ~2.3ms/record.)
```
