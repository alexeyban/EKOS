# Devlog 258 — EKOS home and projects: config and compiled data outside the sources (RFC 0178, Part 1)

**Date:** 2026-10-13
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

First step of making EKOS deployable as a shared installation. A project can now live in an EKOS home (`~/.ekos` by default): its `ekos.toml` and everything that used to be `.ekos/` sit under `<home>/data/<tenant>/<project>/`, and the sources are only read. Each project has its own ledger, and tenants are separate directory trees.

Nothing changes for a workspace used in place: without `--project` every command behaves as before.

This is Part 1 of RFC 0178. Part 2 (a project made of several named sources) is designed there and not built. Packaging, tenants and users, access control inside a project and the MCP gateway are RFCs 0179 to 0183, not started.

---

## Why

The request was a production setup: an installer or image, EKOS in its own folder, compiled data under it, a ledger per project and per tenant, users, groups and roles, and projects made of several repositories and document sets. The agreed plan has six phases. This one is the base for the others: nothing can be packaged or shared while state must sit inside a source tree.

---

## What was built

| Component | Where |
|---|---|
| Home resolution, name validation, project selection, project listing | `ekos/crates/cli/src/home.rs` (new) |
| `ekos project add / list / show / remove` | `ekos/crates/cli/src/commands/project.rs` (new) |
| Global `--home`, `--project`, `--tenant`; `EKOS_HOME`, `EKOS_PROJECT`, `EKOS_TENANT` | `ekos/crates/cli/src/app.rs` |
| `ProjectLocation`; `EkosConfig::location`, read first by `ekos_dir`; `output_base` | `ekos/crates/compiler-core/src/config.rs` |
| Session inbox under the state directory for a project | `ekos/crates/session/src/inbox.rs` (`open_under`), `commands/session.rs` (`inbox_at`) |
| `doctor` reports the project | `commands/doctor.rs` |

### How a project is selected

| | Without `--project` | With `--project` |
|---|---|---|
| Config | `--config`, `EKOS_CONFIG`, `./ekos.toml` | `<home>/data/<tenant>/<project>/ekos.toml` |
| Working directory | where the command was run | `[workspace] root` of that config |
| State | `./.ekos` or a `[storage]` container | `<home>/data/<tenant>/<project>/state` |

### Implementation details worth remembering

- **One function still decides the state directory.** `EkosConfig::ekos_dir` returns the project's `state/` when `location` is set. `location` is `#[serde(skip)]`: a config written back to disk never carries a machine's paths. Every path that hung off `ekos_dir` moved with no further change.
- **Two things did not hang off it.** The session inbox defaulted to `.ekos/session/inbox` joined to the workspace, and `docs generate`, `architecture` and `dbt` default their output to the working directory. For a project the inbox is `state/session/inbox` and those outputs go to `<project>/out/`.
- **The inbox keeps its containment check.** It refused an inbox that resolves outside the workspace. For a project the same check runs against the state directory, and anchors are still checked against the sources.
- **The process changes directory once.** With a project, `main_with` enters the source root before dispatch. Relative paths in a config or on a command line then mean what they meant in place.
- **A project config is loaded strictly.** `from_file_or_default` turns a broken `ekos.toml` into defaults, which is right for a first run in an empty directory. For a named project it would compile nothing and report success, so `load_project` uses `from_file` and requires an absolute, existing `[workspace] root`.
- **`project remove` moves, it does not delete.** The project goes to `<home>/trash/<tenant>/<project>-<time>`. `--purge` deletes and needs `--yes`.
- **Names are validated before any path is built**: `[a-z0-9][a-z0-9_-]{0,62}`.

### Decisions

- **No registry file.** A project exists when its `ekos.toml` exists. A list kept beside the directories would drift from them.
- **Refused with `--project`:** `--config` (a second config), `mcp serve --workspace` (a second workspace), `init` (writes `ekos.toml` into the working directory, which is now a source tree).
- **`[workspace] root` gets a meaning.** It was parsed and never read. For a project it is the source directory.
- **A project location wins over a `[storage]` container.** Both redirect the state directory; the one named on the command line decides.

---

## A dependency advisory found on the way

`scripts/audit.sh` passed on 2026-10-12 and failed on 2026-10-13 with no dependency change: five new advisories for `quinn-proto` (RUSTSEC-2026-0338 to 0342), a transitive dependency. `cargo update -p quinn-proto` in `ekos/` and `tests/integration/` moved it to 0.11.19. Separate commit.

---

## Knowledge Captured

- **A command that echoes its input defeats a test that greps for it.** `query find` prints "No objects found matching '…'" with the query in it, so a test that looks for the search word in the output passes on a miss. Assert on the result line.
- **A blanket rename inside one function reaches the file's tests.** Replacing `cli.command` after a split point also rewrote a test further down. Bound the replacement to the function.
- **`if let Variant { field } = value` moves `value` for the rest of the function.** To handle one enum variant early and keep the rest, `let rest = match value { Variant { .. } => return …, other => other };`.
- **The default home `~/.ekos` is also what `./.ekos` means when `ekos` is run in the home directory.** The names inside do not collide (`data/`, `trash/` against `artifacts/`, `ledger/`), but they would share a directory.
- **The audit gate depends on the day.** A green audit says nothing about tomorrow; run it before every push.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0178-ekos-home-projects-sources.md` | New |
| `ekos/crates/cli/src/home.rs` | New: home, names, selection, listing, 5 tests |
| `ekos/crates/cli/src/commands/project.rs` | New: add, list, show, remove, 5 tests |
| `ekos/crates/cli/src/app.rs` | Global flags, `ProjectCommands`, selection before the config is loaded |
| `ekos/crates/compiler-core/src/config.rs`, `lib.rs` | `ProjectLocation`, `location`, `output_base`, test |
| `ekos/crates/session/src/inbox.rs`, `ekos/crates/cli/src/commands/{session,mcp}.rs` | Inbox under the state directory for a project, test |
| `ekos/crates/cli/src/commands/{doctor,init}.rs` | Project check; template and detection printer shared |
| `ekos/crates/cli/tests/project_home.rs` | New: three end-to-end tests through the binary |
| `ekos/Cargo.lock`, `tests/integration/Cargo.lock` | `quinn-proto` 0.11.19 |
| `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Documented |
