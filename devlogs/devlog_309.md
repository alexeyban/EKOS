# Devlog 309 — Step 2 in the console: generate and publish the estate documentation (RFC 0230)

**Date:** 2026-10-26
**PRs:** none (direct to `main`); ekos-lakehouse devlog_21 (`7a6eb94`)
**Branch:** `main`

---

## Summary

The plan's step 2 — *pick a documentation store, click Generate docs, then Publish* — was the only step without a
console page (RFC 0214 had left it to a Phase 2). ekos-lakehouse now has `lakehouse docs show|generate|publish`, a
`[extensions.lakehouse.docs]` store setting, the lakehouse-docs publishers compiled into its binary, and the
**Documentation** page. Step 2 on the bar is *done* only once the latest generation is published. On iDempiere the
1,060 pages were generated and previewed against the EKOS GitHub wiki from the CLI and from the page (17/17); the
real publish waits for the maintainer. The plan's exit blockers (195 conflicts, three delete scenarios) were deferred
by the maintainer to TODO.

---

## RFC 0230

### What was built
See ekos-lakehouse devlog_21: `docs.rs` (settings, publisher arguments, destination, step 2, the publish record, the
publisher check), the CLI group and classes (read / write / external), step 2 from the estate record and the latest
real publish, the Documentation page, and the demo book's step 2 on that page. In this repository: the RFC, the
operator guide's step 2, the eval report and `docs_tour.py`.

### Decisions
- **Publishers in the binary** (`include_str!`, written to the state directory per run): an installed binary has no
  source tree.
- **Credentials only as the publishers' own variables**, shown by name.
- **A preview never completes step 2.**
- **`sidebar = "keep"`** for a shared wiki: the GitHub publisher otherwise replaces its `_Sidebar.md`.

---

## Knowledge Captured

- **The GitHub wiki publisher writes `_Sidebar.md` unless told `--sidebar keep`** — the earlier 1,061-file dry run
  (RFC 0214) included it; on EKOS's own wiki that would have replaced the wiki's sidebar.
- **The console renders extension Markdown without HTML**, so HTML comments (manual-section markers) show as text.
- A dry run of the GitHub publisher needs no token for a public wiki: it clones read-only and compares.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0230-step-2-documentation-in-the-console.md` | new RFC with results |
| `evals/oracle-idempiere/part2-docs/` | `docs_tour.py`, `documentation-page.png`, the console run in the README |
| `docs/operator-workflow.md`, `docs/generated/ekos-self-documentation.html`, `TODO.md`, `CLAUDE.md` | step 2 |
