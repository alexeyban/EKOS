# RFC 0178 — EKOS home, projects and sources

**Status:** Accepted — Part 1 implemented 2026-10-13 (devlog_258); Part 2 implemented 2026-10-13 for local directories and repositories (devlog_259)
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

## Part 2 — Sources

```toml
[[sources]]
name = "payments-api"      # unique in the project, same name rule as a project
kind = "git"               # git | dir
path = "/sources/payments-api"

[[sources]]
name = "handbook"
kind = "dir"
path = "/sources/handbook"
```

### What was already there

`build` scans every `[observe] paths` entry from its own root, writes a `project` field on each
artifact (`ekos_common::project::project_key_for_base`: the entry's path relative to the
workspace) and hashes ids from `"<key>:<path>"`, so two entries with a file of the same name stay
two objects (RFC 0044 / 0079). This RFC first assumed otherwise. Part 2 names that key, records
it on objects, and lets a source live anywhere.

### Built

- **`[[sources]]` replaces `[observe] paths`** when present. `EkosConfig::observe_bases` is the
  one function that turns either into directories with keys; `build`, `recover`, the config
  preview and the freshness check all read it. A source's key is its name.
- **`kind`** is `dir` or `git`. Both are local directories. `git` states that commit history is
  expected: `build` warns when such a source has no `.git`, where before the git observer
  silently observed nothing.
- **Every object says where it is from.** `source` (one name) and `sources` (sorted list) are
  written on each `File` object at build and on each compiled object at commit, by this rule:
  1. the object's `project` property, when an analyzer recorded the key — exact;
  2. otherwise the sources holding a file one of its evidence records names. A path present in
     two sources attributes the object to both: the list may be too long, never too short;
  3. otherwise, in a project with one source, that source.
  An object none of these reach has neither property. `commit` prints how many, by kind.
- **Freshness** keys a named source's files by `<source>/<path>` and maps them back to disk
  through the source's directory, which need not be under the workspace.
- **Commands.** `ekos project add <name> --source <source>=<dir>` (repeatable; the first is the
  root), `ekos source add|list|remove`, `ekos build --source <name>` (re-observes one source and
  keeps the others' recorded files).
- **Queries.** `source` is an ordinary property, so EKL reads it (RFC 0175):
  `FIND Object WHERE source = 'handbook'`.

### Rules

- **A compiled project without source names cannot gain them.** Its ids were hashed without a
  key and the ledger is append-only; naming sources afterwards would leave every object beside a
  renamed copy. `ekos source add` refuses and says to create a new project.
- **Renaming a source is removing one and adding another.** Its objects get new ids.
- **Removing a source edits the config only.** Compiled objects stay in the ledger.
- Recover, resolve, compile and commit always run over the whole project: identity and links
  span sources.

### Not built

- `confluence` and `github` as source kinds. There is still one `[confluence]` and one
  `[github]` per project.
- `source` on objects written by the steps after the main commit loop: rollups, business
  semantics, label conflicts, LLM descriptions.
- A commit author (`Person`) has no source: its evidence is a commit, not a file.
- A `--source` flag on `ekos query find` / `ekos ask` and a `source` argument on MCP tools.

RFC 0181 must treat an object without `sources` as not visible to a reader restricted by source.

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
- Part 2, end to end: a project of two git repositories and a document folder. A file of the
  same name in both repositories is two objects with different `source`; each source has
  objects; commits from both repositories are found; one source rebuilt alone keeps the others'
  manifest entries; the kinds left without a source are pinned.
