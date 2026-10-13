# Devlog 260 — Installer into the EKOS home, and one production image (RFC 0179)

**Date:** 2026-10-13
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

Phase B of the production plan. `install.sh` now installs into the EKOS home. A root `Dockerfile`
builds one image holding the `ekos` binary, the console API and the built console UI, and
`deploy/docker-compose.yml` runs it with one volume and the sources mounted read-only. The
console could not run a project kept in the home, which a read-only source needs, so that was
added: it creates, runs, lists and removes home projects, and keeps its own database in the
home. Verified in the container end to end. No Rust code changed.

---

## RFC 0179

### Problem / motivation

RFC 0178 made a home directory for config and compiled data. Nothing installed into it, there
was no deployable image, and the console knew only a directory with `ekos.toml` and `.ekos/`
inside the source tree.

### What was built

| Part | Change | Where |
|---|---|---|
| Installer | Binary to `$EKOS_HOME/bin` (default `~/.ekos/bin`), creates `data/`. `EKOS_INSTALL_DIR` still wins; an earlier `~/.local/bin/ekos` is upgraded where it is. `EKOS_BASE_URL` for a mirror or a local test | `install.sh` |
| Installer tests | Seven cases against a fake release fetched over `file://`, including a wrong and a missing checksum | `scripts/test-install.sh` |
| Console: projects | A workspace whose path is `<home>/data/<tenant>/<project>` is run with `--home/--tenant/--project`; its state directory is `state/` | `web/api/app/home.py` |
| Console: create | `POST /api/workspaces` with `source` runs `ekos project add <id> --source <dir> --detect` | `routes/workspaces.py`, `schemas.py` |
| Console: remove | A project is removed with `ekos project remove` (to `trash/`) | `routes/workspaces.py` |
| Console: start | Projects of tenant `default` missing from the registry are registered | `main.py` |
| Console: config guard | A save whose `[workspace] root` or `[[sources]] path` leaves `EKOS_CONSOLE_WORKSPACES_ROOT` is refused | `config_io.py`, `routes/config.py` |
| Console: state | With `EKOS_HOME`, database and run logs default to `<home>/control/` | `settings.py` |
| UI | The add form has two modes: new project from a source directory (default), existing workspace | `pages/Workspaces.tsx` |
| Image | Three stages (Rust, Node, Python runtime), non-root, `EKOS_HOME=/var/lib/ekos`, healthcheck | `Dockerfile`, `.dockerignore` |
| Compose | One service, named volume, `/sources` read-only, loopback port, credentials from `deploy/.env` | `deploy/` |
| Release | `image` job: build and start on every run, push to `ghcr.io` on a tag | `.github/workflows/release.yml` |

### Implementation details worth remembering

- **A project is recognised from its path, not from a new column.** The registry table is
  unchanged, so there is no schema migration. RFC 0180 changes the model anyway.
- **`home.command` and `home.mcp_command` are the only builders of an `ekos` command line for a
  workspace.** Six call sites were changed (two in `readproc`, one in `semantics_write`, two in
  the runner, one in the supervisor). A new one that bypasses the module would run `ekos` in the
  project directory, find `./ekos.toml` there, and use `./.ekos` instead of `state/`: a second,
  empty ledger with no error.
- **`ekos mcp serve` takes no `--workspace` for a project.** The CLI refuses the two together.
- **`config validate --file` is not `--config`**, so validating an edited config works with
  `--project`.
- **The config path is the same join in both cases** (`<path>/ekos.toml`), so the editor's file
  code needed no change.
- **The image has no Rust toolchain.** `ekos doctor` already reports that as fine.

### Decisions

- **The workspaces root does not apply to a project directory.** The root limits which sources
  may be read. The home is the console's own storage. The sources are checked when the project
  is created and again on every config save.
- **Console removal of a project runs `project remove`.** Deregistering alone would bring the
  project back at the next start, since a project exists because its directory does.
- **An earlier install is upgraded in place.** A second binary in `~/.ekos/bin` would leave the
  old one first on `PATH`, and the user would keep running it.
- **No third-party action in the image job.** It holds a token that can publish a package; the
  runner's `docker` is enough.
- **`linux/amd64` only.** An arm64 image needs emulation or a second runner.

---

## Verification

| Check | Result |
|---|---|
| Console API tests (`uv run pytest`, real binary) | 178 pass, 36 of them new |
| Console UI (`tsc`, `vitest`) | clean, 81 pass |
| `scripts/test-install.sh` | all cases pass |
| `docker build` | builds, 572 MB |
| Container | uid 1000, `/sources` read-only, health and UI on one port |
| Create project from `/sources/shop` | 201, MCP server ready |
| Pipeline from the API | five stages succeeded, 2 tables, 1 document, 1 commit author |
| Source directory afterwards | `git status` clean, no new files |
| Source outside `/sources`, read token creating | 400, 403 |
| Config save pointing at `/var/lib/ekos` | 422 |
| Restart | project, objects and run history kept; a CLI-created project appeared |
| Remove | moved to `trash/default/` |

The fixture had no `[llm]` section, so no model was called.

Not verified: the `image` job on GitHub (it runs on the next tag or a manual dispatch), and the
UI in a browser against the container (only that the page is served).

---

## Knowledge Captured

- **`RUN --mount` fails without BuildKit, and BuildKit is not everywhere.** Ubuntu's `docker.io`
  package ships without the buildx component; `docker build` then stops at the first cache
  mount, and `DOCKER_BUILDKIT=1` fails differently. The Dockerfile uses nothing BuildKit-only.
- **`rust:<version>-slim` plus a `rust-toolchain.toml` that lists components downloads them.**
  The slim image has the minimal profile. The pin still holds; the build pays a download.
- **Compose's `env_file` and `--env-file` are different things.** `--env-file` only feeds
  `${...}` in the YAML. The container's environment comes from `env_file:`. `deploy/.env` serves
  both because Compose also reads `.env` in the project directory for interpolation.
- **A host repository mounted into a container belongs to another uid**, and git refuses it.
  `git config --system --add safe.directory '*'` in the image; the git observer then found the
  commit.
- **`mkdir` and `chown` the volume path in the image before `VOLUME`.** A new named volume
  copies the owner from the image; without it the non-root user cannot write its own home.
- **A console setting whose default depends on another setting** is an empty string filled in by
  a validator. A literal default cannot tell "unset" from "set to the old default".
- **`ekos eval run` writes reports under the working directory**, which for a project is the
  source root. It fails on a read-only source. Open item.
- **`web/ui/node_modules/.vite` is owned by root on this machine** (left by the development
  Compose file). `vitest` passes and then prints an `EACCES` on its cache. Not a test failure.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0179-installer-and-image.md` | New |
| `install.sh` | Destination rules, `EKOS_BASE_URL`, creates `data/` |
| `scripts/test-install.sh` | New |
| `Dockerfile`, `.dockerignore` | New |
| `deploy/docker-compose.yml`, `deploy/.env.example` | New |
| `.github/workflows/release.yml` | `image` job; `verify` uses `~/.ekos/bin` |
| `web/api/app/home.py` | New |
| `web/api/app/settings.py` | `ekos_home`, defaults under the home |
| `web/api/app/routes/workspaces.py` | Create and remove projects, accept a project directory |
| `web/api/app/routes/config.py`, `config_io.py` | Source-root guard on save |
| `web/api/app/{readproc,semantics_write,runner,supervisor}.py`, `routes/stats.py` | Command lines and state directory through `home` |
| `web/api/app/main.py`, `schemas.py` | Register home projects at start; `source` on create |
| `web/api/tests/test_home.py`, `test_projects_live.py` | New |
| `web/ui/src/pages/Workspaces.tsx`, `.test.tsx`, `index.css` | Two-mode add form |
| `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `TODO.md`, `web/README.md`, `docs/generated/ekos-self-documentation.html` | Documented |
