# RFC 0214 — Source-estate documentation, published to a wiki

**Status:** Accepted (Phase 1)
**Date:** 2026-10-25
**Plan:** Oracle/iDempiere plan v3, Part II.2 (step 2 of the operator workflow)
**Depends on:** RFC 0035/0037 (docs-gen), RFC 0208 (lakehouse-docs publishers), RFC 0209–0212 (Java, Oracle,
schema history, write behaviour), RFC 0213 (review scope and sign-off)

## 1. Problem

Step 2 of the operator workflow documents a source estate before any pipeline exists: what the system is, its data
model by functional area, one page per table, the routines, the schema history and what EKOS found. `lakehouse-docs`
(RFC 0208) documents a pipeline that already exists and reads pipeline files; it cannot read the ledger. EKOS's own
`docs generate` reads the ledger but renders a code-repository view (objects, curated architecture), not a database
estate, and has no publisher.

The output goes to a **public** wiki in the first deployment (github.com/alexeyban/EKOS/wiki). Two things must never
reach it: data values (live profiles carry `min`/`max`) and text from a source whose licence does not allow it (the
iDempiere documentation repository has no licence file).

## 2. Goals and non-goals

**Goals**

1. `ekos docs generate --layout estate --output <dir>`: Markdown + Mermaid pages from the ledger only, deterministic,
   in the page format the RFC 0208 publishers read (front matter `title`/`order`/`ekos_generated: true`,
   `<dir>/index.md` is the parent of the other pages in `<dir>`, manual sections kept on regeneration, generated pages
   that no longer apply removed, hand-written pages never touched).
2. Sections: Home (sources, licences, counts, review state, provenance), Data model (areas, one page per in-scope
   table, ER per area and per table neighbourhood), Glossary (business names from the application model, cited),
   Document lifecycles (per document type, where each `DocAction` method starts — `ModelTable.document_lifecycle`),
   Logic (routines with reads/writes/callers and nested statements), Schema history (by release, by table), Findings
   (conflicts, invalid objects, write-behaviour classes).
3. **Scope from the review** (RFC 0213): a table gets a page only when it is in scope (decided, else proposed);
   out-of-scope tables are listed once with their class.
4. **No values**: a column's profile contributes counts only (nulls, distinct, rows), never `min`/`max`, never seed rows.
5. **Licence gate**: every source's licence is detected (`LICENSE*`/`COPYING*` at its root) or configured
   (`[docs-estate] licences`); text from a source with no known licence (descriptions, comments, business names,
   excerpts) is left out — names, counts and paths only. Home lists each source, its licence and what was taken.
6. Publishing uses the RFC 0208 publishers unchanged (`publish_github_wiki.py --docs <dir>`); each publish to a
   public target is confirmed by a person.

**Non-goals (Phase 2+)**

- Which statuses each `DocAction` method sets (a state diagram per document type): needs a Java analysis of the
  method bodies that the ledger does not hold yet. Phase 1 indexes where each method starts.
- `lakehouse docs generate --scope source|pipeline` / `lakehouse docs publish` catalogue commands and the console's
  Documentation page; Confluence and Azure DevOps runs (no sandbox credentials here; the publishers' dry-run only).
- Flowcharts from `ProcedureIr` (Phase 1 lists statements in order).
- Mermaid rendered to images (only Confluence needs it; the publisher already does it).

## 3. Design

### 3.1 Where

- `ekos-docs-gen::estate` (pure): `EstateInput { objects, relationships, scope, signoffs, licences, provenance,
  options } → Vec<EstatePage { path, title, order, body }>`. No IO; sorted everywhere; a reversed-input test pins
  determinism.
- `cli/commands/docs_estate.rs` (IO): reads the ledger once, the review state (RFC 0213 cache), the committed manifest
  (evidence time, git head), detects licences, writes pages with RFC 0208's file rules.

### 3.2 Which objects

| Page | From |
|---|---|
| table | `Table` with `origin: live` first, else a DDL `Table` with a `dialect` (never a PDF table); columns, PK, check constraints (expressions are the source's DDL, not data), `ForeignKey` edges both ways, `ModelTable` (`Models` edge or same name) for model class, business names and descriptions, `WriteBehaviour`, readers/writers (`ReadsFrom`/`WritesTo`/`DependsOn` from `Procedure`/`View`/`Trigger`), `SchemaChange` via `Changes`, open `ConflictingEvidence` via `Disputes` |
| area | table-name prefix before the first `_` (upper case); `I_`/`T_` areas exist but their tables are out of scope by default |
| routine | `Procedure` (live first; a file twin with the same name is mentioned, not repeated), its `ProcedureStatement`s by `Contains`, callers by incoming `Calls`/`DependsOn` |
| schema history | `SchemaChangeSet` by `release` and `applied_at`, tickets |
| findings | `ConflictingEvidence` (current ids only, `.ekos/conflicts/current.json`), live objects with `status != VALID`, `WriteBehaviour` class counts |
| glossary | `ModelTable` column `business_name` → table.column, description, source path:line |

### 3.3 Diagrams

`erDiagram` per area (FKs inside the area; above `max-er-tables`, default 30, the most-connected tables are drawn and
the rest listed) and per table (the table and its direct FK neighbours, capped the same way). Column lists are not
drawn into diagrams (GitHub's renderer gives up on large ER diagrams); the table page has the columns.

### 3.4 Licence detection

A source's licence is the first of: `[docs-estate] licences.<source>`; the SPDX id recognised in its root
`LICENSE*`/`COPYING*` (GPL-2.0/3.0, LGPL, AGPL, Apache-2.0, MIT, BSD, MPL-2.0, EPL-2.0, CC-BY*); else `unknown`. A
database has no file: `unknown` unless configured. `none` in the config means "do not use text from it" explicitly.
Text attributable to a source with `unknown`/`none` is dropped at render time, and Home says how much was dropped.

### 3.5 Config

```toml
[docs-estate]
licences = { "idempiere-db" = "GPL-2.0" }   # the database's schema comments come from iDempiere
max-er-tables = 30
```

## 4. Phases

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | `estate` renderer + `--layout estate`, licence gate, value gate, scope from review | iDempiere pages generated; no `min`/`max` and no docs-repo text in the output (tests + a grep of the real output); GitHub wiki publisher dry-run; a real publish only after the maintainer confirms |
| 2 | lakehouse `docs` commands, console Documentation page, document lifecycles, routine flowcharts | one click publishes in the console |

## 5. Tests

Pure: a table page has columns with null/distinct counts and no min/max; an out-of-scope table has no page; text from
an unknown-licence source is dropped and counted; an area ER lists only in-area FKs and caps at `max-er-tables`;
reversed input gives identical pages. CLI: regenerating keeps a manual section, removes a generated page that no longer
applies, never touches a hand-written page.

## 6. Results

**Phase 1** (2026-10-25, iDempiere testbed `idemdocs`, release build, no model call,
`evals/oracle-idempiere/part2-docs/`):

| | |
|---|---|
| pages | 1,060 (8 MB): 30 areas, 876 table pages (929 tables, 53 out of scope by the review), 27 glossary pages (3,725 terms), 26 document types, 92 routines, 28 releases (3,356 migration scripts), findings |
| run | 26 s, 2.5 GB peak (reads the whole ledger); a rerun writes 0 of 1,060 when nothing changed |
| licences | `idempiere` GPL-2.0 (detected from `LICENSE.md`), `docs` unknown (no licence file — its text is left out), `idempiere-db` set to GPL-2.0 in the testbed's `[docs-estate]` (the schema's comments and routines come from iDempiere's migration scripts); before that setting, 2,001 database texts were left out |
| value gate | 0 of C_ORDER's 11 distinctive profile `min`/`max` values appear anywhere in the output |
| publish | `publish_github_wiki.py --repo alexeyban/EKOS --prefix iDempiere --dry-run`: 1,061 files (pages + `_Sidebar.md`), 0 broken links; not pushed — a public publish waits for the maintainer's confirmation |

Found on the way: a DDL table recorded with SQL quotes (`"AD_CLIENTSHARE"`) collided with the live one on one page
path; names are now normalised (`table_key`) and a run refuses two pages on one path. The GPL-2.0 text names the
Library GPL in its body, so detection reads only a licence file's opening. Every iDempiere `DocAction` class
implements all eleven methods (it is a Java interface), so the lifecycle table is an index of where to read, not a
difference between document types. The live catalog declares fewer foreign keys than the application model implies
(`C_ORDER` → `C_BPARTNER` only via `BILL_BPARTNER_ID`); table pages show the code's `references` next to the
database's constraints.

The iDempiere output is published, with a walkthrough of steps 1–3, at
https://github.com/alexeyban/ekos-lakehouse-oracle (GPL-2.0, as the pages derive from iDempiere's code).
