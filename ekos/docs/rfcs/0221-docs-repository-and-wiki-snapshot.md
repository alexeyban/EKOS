# RFC 0221 — Documentation Repository and Wiki Snapshot

**Status:** Accepted
**Date:** 2026-10-24
**Depends on:** RFC 0025 / 0144 (local documents, Markdown sections), RFC 0178 (`[[sources]]`), RFC 0219
(tickets)
**Plan:** `oracle-idempiere-plan.md` v3, Part I.8 — the docs repository and the wiki snapshot (RFC 0219 did
the git history and ticket anchors).

---

## 1. Motivation

iDempiere documents itself in two places: the Docusaurus site
[idempiere/idempiere.github.io](https://github.com/idempiere/idempiere.github.io) (510 Markdown pages) and
wiki.idempiere.org (MediaWiki). The code and the database say *what*; these say *how it is meant to be used*
and often *why*. Both should reach the same ledger as the code, joined to it.

Read through `localdocs` today, the docs repository yields 514 documents and 3,016 sections — and 504 of
those sections are nothing but YAML front matter (`sidebar_position: 36`, `tags: [technical]`), the block
before the first heading. The front matter is the page's own metadata (title, description) and, for a wiki
snapshot, the only place its provenance (URL, revision) can live. And the docs name 1,284 distinct
`IDEMPIERE-NNNN` tickets that nothing links.

The wiki cannot be read by a script from here: every request, API included, gets a Cloudflare JavaScript
challenge (HTTP 403, "Just a moment…"). Defeating the challenge is out of the question; the snapshot must
also work from an export a person saves in a browser.

## 2. Goals and non-goals

**Goals**

1. Markdown front matter becomes `Document` properties and is not a section.
2. A section records the tickets it names (`[tickets] keys`), and `ekos commit` links ticket → section.
3. `scripts/wiki-snapshot.py`: MediaWiki pages → Markdown files with provenance front matter, from the API
   or from a `Special:Export` XML file.
4. The docs repository and the snapshot are `[[sources]]` of the iDempiere project, at pinned revisions.

**Non-goals**

- Republishing either source. The docs repository has no licence file (GitHub reports none); the wiki's
  licence is recorded per snapshot when stated. Analysis is local; Part II.2's *Publish* must check
  `licence` before it uploads anything derived from these pages.
- Bypassing bot protection, crawling a whole wiki unasked, or fetching on every build — the snapshot is a
  separate, explicit step whose output is an ordinary directory.
- Plain-word table mentions in prose (`C_Order` without backticks); backticked names already link
  (RFC 0144 `doc_links`, `ModelTable` since RFC 0209).

## 3. Design

### 3.1 Front matter (`ekos-plugin-localdocs`, Markdown only)

A file whose first line is `---` and which has a closing `---` or `...` line within its first 200 lines has
front matter. It is parsed as YAML; only top-level keys with a scalar or a list of scalars are kept (as
strings / string arrays, at most 40 keys, each value at most 500 characters, sanitized like section text).
Unparseable YAML keeps nothing but is still removed from the body — it is metadata either way. The block's
lines are blanked (line breaks kept) before sectioning, so every section keeps its real line numbers and a
file that was only front matter before its first heading loses its heading-less first section.

The artifact gains `front_matter` only when there is some, so other documents' artifacts are byte-identical.
`PIPELINE_LOGIC_VERSION` 4 → 5 (the observer's output changed). Section indices of a file with front matter
shift down by one, so their section ids change once.

`LocalDocAnalyzerPass` (v4) writes `front_matter` on the `Document` and promotes, when present, `title`,
`description`, and the snapshot's provenance keys `source_url`, `source_revision`, `revision_timestamp`,
`licence` as `Document` properties; `title`, `source_url`, `source_revision` and `licence` are also copied to
every section, so a cited section shows where it came from.

### 3.2 Ticket mentions in sections

`LocalDocAnalyzerPass::with_ticket_keys(keys)` (the keys join its cache inputs): each section records
`tickets` — `ekos_common::tickets::find` over its text, sorted, de-duplicated. `semantic::tickets::link`
adds `Section` to its citing kinds, so a ticket that a commit names gets `TicketFor` to every section that
names it. No ticket object is made from a document alone (RFC 0219: tickets come from commits).

### 3.3 `scripts/wiki-snapshot.py`

Python 3 standard library plus `pandoc` (MediaWiki → GitHub-flavoured Markdown).

```
wiki-snapshot.py --xml export.xml --base-url https://wiki.idempiere.org/en/ --out DIR [--licence TEXT]
wiki-snapshot.py --api https://wiki.example.org/api.php --page TITLE … | --category NAME --out DIR
```

- One file per page, `DIR/<namespace>/<title>.md`, with front matter: `title`, `source_url`,
  `source_revision` (the revision id), `revision_timestamp`, `licence` (`--licence`, else the API's
  `rightsinfo`, else `unknown`), `generator: ekos-wiki-snapshot <version>`. No retrieval time: the same
  revision gives the same file.
- Redirect pages are skipped and counted; a page pandoc cannot convert is kept as its raw wikitext in a
  fenced block and counted. A `SNAPSHOT.json` lists every page written with its revision.
- API mode: one request at a time, at least one second apart, a plain `ekos-wiki-snapshot/<version>`
  User-Agent, `maxlag`, and it stops on a 403 / challenge page with a message pointing at `--xml` — it
  never retries around a block.

### 3.4 The iDempiere project

`[[sources]]` replaces `[observe] paths` with named sources: `idempiere` (git, the code), `docs` (the
docs repository at a pinned commit), `wiki` (the snapshot directory). Source names key object ids, so this
is a fresh project, not an edit of the existing one.

## 4. Phases

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | front matter; section tickets + `TicketFor` | docs repo: 0 front-matter sections; ticket links counted |
| 2 | `wiki-snapshot.py` + tests | an XML export → Markdown with provenance; API mode stops on a block |
| 3 | the three-source project, full pipeline | counts per source; a question answered from docs + code |

## 5. Tests

Front matter: parsed keys, nested values dropped, unclosed `---` is not front matter, line numbers kept,
unparseable YAML removed. Analyzer: promoted properties on document and sections; section tickets with the
configured keys only. Linker: `Section` citations linked. Snapshot: a fixture export (two pages, a redirect)
→ two files with front matter, the redirect counted; a stubbed 403 stops API mode.

## 6. Results

Docs repository `idempiere/idempiere.github.io` @ `3959ecee0e`, release build, scratch `EKOS_HOME`, no model
calls.

**Phase 1.** Sections 3,016 → 2,520; heading-less first sections 504 → 8 (the 8 left are MDX `import` lines,
real text before the first heading). 499 of 514 documents carry a `description`. With `[tickets] keys =
["IDEMPIERE", "TICKET"]`, 860 sections record tickets; the docs repository's own 329 commits name 19 tickets,
linked to 39 sections, and 1,267 cited tickets are named by none of its commits (the code repository's history
is where they live — Phase 3).

**Phase 2.** `wiki-snapshot.py` against a fixture export: two pages written with provenance, the redirect
skipped, identical output on a re-run; a 403 stops API mode after one request. Not yet run against a real
wiki: wiki.idempiere.org answers every scripted request with a Cloudflare challenge, so a snapshot needs a
`Special:Export` file saved in a browser.

**Phase 3.** Not yet measured.
