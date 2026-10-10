# Devlog 246 — Repository cleanup and docs brought to the current state

**Date:** 2026-10-10
**PRs:** none — committed to main, `[skip ci]` (no code changes)
**Branch:** main

---

## Summary

Removed 2,260 files of material that no longer described the project, cut the token documents out of the product docs (the files stay), replaced the August-era GitHub Pages landing with a pointer to ekos.dev, and brought the README and CHANGELOG up to what is actually on `main`. Everything removed is in git history; links that need it are pinned to `2a13a0c`, the last commit that has it.

---

## What changed

| Item | Action | Why / where it went |
|---|---|---|
| `archive/doc/` (2,229 files) | removed | Generated-docs output from 2026-08-21. The two decks that cite it as sample output (`generated-documentation.html`, `cross-project-documentation.html`) now link `blob/2a13a0c/archive/doc/…`, which still resolves |
| `archive/demo/` | removed | The retired twelve-act walkthrough. README section and `CLAUDE.md`/`sonar-project.properties` mentions updated; runnable demos are in `demo/` |
| `archive_plans/` (8 files) | removed | Pre-RFC planning documents, referenced nowhere; superseded by RFCs and `TODO.md` |
| `landing/index.html` | removed | The old landing page, superseded by ekos.dev |
| `docs/spikes/`, `docs/GAP_ANALYSIS.md` | removed | Early spike files, and a gap analysis that called itself a 2026-08-27 snapshot. `TODO.md` is the live backlog; its historical entries mentioning these were left as history |
| `docs/index.html` (alexeyban.github.io/EKOS/) | replaced | Now a short page pointing to ekos.dev (canonical), plus the capabilities reference and presentations that only live on GitHub Pages. The old page carried token links and August-era claims |
| Token documents (`TOKENOMICS.md`, `VISION.md`, `PIONEER_PROGRAM.md`, the vision deck) | kept, unlinked | Removed from `docs/presentations.html` and `SECURITY.md`. ekos.dev's `/token/` page still links `TOKENOMICS.md` |
| `README.md` | updated | Intro points to ekos.dev; the crate list includes `session`, `plpgsql` and the Migrate crates; the version paragraph says v1.0.3 and `main` at RFC 0172 / 245 devlogs |
| `CHANGELOG.md` | updated | "Unreleased" section covering the 49 commits since v1.0.3: Migrate, procedures/views/triggers, semantics → LinkML, freshness, ConflictingEvidence, and fixes |

## Knowledge Captured

- **Link to a commit, not `main`, when deleting material that published pages cite.** `blob/<sha>/path` keeps the evidence reachable without keeping the files in the tree.
- **`sonar.exclusions` comments named `archive/demo/`.** Config comments are documentation too; search them when removing directories.
- **Saved eval reports (`evals/reports/zen-full/*.json`) quote paths that no longer exist.** They are records of what a run saw; leave them.
