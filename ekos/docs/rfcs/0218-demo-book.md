# RFC 0218 — The generated demo book: the operator workflow on iDempiere, from the console

**Status:** Accepted
**Date:** 2026-10-26
**Plan:** Oracle/iDempiere plan v3, Part V
**Repos:** EKOS (run logs for every command that changes something), ekos-lakehouse (`tools/demo-book`), the demo
repository `alexeyban/ekos-lakehouse-oracle` (the book itself)
**Depends on:** RFC 0213 (step bar), RFC 0220 (run logs), RFC 0208 (lakehouse-docs: manual sections, publishers),
RFCs 0214–0216 and 0225–0228 (the steps)

## 1. Problem

The plan's demo is a book: for each step, what the operator sees, what they click, the run log behind it, what
happens behind the scenes in plain words, what they get, and the caveats. It must be generated, so that it never
drifts from the product: screenshots, log excerpts and numbers refresh on every run; only the narration is written by a
person.

Two things stand in the way:

- **Only five commands write a run log** (RFC 0220: `build`, `recover`, `resolve`, `compile`, `commit`). Step 1's
  pipeline has one; steps 2–7 — a sign-off, an extraction approval, a requirement draft, a mart build — have none, so
  the book has nothing to quote and the gate "model calls in steps 1, 2, 3, 5, 6: 0, printed by every run" cannot be
  shown.
- **The demo repository is behind**: it describes steps 4–7 as blocked and has no tour of them.

## 2. Design

### 2.1 A run log for every command that changes something (EKOS)

Every command whose catalogue class (RFC 0186) is **write**, **review**, **admin** or **external** writes an RFC 0220
run log, built-in or from an extension. Read and local commands do not (a panel refresh would leave a file each time).

- The log's `cmd` is the command's path (`lakehouse marts build`, `review approve`); the file is
  `<state>/runs/<stamp>-<path with dashes>.jsonl`, or `EKOS_RUN_LOG` when set, as before. The console already sets
  `EKOS_RUN_LOG` for every run it starts, so its run page shows these logs with no console change.
- The five pipeline commands keep their names and behaviour.
- The parser records the command's path (`Cli::command_path`), so the class is looked up the same way the console's
  catalogue does.
- A command that prints a document on stdout keeps its logging on stderr; its run log is still written.

Extensions write stage lines with `ekos_common::run_log::emit` at the points an operator cares about. In
ekos-lakehouse: a requirement drafted (terms, questions, the model), approved; an extraction change and approval; a
mart proposed (branch), merged (commit), its build launched (run id), each measure reconciled (both totals). A line
carries counts, names and totals of aggregates — never a row value.

### 2.2 The tour (ekos-lakehouse `tools/demo-book`)

`python -m demo_book tour --console <url> --workspace <id> --out <capture>` signs in to a running console and walks
the steps' pages in order: Sources, the step bar, Docs/estate, Review (each subject), Requirements, Extraction,
Lakehouse (bronze/silver), Marts. At fixed checkpoints it saves a screenshot (`shots/<checkpoint>.png`). For every
checkpoint it also saves the run log of the latest console run of that step's command (`/api/runs` +
`/api/runs/<id>/events`), or, for a command run from a terminal, the newest file under the project's `runs/`
directory. Facts come from the read commands the pages use (`workflow status`, `review list`, `lakehouse requirements
show`, `lakehouse extraction show`, `lakehouse marts show`, all `--json`) and go to `facts.json`.

**The tour reads; it does not drive.** On a project whose steps are done, clicking every action again means the full
extraction, which the maintainer ruled out for this testbed (RFC 0227), and model calls in step 4. Each step was
driven in a browser by its own RFC's test (review, sources, extraction, requirements, marts); the book shows the
state and the run logs those runs left. A step not reached shows as such.

### 2.3 The book

`python -m demo_book render --capture <capture> --out <book>` writes one Markdown page per step plus an index into
`<book>`, with the screenshots under `images/` and the excerpts under `logs/`:

- **Generated:** the screenshot, the log excerpt (the `run started`, stage, `llm` and `run finished` lines, at most
  20), the numbers (a table per step, from `facts.json` only), the model-call count of every quoted run, and the
  step's state from the bar.
- **Written by a person:** "what happens behind the scenes" and "caveats", as lakehouse-docs manual sections
  (`<!-- ekos:manual id="…" -->`), kept on every render. A first render writes a draft that says what the step's
  commands do; the maintainer rewrites it once.
- A block whose input is missing says so ("not captured in this run") and the render exits non-zero, naming it. A
  number is never carried over from an earlier render.

`render --pdf` prints the book to one PDF through the tour's Chromium. Publishing goes through lakehouse-docs'
publishers (`--docs <book>`; the same three targets) and is the operator's action, never part of a render.

### 2.4 The demo repository

The book is committed to `ekos-lakehouse-oracle` under `book/`; the README's step table points at it and stops
describing steps 4–7 as blocked.

## 3. Not in this RFC

Driving every step from an empty project, video, narration by a model, CDC (RFC 0217).

## 4. Phases and exit

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | run logs for write/review/admin/external commands; lakehouse stage lines | unit tests; a console run of a lakehouse command shows its events and `llm` count |
| 2 | `demo_book tour` and `render` (+ `--pdf`) | unit tests for render (manual sections kept, missing input fails, numbers from facts only); the iDempiere book generated end to end |
| 3 | the book in the demo repository | committed and pushed; publishing left to the operator (a dry run shown) |

## 5. Results (2026-10-26, iDempiere testbed)

**Phase 1.** Every write/review/admin/external command now writes a run log; `ekos docs generate` records its
latest estate generation in `<state>/docs/estate.json`, and the lakehouse step bar reads it (step 2 was hard-coded
*todo* before; it now reads *partial* — "1060 page(s) generated … into …; publishing is the operator's"). From the
console, `docs generate`, `lakehouse requirements edit`, `lakehouse extraction change` and `lakehouse marts reconcile`
each left their stage lines and `llm count 0` in the run's events (`/api/runs/<id>/events`, what the run page shows); `database test` from the Sources page left its
log under `<state>/runs/` (the page calls it directly, not through the run queue).

**Phase 2.** `tools/demo-book` (6 unit tests). The tour took 10 screenshots and found 74 runs; with `--act test-db
--act docs --act edit-undo --act reconcile` every action succeeded, and after `edit-undo` the approved requirement and
the extraction plan's digest (`03d5fce4e39fa6f0`) were unchanged. The book: 8 pages, 21 PDF pages, 3.6 MB.

| Step | On the bar | Model calls in the quoted runs |
|---|---|---|
| 1 | done | 0 |
| 2 | partial (pages generated, not published) | 0 |
| 3 | partial (4 of 6 signed) | no run log |
| 4–7 | blocked behind step 3 | 0, 0, no run log, 0 |

Steps 3 and 6 have no run log to quote: the sign-offs and the 165-table subset load ran before this RFC. The book
says so on those pages.

**Phase 3.** The book is in the demo repository under `book/`; publishing it is left to the operator.

Found on the way:

- **Step 2 was never read from anything.** The bar said "not built yet" for a project with 1,060 generated pages.
- **A full-page screenshot of a page listing 849 tables fails** (`Page.captureScreenshot`), and the long review
  pages came to 7 MB each. The tour photographs the window, as an operator sees it.
- **The connection test from a restarted scratch console failed** — its keychain was in memory. The run log said
  exactly why (`EKOS_ORACLE_PASSWORD … is not set`), which is what the log is for.
- **A change the plan already has is refused** ("C_ORDER: cannot load in full: already in full"), and a table with a
  change is listed twice on the Extraction page (changes and tables); only one of the two rows selects it.
- The public book needed local paths and the account name out of the text and the screenshots: `--replace OLD=NEW`
  on both `tour` and `render`.
