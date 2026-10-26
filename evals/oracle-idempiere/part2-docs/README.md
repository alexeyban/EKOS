# Part II.2 — source-estate documentation (RFC 0214 Phase 1)

**Date:** 2026-10-25 · **Project:** `idemdocs` (iDempiere code + docs + Oracle XE) · **Model calls:** 0

```bash
ekos --project idemdocs docs generate --layout estate --output <dir>
python tools/lakehouse-docs/publish/publish_github_wiki.py --docs <dir> --repo alexeyban/EKOS --prefix iDempiere --dry-run
```

| | |
|---|---|
| pages | 1,060 (8 MB) |
| tables | 929, 876 in scope (each with a page), 30 areas by name prefix |
| foreign keys (declared by the database) | 3,100 |
| glossary | 3,725 terms on 27 letter pages (the largest 146 KB) |
| document types with `DocAction` | 26 |
| routines | 92 (database first; file twins listed on the routine's page) |
| migration scripts | 3,356 in 28 releases |
| open conflicts | 446 |
| run | 26 s, 2.5 GB peak; a second run: 0 written, 1,060 unchanged |
| wiki dry run | 1,061 files, 0 broken links |

**Gates checked on the real output**

- No values: none of C_ORDER's 11 distinctive profile `min`/`max` values (dates, ids, amounts) appear in any page.
- Licences: `idempiere` GPL-2.0 (detected), `docs` unknown → its text left out (no licence file in the iDempiere
  documentation repository), `idempiere-db` GPL-2.0 by `[docs-estate] licences` in the testbed config (schema
  comments and routines come from iDempiere's migration scripts). Without that line, 2,001 database texts were left
  out and the home page said so.

**From the console (RFC 0230, 2026-10-26):** `docs_tour.py` drives the Documentation page: the store, its destination,
the credential variable's name and the publisher check are shown, the Home page is rendered, **Generate** and
**Preview publish** run (their run logs: pages written, the dry run's result, 0 model calls) and a preview leaves step
2 *partial*. **17 passed, 0 failed.** The preview with `sidebar = "keep"`: 1,060 pages changed, 0 removed, the wiki's
`_Sidebar.md` untouched. Screenshot: `documentation-page.png`.

**BLOCKER for the public publish:** the maintainer's confirmation. The EKOS wiki has one page (`Home.md`); the
publisher writes `iDempiere-*` pages next to it and, by default, replaces `_Sidebar.md` with the iDempiere tree —
pass `--sidebar keep` to leave the wiki's sidebar alone.
