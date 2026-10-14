# RFC 0189 — One button to initialise and compile a project

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-14
**Builds on:** RFC 0131 (command runner, chained runs), RFC 0178/0179 (home projects)

## Motivation

A new project in the console needs five commands in order, and a workspace registered in
place may not have an `ekos.toml` yet. The Run page has a `pipeline` card, but it is one card
among twenty and does not initialise. A person who has just added a project wants one
button that makes it queryable.

## The command

A new chained command in the allowlist, `init-and-compile` (write, needs `operate`):

| Stage | argv | Why |
|---|---|---|
| `init` | `init --detect` | creates the state directories; an existing `ekos.toml` is kept (init never overwrites one). The console registers only a workspace that has one, so in practice this checks the workspace is ready |
| `build` → `recover` | as the pipeline | |
| `resolve` | `resolve --force` | identity conflicts are reported in the log and do not stop the run; they stay open for review |
| `compile` | | |
| `commit` | `commit --yes` | |
| `coverage` | `coverage` | says which input kinds produced objects, so an empty result is visible |

**Home projects** (RFC 0178) skip `init`: `ekos project add` already wrote their config, and
`ekos init` refuses to run for a project. The stages are decided when the run is submitted
(`Command.stages_for(path)`), and the run's stage list shows what will run.

Stage arguments become data on the command (`stage_args`), so the existing `pipeline` keeps
its `commit --yes` and its plain `resolve`.

## The button

In the project's top bar, next to its name, on every tab: **Init & compile**, shown to people
who may operate the project. It asks once ("Initialise and compile <name>? This runs build,
recover, resolve, compile and commit."), submits the command and opens the run's page, where
the stages and the log stream as for any run.

## Tests (written first)

Stages and argv for an in-place workspace and for a home project; `resolve --force` and
`commit --yes` only where declared (the pipeline is unchanged); a viewer is refused; a run of
the command against a real fixture with `EKOS_BIN` produces objects; the button is hidden for
a viewer and opens the run after confirming.
