# RFC 0230 — Step 2 in the console: generate and publish the source-estate documentation

**Status:** Accepted
**Date:** 2026-10-26
**Plan:** Oracle/iDempiere plan v3, Part II.2 (step 2), demo-grade
**Repos:** ekos-lakehouse (`lakehouse docs`, the Documentation page); EKOS docs only
**Depends on:** RFC 0214 (estate pages, Phase 1), RFC 0208 (lakehouse-docs publishers), RFC 0218 (step 2 from the
estate record), RFC 0198/0207/0226 (extension pages)

## 1. Problem

The plan's step 2: the operator *picks a documentation store, clicks **Generate docs**, then **Publish***. RFC 0214
built the pages (`ekos docs generate --layout estate`) and left the rest to a Phase 2 that was never built: there is
no store to pick, no Publish, and the publishers are Python scripts in the ekos-lakehouse source tree that an
installed binary cannot find. Step 2 is the only step of the seven without a console page.

## 2. Design

### 2.1 Settings

```toml
[extensions.lakehouse.docs]
output = "docs/estate"          # relative to the project directory (default)
target = "github"               # github | azure-devops | confluence
python = "python3"              # the interpreter that runs the publisher (default python3)
# github
repo = "owner/name"             # whose wiki
prefix = "iDempiere"            # page-name prefix, several projects in one wiki
sidebar = "keep"                # leave the wiki's own _Sidebar.md (the publisher's default replaces it)
# azure-devops: org-url, project, wiki, root-path · confluence: space, parent-id, title-prefix
```

Credentials are never here. Each publisher reads its own variables (`GITHUB_TOKEN`; `AZDO_PAT`; `CONFLUENCE_URL`
with `CONFLUENCE_PAT`, or `CONFLUENCE_USER` with `CONFLUENCE_API_TOKEN`); in the console a tenant gets them the way it
gets any other variable an owner grants it (RFC 0184).

### 2.2 Commands

| Command | Class | What it does |
|---|---|---|
| `lakehouse docs show [--json]` | read | the latest generation (pages, when, where), the target and its settings, the latest publish and dry run, whether the publisher can run (interpreter, modules), and the Home page |
| `lakehouse docs generate` | write | EKOS's estate generation into `output` (the RFC 0218 record) |
| `lakehouse docs publish [--dry-run]` | external | the target's publisher over `output`; a dry run sends nothing |

`publish` refuses without a generation, and without a `target` and its required settings, naming what is missing.
The publishers are compiled into the binary (`include_str!`) and written to `<state>/lakehouse/docs/publish/` before
each run, so an installed binary carries the version it was built with. Each publish is recorded in
`<state>/lakehouse/docs/{publish,dry-run}.json`: target, destination, when, pages, the pages generated at, the
outcome and the publisher's summary line. Run-log lines: generated (pages), published / dry run (target, pages, ok).

### 2.3 Step 2 on the bar

| State | When |
|---|---|
| todo | no generation |
| partial | generated, not published since (a dry run does not count) |
| done | the latest publish succeeded and is newer than the latest generation |

### 2.4 The Documentation page

The lakehouse extension's **Documentation** page: the state (generation, target, last publish and dry run, the
publisher check), the generated Home page rendered (the console's own Markdown renderer), and actions **Generate**,
**Preview publish** (`--dry-run`) and **Publish** (a confirmation naming the destination). Publishing to a public
wiki is the operator's decision every time.

## 3. Not in this RFC

Document lifecycles and routine flowcharts (RFC 0214's other Phase 2 items), editing pages in the console, choosing
pages to publish, and Mermaid rendered to images (only Confluence needs it; its publisher does it when `mmdc` is
present).

## 4. Phases and exit

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | settings, `lakehouse docs show|generate|publish`, step 2 states, run-log lines | unit tests; on iDempiere, generate and a GitHub dry run from the CLI |
| 2 | the Documentation page | a browser run: generate and preview on the page; a real publish only with the maintainer's confirmation |

## 5. Results (2026-10-26, iDempiere testbed)

**Phase 1.** `lakehouse docs show` reported the 1,060 pages generated earlier, the store (`github`, the
`alexeyban/EKOS` wiki, prefix `iDempiere`), `GITHUB_TOKEN` as the credential variable and `python3 3.10.12: ready`.
`lakehouse docs generate`: 1,060 pages, all unchanged. `lakehouse docs publish --dry-run`, with no token: "github
wiki: 1061 changed, 0 removed (dry run)" — the same count as RFC 0214's dry run — recorded in `dry-run.json`, its run
log ending with 0 model calls; step 2 stayed *partial*.

**Phase 2.** `evals/oracle-idempiere/part2-docs/docs_tour.py`: 17 passed, 0 failed — Generate and Preview publish on
the page, both run logs, the preview shown on the page, step 2 still *partial*. Publish was not clicked.

Found on the way:

- **The GitHub publisher replaces the wiki's `_Sidebar.md` by default** — on a shared wiki (here EKOS's own) the
  project's tree would replace the wiki's sidebar. `sidebar = "keep"` passes `--sidebar keep`; the preview then
  changes 1,060 pages and not the sidebar.
- **The console's Markdown view shows raw HTML as text**, so the Home panel strips the manual-section markers.

**Not done:** the real publish (the maintainer's decision), Azure DevOps and Confluence against real services (no
credentials here; their publishers are tested in lakehouse-docs).
