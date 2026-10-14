# RFC 0188 — Uninstall from the browser

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-14
**Builds on:** RFC 0184 (console settings, local mode), RFC 0185 (`ekos setup`, `ekos uninstall`, `control/install.json`)

## Motivation

EKOS installs with a page in the browser (RFC 0185), but removing it needs a terminal:
`ekos uninstall [--purge --yes]`. Someone who installed with the wizard should be able to
remove EKOS the same way: from the console, or from a page started by the command, seeing
what will be removed before anything is.

## 1. `ekos uninstall --ui`

The binary serves one embedded page on `127.0.0.1:<port>`, as the setup wizard does: a
random token in the link, required on every request (`?t=` for the page, `x-setup-token` for
the API). It stops when the removal is done, or after an hour without a request.

| Route | Does |
|---|---|
| `GET /` | the page |
| `GET /api/plan` | the home, the program directory, whether setup installed this (`install.json`), the steps of a removal with and without the home, and the projects in the home with their size |
| `POST /api/uninstall` | `{purge, confirm}` → runs the removal, returns each step's result |

**Confirmation.** `confirm` must be `uninstall`; with `purge`, it must be `delete everything`.
The page asks the person to type it. A wrong word is 400 and changes nothing.

The steps shown are computed by `setup::uninstall_plan`, a pure function over the home and
`install.json`. A test runs `uninstall` with recorded effects and checks it performs exactly
the steps the plan listed, in order, so the page never promises one thing and does another.

The page is served from the running program's memory, so it keeps working while the program
directory, the console and (with `purge`) the home are removed, and shows the result of each
step at the end.

**Flags.** `--no-browser` prints the link; `--port`; `--detach` starts the page in a new
process, outside this one's process group (on Linux, outside the console's systemd unit with
`systemd-run --user` when that is available — stopping the unit would otherwise end the
uninstaller with it), prints its link once the page answers, and exits. `--ui` cannot be
combined with `--purge` / `--yes`: the page asks.

## 2. In the console

The Settings page (platform owners) gets a section **Uninstall EKOS**:

| Route | Does |
|---|---|
| `GET /api/platform/uninstall` | `{available, reason}` |
| `POST /api/platform/uninstall` | starts `ekos --home <home> uninstall --ui --detach`, returns `{url}` |

Available only when the console runs in **local mode** (the page is on this machine's
loopback; a team server's owner reaches it from another machine) and the home has
`control/install.json` (installed by `ekos setup`; a container or a hand-run console is
removed another way). Otherwise 409 with the reason. The page then opens the link; nothing is
removed until the person confirms there.

The process gets an environment from `envs.for_tenant(None)` like every other `ekos` process
the console starts.

## Tests (written first)

- The plan lists exactly what `uninstall` does (recorded effects), with and without `purge`.
- Every route of the page needs the token; a wrong confirmation word changes nothing; the
  right one runs the removal (recorded effects) and returns its steps.
- `--ui` with `--purge` is refused.
- Console: owner only; 409 outside local mode and without `install.json`; in local mode the
  command started is `uninstall --ui --detach` with the home, and its link is returned.
- UI: the section shows the reason when unavailable and follows the link otherwise.
- Package check: the console's button starts the page; the browser removes EKOS through it.

## Not built

Uninstalling a team server from the browser; removing one project (that is `ekos project
remove`, already in the console).
