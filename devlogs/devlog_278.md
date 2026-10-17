# Devlog 278 — closing the Lakehouse release gaps: its own name, projects appearing live

**Date:** 2026-10-17
**PRs:** none (local commits on `main`, `[skip ci]`)
**Branch:** main
**Design:** RFC 0199 (follow-ups found by publishing EKOS Lakehouse 0.1.0)

---

## Summary

Publishing EKOS Lakehouse 0.1.0 left three gaps; two were in core. An extension binary reported
EKOS's name and version (`ekos 1.1.0`); it now says `ekos-lakehouse 0.1.0 (EKOS 1.1.0)`. A project
created in the EKOS home while the console ran (by the CLI or the Lakehouse installer) appeared
only after a console restart; it now appears on the next project list. The third gap (one Dagster
per project) was the extension's own and is closed in its repository (devlog_10 there).

---

## What was built

| Piece | What |
|---|---|
| `EkosExtension::binary()` | `(name, version)`; `Extensions::extend_cli` sets the command's name, bin name and `version` to `<version> (EKOS <x>)` |
| `web/api/app/home_projects.py` | `register_new()` (moved from `main.py`) and `workspace_id_for`; called at start-up and by `GET /api/workspaces` |
| tests | `an_extension_binary_has_its_own_name_and_version`; `test_a_project_added_after_start_up_is_listed` |

## Decisions
- Registering on list rather than watching the directory: a scan of `data/<tenant>/` is cheap,
  and a list is exactly when a person expects to see a new project.

---

## Knowledge Captured
- `[skip ci]` on the head commit skips **every** push-triggered workflow, the Pages deploy
  included: after doc changes, `gh workflow run pages.yml` (it has `workflow_dispatch`).
- clap keeps borrowed strings for `version`; an extension's computed version string is leaked
  once per process (`Box::leak`), as clap's own derive does with literals.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/crates/cli/src/extension.rs`, `tests/extension_commands.rs` | `binary()` |
| `web/api/app/{home_projects,main}.py`, `routes/workspaces.py`, `tests/test_home_projects.py` | live home-project registration |
| `CLAUDE.md`, `TODO.md` | notes |
