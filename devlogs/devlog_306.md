# Devlog 306 — The generated demo book; run logs for every command that changes something (RFC 0218)

**Date:** 2026-10-26
**PRs:** none (direct to `main`); ekos-lakehouse devlog_20; demo repo `alexeyban/ekos-lakehouse-oracle`
**Branch:** `main`

---

## Summary

Part V of the Oracle/iDempiere plan. A script now walks the EKOS console through the seven steps of the operator
workflow and renders a book: per step the screen, what the operator clicks, the run log behind it, what happens
behind it, what they get and the caveats. Screenshots, logs and numbers come from the product on every render; only
manual sections are written by a person. To have logs to quote, every command classed write, review, admin or
external now writes an RFC 0220 run log, and ekos-lakehouse emits stage lines for its step 4–7 actions. Step 2 on the
bar now comes from a record `docs generate --layout estate` writes; it had been hard-coded *todo*. The iDempiere book
(8 pages, 21-page PDF) is in the demo repository. CDC (RFC 0217) was skipped: the plan builds it only if an operator
asks.

---

## RFC 0218 — the demo book

### Problem / motivation
The plan's demo is a book generated from the product. Only the five pipeline commands wrote run logs, so steps 2–7
had nothing to quote, and "0 model calls, printed by every run" could not be shown past step 1.

### What was built

| Component | Change |
|---|---|
| `Cli::command_path` (`app.rs`) | the command's path from the parsed matches, built-in or extension |
| `catalogue::class_for` | a path's class, built-ins first, then an extension's rows |
| `run_log::for_class` | a run log for write/review/admin/external commands; `cmd` is the path, the file name the path with dashes |
| `init_logging_stderr_with` | machine-output commands keep logs on stderr and still get the run-log layer |
| `docs_estate` | `<state>/docs/estate.json` (output, pages, written/unchanged/removed, at) + a `docs` stage line |
| lakehouse `workflow::docs_step` | step 2 *partial* from that record ("N page(s) generated … into …; publishing is the operator's") |
| lakehouse stage lines | gold drafted; marts merged / build launched / reconciled (both totals); requirements resolved / approved; extraction changed / changes undone / approved |
| `tools/demo-book` | `tour` (Playwright: screenshots, facts from the read commands, console + terminal run logs; `--act` for actions that leave the project as it was) and `render` (pages, excerpts, PDF) |
| demo repo | `book/`, README step table for all seven steps, walkthrough 5 rewritten |

### Decisions
- **The tour reads; it does not drive the whole workflow.** Driving step 6 again means the full 849-table run the
  maintainer ruled out; step 4 again means model calls. Each step was driven in a browser by its own RFC's test. The
  `--act` actions are the ones that leave the project as it was: a connection test, the estate docs, a change and its
  undo in steps 4 and 5 (approved digests unchanged, checked), one mart's reconciliation.
- **Run logs by catalogue class, not by a list.** The class already says "this changes something"; read and local
  commands would leave a file per panel refresh.
- **Missing input fails the render** unless `--allow-missing`, and is said on the page. A number is never carried
  over from an earlier render.
- **Narration in lakehouse-docs manual sections**, so the book publishes through the same publishers and keeps a
  person's text the same way.
- **`--replace OLD=NEW`** on both commands: the demo repository is public, and the pages, logs and screenshots
  carried the local account name and scratch paths. Replaced in the DOM before each screenshot.

---

## Knowledge Captured

- **Chromium cannot screenshot a very long page** (`Page.captureScreenshot: Unable to capture screenshot` on the
  849-table Lakehouse page); full-page shots of long review pages reached 7 MB. Photograph the window.
- **An extension action without inputs or a confirmation runs on the click** — there is no Run button to wait for.
- **A table with a change is listed twice on the Extraction page**; only the tables panel selects it. Select by trying
  each row until the action's button appears.
- **`lakehouse extraction change` refuses a change the plan already has** ("already in full"); pick a table whose mode
  differs.
- **The Sources page runs `database test` directly, not through the run queue**, so its log lands in
  `<state>/runs/`, not on a console run page. The tour reads both places.
- **A scratch console with an in-memory keychain loses database passwords on restart**; the run log names the unset
  variable, which is how it was found.
- **`--layout` on the Commands page is a text field** (no `possible` values in clap), not a select.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/app.rs` | `command_path`; run log by class; test |
| `ekos/crates/cli/src/run_log.rs` | `for_class`, `open_started`; test |
| `ekos/crates/cli/src/commands/catalogue.rs` | `class_for` |
| `ekos/crates/cli/src/commands/mod.rs` | `init_logging_stderr_with` |
| `ekos/crates/cli/src/commands/docs_estate.rs` | `RECORD`, the record and its stage line |
| `ekos/docs/rfcs/0218-demo-book.md` | new RFC with results |
| `docs/operator-workflow.md`, `README.md`, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | run logs, step 2, the demo book |
