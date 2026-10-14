# Devlog 270 — Every command and every setting from the console (RFC 0186)

**Date:** 2026-10-14
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

Third of four phases (RFC 0184–0187). The console ran 17 hand-listed commands out of 107 and
edited `ekos.toml` only as text. Now the binary describes itself — `ekos commands --json` (every
command, its arguments, a class saying who may run it) and `ekos config schema --json` (the
schema of `ekos.toml`) — and the console builds a Commands tab and a Settings form from those.
Checked end to end in the package check, in a browser.

---

## RFC 0186

### What was built

| Piece | What it does |
|---|---|
| `commands/catalogue.rs` | Walks clap's tree; per command its arguments (`flag`/`value`/`path` — `PathBuf` detected by the value parser's `TypeId`) and a class from `CLASSES`; guard tests both ways |
| `ekos commands --json`, `ekos config schema --json` | The two descriptions; `schemars::JsonSchema` on all 36 config types |
| `web/api/app/catalogue.py` | Loads them once; `build_argv` (listed args only, `--long=value`, positionals after `--`, paths inside the project / tenant roots, reviewer forced) |
| `routes/cli.py` | `GET/POST /api/workspaces/{id}/cli`, `GET /api/config-schema` |
| `commands.py` `CliCommand`, `runner.resolve` | A validated argv as a run; not reachable from the named-command route |
| `config_io.apply_changes`, `POST …/config/edit` | Field changes applied with `tomlkit`, comments kept, nothing written |
| UI `Commands.tsx`, `ConfigForm.tsx`, `config-schema.ts` | The Commands tab, the Settings form; tests on the real schema as a fixture |
| `deploy/e2e/package_console.py`, `package_browser.py` | A command outside the old list run from the API and from the page; a setting saved through the form |

### Implementation details worth remembering

- `clap::Command::build()` adds a generated `help` subcommand under every group: a walk of the
  built tree must skip `help`, or 200 phantom commands appear.
- A clap variant named `Commands` inside `enum Commands` trips clippy's `enum_variant_names`; the
  variant is `Catalogue` with `#[command(name = "commands")]`.
- `ArgAction` decides a flag (`SetTrue`/`SetFalse`/`Count`); `get_value_parser().type_id() ==
  TypeId::of::<PathBuf>()` decides a path — `AnyValueId` implements `PartialEq<TypeId>`.
- schemars 1 honours serde: kebab-case names, `#[serde(skip)]` fields (no `JsonSchema` bound on
  `ProjectLocation`), `Option<T>` as `["T", "null"]`, doc comments as `description`.

### Decisions

- **The class is a table, not inferred.** Whether `ledger repair` or `migrate v5` may run from a
  browser is a judgement; the guard test makes skipping it impossible.
- **Commands go through the job runner** like the pipeline: queued per workspace, logged,
  cancellable — not a synchronous endpoint.
- **The form writes text, not structure.** Edits go through `tomlkit` on the current text and the
  existing validate/save; sections that are lists of tables stay in the editor rather than a
  half-right generated widget.

---

## Knowledge Captured

- Playwright `button:has-text('Run')` matched another button on the page first; scope selectors
  to the section (`section.commands-form button:has-text('Run')`).
- A project in the `default` tenant has the plain project name as its console id (`shop`), not
  `default.shop`; a test that guesses the id tests the 404 page.
- My own check scripts assumed `query find` had `text`/`--limit`; it has `query` and no limit.
  The catalogue is the source of truth, including for tests.

---

## Verification

`deploy/e2e/package.sh`: 19 stages passed (console stage 20 checks, browser stage 6). Console API
suite, UI suite, Rust workspace tests, clippy, fmt, ruff: see the commit. Not checked: Windows
and macOS (as RFC 0185).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0186-every-setting-and-command-from-the-web.md` | New RFC |
| `ekos/crates/cli/src/commands/catalogue.rs`, `app.rs`, `commands/mod.rs`, `Cargo.toml` | Catalogue, `commands`, `config schema` |
| `ekos/crates/compiler-core/src/config.rs`, `Cargo.toml`, `ekos/Cargo.toml`, lockfiles | `JsonSchema` derives, `schemars` |
| `web/api/app/{catalogue,commands,runner,config_io}.py`, `routes/{cli,config}.py`, `main.py` | Console side |
| `web/api/tests/test_{cli_route,config_edit,authz_routes}.py` | Tests |
| `web/ui/src/pages/{Commands,ConfigForm,config-schema,Config}.tsx/ts`, tests, fixture, `main.tsx`, `WorkspaceShell.tsx`, `api/types.ts`, `index.css` | UI |
| `deploy/e2e/package.sh`, `package_console.py`, `package_browser.py` | Package check stages |
| `README.md`, capabilities page, `CHANGELOG.md`, `TODO.md`, `CLAUDE.md` | Docs |
