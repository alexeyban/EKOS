# Devlog 298 — Source-estate documentation from the ledger (RFC 0214 Phase 1)

**Date:** 2026-10-25
**PRs:** none (direct to `main`)
**Branch:** `main`

---

## Summary

Step 2 of the operator workflow now has its generator. `ekos docs generate --layout estate` turns the ledger into a
database estate's documentation: 1,060 pages for iDempiere in 26 s, in the page format the lakehouse-docs publishers
already read, so the GitHub wiki, Confluence and Azure DevOps publishers work unchanged. The wiki target is public, so
two gates are built into the renderer: no data value ever reaches a page, and text is used only from sources with a
known licence. A dry run against github.com/alexeyban/EKOS/wiki produced 1,061 files with no broken links; the real
publish waits for the maintainer.

---

## RFC 0214 Phase 1

### Problem / motivation
lakehouse-docs (RFC 0208) documents a pipeline from pipeline files and cannot read the ledger; `docs generate` read
the ledger but rendered a code-repository view with no publisher. Live profiles carry `min`/`max` values, and the
iDempiere documentation repository has no licence, so neither can go to a public wiki unfiltered.

### What was built

| Component | What it does |
|---|---|
| `docs-gen/src/estate.rs` | pure `build(EstateInput) → Vec<EstatePage>`: home, data model (areas, ER, table pages), glossary, `DocAction` index, routines (nested statements), schema history, findings; `detect_licence`, `licence_allows_text`, `table_key` (8 tests) |
| `cli/src/commands/docs_estate.rs` | reads the ledger read-only, review scope and sign-offs (RFC 0213 cache), committed manifest, licence files; writes pages keeping manual sections, deleting only pages with the estate marker, refusing two pages on one path (2 tests) |
| `[docs-estate]` | `licences` (source/database → SPDX or `none`), `max-er-tables` |
| `evals/oracle-idempiere/part2-docs/` | the iDempiere run, gates checked on the real output |

### Implementation details worth remembering
- **Table pages are a star**: the neighbourhood ER draws only this table's own keys; drawing every FK among its
  neighbours made C_ORDER's diagram an unreadable web.
- **Database FKs vs code references** side by side: the live catalog declares only some (C_ORDER → C_BPARTNER via
  `BILL_BPARTNER_ID` alone), the application model says which table each `*_ID` column points to.
- **Glossary rows are compact** (first description, first ten columns, "and N more"): "Tenant" names a column in
  nearly every table; one section per use made the C page 500 KB.
- Statements are every descendant of the routine by `Contains`, sorted by `index` (the pre-order position), shown
  with their depth.

### Decisions
- **In EKOS core, not the Python tool**: only Rust reads the ledger; the publishers stay where they are and read the
  same page format.
- **Licence gate at render time, per object**: an object's text needs every one of its sources (`sources`, `source`,
  `database`) licensed. A database has no licence file, so its comments are out until configured — for the testbed,
  `idempiere-db = "GPL-2.0"` because the schema and routines come from iDempiere's migration scripts.
- **Lifecycle table kept although uniform**: every iDempiere `DocAction` class implements all eleven methods (a Java
  interface), so the table is an index of where to read each action; which statuses an action sets is Phase 2.

---

## Knowledge Captured

- **The GPL-2.0 text mentions the "GNU Library General Public License"** in its body; a keyword match over the whole
  file called iDempiere LGPL-2.1. Identify a licence by its opening only.
- **A DDL table can carry SQL quotes in its name** (`"AD_CLIENTSHARE"`) next to the live `AD_CLIENTSHARE`; anything
  keyed by table name must strip quotes (`estate::table_key`).
- The GitHub wiki publisher replaces `_Sidebar.md` by default; on a shared wiki
  pass `--sidebar keep`.
- A shell `rm -rf` on a scratch directory was refused by the permission layer; write to a new directory instead.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/docs-gen/src/{estate,lib}.rs`, `Cargo.toml` | renderer |
| `ekos/crates/cli/src/commands/{docs_estate,docs,mod}.rs`, `app.rs` | `--layout estate` |
| `ekos/crates/compiler-core/src/{config,lib}.rs` | `[docs-estate]` |
| `ekos/docs/rfcs/0214-source-estate-documentation.md` | new RFC with Phase 1 results |
| `evals/oracle-idempiere/part2-docs/README.md` | run and gates |
| `README.md`, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | RFC 0214 |
