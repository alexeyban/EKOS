# RFC 0179 — Installer into the EKOS home, and one production image

**Status:** Accepted — implemented 2026-10-13 (devlog_260)
**Author:** EKOS team
**Created:** 2026-10-13
**Builds on:** RFC 0153 (`install.sh`, release workflow), RFC 0127–0131 (web console), RFC 0178 (EKOS home and projects)
**Part of:** production deployment (0178 home/projects/sources, this RFC, then 0180 tenants/users/roles, 0181 read scope, 0182 MCP gateway, 0183 operations)

## Motivation

RFC 0178 gave EKOS a home directory holding each project's config and compiled state. Nothing
installs into it or runs from it yet:

- `install.sh` puts the binary in `~/.local/bin`, apart from the data.
- There is no deployable image. `web/docker-compose.yml` is a development file: the binary is
  bind-mounted from a build folder and the UI runs under Vite's dev server.
- The console knows only in-place workspaces (a directory with `ekos.toml` and `.ekos/`). It
  cannot run a project from the home, so it cannot work over a source tree mounted read-only.
- The console keeps its own database and run logs in `.ekos-web/` under whatever directory it
  was started from.

## Layout

```
$EKOS_HOME/
  bin/ekos                       installer
  control/console.db             console registry, runs, schedules
  control/runs/                  run logs
  data/<tenant>/<project>/       RFC 0178
```

## 1. Installer

`install.sh` installs to `$EKOS_HOME/bin` (default `~/.ekos/bin`) and creates `$EKOS_HOME/data`.

| Case | Destination |
|---|---|
| `EKOS_INSTALL_DIR` set | that directory (unchanged) |
| `EKOS_HOME` set | `$EKOS_HOME/bin` |
| neither, and `~/.local/bin/ekos` exists | `~/.local/bin` — an earlier install is upgraded where it is, so the `ekos` on `PATH` does not stay old behind a second copy |
| neither | `~/.ekos/bin` |

`EKOS_BASE_URL` overrides the release download location. It exists so the script can be tested
against a local directory; the checksum verification is the same.

## 2. Console runs projects from the home

A registered workspace whose path is `<home>/data/<tenant>/<project>` is a **project**. The
console recognises it from the path; the registry table is unchanged.

| | In-place workspace | Project |
|---|---|---|
| Command line | `ekos <args>` in the workspace directory | `ekos --home <home> --tenant <t> --project <p> <args>` |
| MCP server | `ekos mcp serve --workspace <dir> …` | `ekos --home … --project <p> mcp serve …` |
| Config file | `<dir>/ekos.toml` | `<project dir>/ekos.toml` (the same join) |
| State (query log) | `<dir>/.ekos` | `<project dir>/state` |

One module (`app/home.py`) holds this table. Every place the console starts `ekos` goes through
it.

The home is the `EKOS_HOME` environment variable. Unset, the console has no home and behaves as
before.

### Creating a project

`POST /api/workspaces` accepts `source` in place of `path`:

```json
{"id": "shop", "name": "Shop", "source": "/sources/shop"}
```

- `id` is the project name and must match RFC 0178's name rule.
- `source` must be an existing directory, inside `EKOS_CONSOLE_WORKSPACES_ROOT` when that is set.
- The console runs `ekos project add <id> --source <dir> --detect` (tenant `default`) and
  registers the resulting project directory.
- The UI's form has a second mode for it.

`path` still registers an existing directory. A project directory in the home is accepted with
`state/` where an in-place workspace needs `.ekos/`, and is exempt from the workspaces root: the
root limits which *sources* may be read, and the home is the console's own storage.

At start the console registers every project found under `data/default/` that the registry does
not hold, so a project created with the CLI in the same home appears.

Removing a project in the console runs `ekos project remove` (to `trash/`). Removing an in-place
workspace only deregisters it, as before.

### Editing a project's config

`[workspace] root` and every `[[sources]] path` in a project's `ekos.toml` name directories the
pipeline will read. When `EKOS_CONSOLE_WORKSPACES_ROOT` is set, the config editor refuses a save
that points any of them outside it. Without this the limit on `source` would be bypassed by one
edit.

## 3. Console state under the home

With `EKOS_HOME` set, the console database defaults to `$EKOS_HOME/control/console.db` and run
logs to `$EKOS_HOME/control/runs`. Without it the defaults stay `.ekos-web/…`.
`EKOS_CONSOLE_CONSOLE_DB` and `EKOS_CONSOLE_RUNS_DIR` override both.

## 4. Image

`Dockerfile` at the repository root, three stages:

1. Rust, pinned to `rust-toolchain.toml`'s version: `cargo build --release --locked -p ekos`.
2. Node: `npm ci && npm run build` in `web/ui`.
3. `python:3.12-slim`: the console API and its locked dependencies, the built UI, the `ekos`
   binary, and `git` (the git observer and `semantics` blame call it).

- Runs as user `ekos` (uid 1000), never root.
- `EKOS_HOME=/var/lib/ekos` is the one volume.
- `EKOS_CONSOLE_WORKSPACES_ROOT=/sources`: sources are mounted there, read-only.
- Listens on 8000: the API under `/api`, the UI at `/`. Healthcheck on `/api/health`.
- Build stages and the runtime use the same Debian release, so the binary's glibc matches.

`deploy/docker-compose.yml`: the one service, the named volume, a read-only bind mount of the
source directories, credentials from `deploy/.env` (never built into the image), the port bound
to loopback.

A repository mounted from the host belongs to another user id, which git refuses to read. The
image sets `safe.directory = *` in the system git config: every repository in the container is
one the operator mounted.

## 5. Publishing

`release.yml` gains an `image` job. On a `v*` tag it builds the image and pushes
`ghcr.io/alexeyban/ekos:<version>` and `:latest`. On `workflow_dispatch` it builds and does not
push. It uses the runner's own `docker`, no third-party action.

## Not built

- Tenants other than `default` in the console (RFC 0180). A project of another tenant can be
  registered by path, and runs correctly, but the console has no notion of who may see it.
- `ekos eval run` writes its reports under the working directory, which for a project is the
  source root. On a read-only source it fails. The console's Evals tab shows nothing for a
  project.
- TLS. The image serves plain HTTP on loopback; put a reverse proxy in front.
- Static tokens still work in the image. RFC 0180 makes production refuse them.
- Idle shutdown of per-project MCP servers (RFC 0182). Each registered project holds a server.
- arm64 image. The job builds `linux/amd64` only.

## Verification

- `install.sh` run against a local fake release: each row of the destination table, and a wrong
  checksum still refuses.
- Console unit tests: the table in §2 (command line, state directory), recognising a project
  path, name validation, defaults of §3.
- Console API tests with the real binary: create a project from a source directory, the source
  tree is unchanged afterwards, a source outside the root is refused, the project is listed after
  a restart, removing it moves it to `trash/`, a config save pointing outside the root is refused.
- Every existing console test passes unchanged.
- The image: `docker build`, `docker compose up`, create a project from `/sources` mounted
  read-only, run the pipeline from the API, query it, restart the container, query again.
