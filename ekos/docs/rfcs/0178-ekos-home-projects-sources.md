# RFC 0178 — EKOS home, projects and sources

**Status:** Accepted — Part 1 implemented 2026-10-13 (devlog_258); Part 2 designed in outline, one point open, not started
**Author:** EKOS team
**Created:** 2026-10-13
**Builds on:** RFC 0111 (`[storage]` containers, `EkosConfig::ekos_dir`), RFC 0152 (`init --detect`), RFC 0171 (freshness)
**Part of:** production deployment (this RFC, then 0179 packaging, 0180 tenants/users/roles, 0181 read scope, 0182 MCP gateway, 0183 operations)

## Motivation

EKOS is used per repository. `ekos.toml` and the `.ekos/` state directory sit inside the source
tree, and every command is run from that tree. That is right for one developer and one
repository. It does not fit a shared installation:

- a source tree mounted read-only cannot hold `.ekos/`;
- nothing says which compiled workspaces exist on a machine;
- a project made of several repositories, a document folder and a Confluence space is only
  half represented. `[observe] paths` may list several directories, each is scanned from its own
  root, and RFC 0044/0079 keep their objects apart by a key derived from the path. But the key
  is a path, not a name, it is recorded on some objects and not others, there is one
  `[confluence]` and one `[github]` per workspace, and nothing can be asked "from which source";
- there is no level above a workspace to hang a tenant on.

## Levels

**Tenant → Project → Source → object.**

- A **tenant** is a directory tree. Nothing else in this RFC; permissions are RFC 0180.
- A **project** is one `ekos.toml` and one ledger.
- A **source** is one named input of a project: a git repository, a directory, a Confluence
  space. (Part 2.)
- An **object** is what it is today: a module, document, file, table.

## Part 1 — Home and projects

### Layout

```
$EKOS_HOME/                         default ~/.ekos
  data/<tenant>/<project>/
      ekos.toml                     [workspace] root = "/abs/source/dir"
      state/                        what `.ekos/` is today
  trash/<tenant>/<project>-<time>/  a removed project, until purged
```

`$EKOS_HOME` comes from `--home`, then the `EKOS_HOME` variable, then `~/.ekos`.

A project exists exactly when `data/<tenant>/<project>/ekos.toml` exists. There is no registry
file. The tenant defaults to `default`.

Names (tenant and project) match `[a-z0-9][a-z0-9_-]{0,62}`. Anything else is refused before a
path is built from it.

### Selecting a project

Global flags `--project <name>` (variable `EKOS_PROJECT`) and `--tenant <name>` (`EKOS_TENANT`).

| | Without `--project` | With `--project` |
|---|---|---|
| Config | `--config`, `EKOS_CONFIG`, `./ekos.toml` | `data/<tenant>/<project>/ekos.toml` |
| Working directory | where the command was run | `[workspace] root` of that config |
| State directory | `./.ekos` (or a `[storage]` container) | `data/<tenant>/<project>/state` |

Without `--project` nothing changes.

With `--project`:

- The config must exist and parse. A missing or broken project config is an error, never a
  silent default: the fallback to defaults is for a developer's first run in a fresh directory.
- `[workspace] root` must be absolute and an existing directory.
- The process changes into the source root once, before any command runs, so every relative path
  a command or config names resolves there.
- `--config` together with `--project` is refused. `--tenant` without `--project` is refused.
- `ekos init` is refused: it writes `ekos.toml` into the working directory, which is now a source
  tree. `ekos project add` is the equivalent.
- Nothing is written into the source tree.

### One decision point

`EkosConfig::ekos_dir` stays the only function that decides the state directory. `EkosConfig`
gains a runtime-only field (`location`, never read from or written to a file) that it returns
first. Every path under the state directory already hangs off this function.

Two things were found built from the working directory instead:

| Path | Change |
|---|---|
| Session inbox default, `.ekos/session/inbox` under the workspace | With a project: `state/session/inbox`. The inbox's containment check ("the inbox may not resolve outside the workspace") is kept, against the state directory; anchors are still checked against the source root |
| Default output of `docs generate`, `architecture investigate` and `dbt generate` (`./docs-generated`, `./doc`, `./dbt-generated`) | With a project: under `data/<tenant>/<project>/out/`. An explicit `--output` is unchanged |

### Commands

```
ekos project add <name> --source <dir> [--tenant <t>] [--detect]
ekos project list [--json]
ekos project show <name> [--tenant <t>] [--json]
ekos project remove <name> [--tenant <t>] [--purge --yes]
```

- `add` creates the project directory and state tree and writes `ekos.toml`: the default config,
  or with `--detect` the RFC 0152 scan of the source directory, with `[workspace] root` set to
  the absolute source path. It refuses an existing project.
- `remove` moves the project to `trash/`. `--purge --yes` deletes it. Nothing in the source tree
  is touched by either.
- `ekos mcp serve --project <name>` serves that project. `--workspace` with `--project` is
  refused.
- `ekos doctor` reports the home, tenant, project, source root and state directory.

### Not in Part 1

- `ekos marketing`, which reads and writes `marketing/` and `devlogs/` in the working directory.
  It is repository tooling, not something a deployed project runs.

- Moving an existing `./.ekos` workspace into the home. It is a copy of `.ekos/` to `state/`
  and of `ekos.toml`, documented, not automated.
- The cluster compile worker, which ships a config between processes and loses the runtime
  location.

## Part 2 — Sources (designed, not built)

```toml
[[sources]]
name = "payments-api"      # unique in the project, same name rule as a project
kind = "git"               # git | dir | confluence | github
path = "/sources/payments-api"

[[sources]]
name = "runbooks"
kind = "confluence"
space = "OPS"
```

This builds on what `build` already does (found while implementing Part 1, and different from
what this RFC first assumed): every `[observe] paths` entry is scanned from its own root, each
artifact gets a `project` field (`ekos_common::project::project_key_for_base`, the entry's path
relative to the workspace), and ids are hashed from `"<key>:<path>"` so two entries with a file of
the same name stay two objects. Displayed names stay relative to the entry.

- A source's **name** replaces the path-derived key. An absolute path outside the workspace
  then has a stable, readable key instead of its full path.
- The four places that turn `[observe] paths` into directories (`build.rs`, `recover.rs`,
  `commands/config.rs`, `freshness.rs`) read one shared function that returns sources.
- Every object gets a `source` property. Today the key reaches `File` objects and the analyzers
  that read it back from their artifact, not every object. Open design point: a symbol's
  knowledge artifact does not record which observation artifact it came from (the Rust analyzer
  writes one for all files), so `source` cannot be derived generically from provenance and has to
  be carried by each analyzer or by evidence. To be settled before code.
- An object merged from several sources lists all of them; derived objects (rollups, risks,
  summaries, business concepts) list the sources of what they were derived from.
- The object property is called `source`. The existing `project` field keeps its name and its
  role in id hashing, so no existing ledger's ids change; "project" in this RFC's sense is the
  level above.
- `[confluence]` and `[github]` become per-source settings.
- A config with no `[[sources]]` is one unnamed source at the workspace root, as today.
- `ekos source add|list|remove`, `ekos build --source <name>`. Recover, resolve, compile and
  commit run over the whole project: identity and links span sources.
- Read commands and MCP tools take a `source` filter; EKL reads `source` like any property
  (RFC 0175).
- A guard test fails when an object reaches the ledger with no `source`.

One ledger per source was considered and rejected: no identity resolution or links across
sources, which is the reason to put them in one project.

## Verification

- Unit tests for name validation and for the resolution table above.
- End to end: the pipeline run with `--project` over a temp source tree leaves that tree
  byte-identical (no `.ekos`, no `ekos.toml`) and puts state under `data/default/<p>/state`.
- Two tenants with a project of the same name: independent ledgers; a search in one does not
  return the other's objects.
- `project remove` moves to `trash/`; `--purge` needs `--yes`.
- Every existing test passes unchanged (no `--project`).
