# Devlog 339 — A stop never loses a run; the app's first start; v1.6.1

**Date:** 2026-11-19
**PRs:** none (pushed to `main`, tag `v1.6.1`)
**Branch:** main

---

## Summary

RFC 0261 §2.3 says the app never force-ends a console that has active runs. Until now
`ekos console stop` did exactly that. The console now refuses a stop while runs are queued or
running and names them; `--wait` waits for them and `--force` ends them.
- **New JSON forms:** `status --json` and `stop --json` print what the desktop app needs.
- **Desktop app:** it starts only once, and sends a home that is not set up to EKOS Setup. When
  it quits, it stops the console only when no run is in progress.
- **Console pages:** seven pages that threw on an answer without a list now show their empty
  state.
- **Release:** v1.6.1 ships this together with the console redesign (devlog_337, devlog_338).

---

## A stop never loses a run

### Problem / motivation
`ekos console stop` wrote `stop-requested`, and the console shut down whatever was running.
- **The desktop app:** its quit dialog asked about runs, but the stop underneath did not care.
- **Setup:** replacing an old copy of the program stopped the console the same way.

### What was built
| Component | Change |
|---|---|
| `web/api/app/lifecycle.py` | `decide_stop`: with runs active and no `force` in the request, it writes `stop-refused` (the runs), removes the request and keeps running. `watch` also keeps `active-runs.json` current, reading the database on a worker thread |
| `web/api/app/models.py` | `active_runs()`: queued and running runs, oldest first: id, workspace, command, status, since |
| `commands/console.rs` | `stop_console(home, StopMode::{Ask, Wait, Force})`. It writes the request (`force` for Force), then waits for the pid file to go or for `stop-refused`. Wait asks again every 2 s. A console that answers neither within 20 s is ended, as before. `stop --json` prints `{stopped}` or `{stopped:false, active_runs}`; a refused stop exits non-zero |
| `commands/console.rs` | `status --json`: `{state: running/starting/stopped, set_up, mode, host, port, since, supervisor, console, active_runs}`. `active_runs` is `null` from a console that does not report runs |
| Callers | `ekos uninstall` and the MSI's `StopConsole` pass `--force`, as the RFC asks, after the person confirmed. Setup's "remove the old program" step asks, so a run stops the step instead of being lost |

### Decisions
- **The console decides, not the CLI.** Only the console knows its runs. The CLI cannot sign in, and
  must not read the console's database. The answer travels as a file, like every other part of the
  stop and restart protocol (RFC 0185).
- **Restart is not held.** RFC 0261 covers stop only. A restart from Settings keeps its RFC 0185
  behaviour, and a test pins that.
- **Older consoles.** A console from before this change never refuses and simply stops. An `ekos`
  from before `--json` refuses the flag, and the app then leaves the console running.

---

## The desktop app

| Change | Detail |
|---|---|
| Single instance | `tauri-plugin-single-instance`: a second start shows the first window (RFC 0261 §2.3 step 1). Two apps would each poll, notify, and believe they own the console |
| First start | `status --json`'s `set_up` is false when the home has no `console.toml`. The start-up screen then says "Run EKOS Setup first", with **Open EKOS Setup** (`ekos setup`, the wizard in the browser) and **Try again** |
| State | read from `status --json`, no longer from the words of `status`. The words remain as a fallback for an older `ekos` |
| Quit | stops the console only when it started it **and** no run is in progress (`stop --json`). After "Cancel the run and quit" it asks again for up to 15 s while the cancelled runs end, then leaves the console running if one still holds it |

---

## Pages that threw on a partial answer

Graph, Commands, ekos.toml, Settings, Review, Sources and Conflicts each read a list straight from
an answer. An answer without that list replaced the page with the router's error screen. Each query
now fills the list in with TanStack Query's `select`, and a test pins Review's. Run against
fixtures that leave lists out, none of the 22 pages throws.

---

## Knowledge Captured
- **Checked against a real console**, started from the checkout in a scratch home with a run
  marked `running` in its database:
  - a plain `stop` refused, named the run and exited 1;
  - `status --json` listed the run;
  - `--force` stopped the console, and the run read `interrupted` after the next start (the
    existing startup sweep);
  - `--wait` stopped two seconds after the run was marked done.
- `deploy/e2e/package.sh` passed (61 checks) with the new stop: restart, the uninstall page and
  `uninstall --purge` all go through it.
- **Seen on a virtual screen** (`xvfb-run dbus-run-session`, a scratch `HOME` with no EKOS home):
  - the window opens on "Run EKOS Setup first", with **Open EKOS Setup** focused;
  - a second start exits 0 and the first app keeps running;
  - the console's log box and "Open in browser instead" are hidden on that screen, since no console
    exists before setup.

  In a fresh D-Bus session the first paint took 20–35 s, and the second start's first hand-over
  took up to 15 s. Both are the desktop portal starting for the first time (xdg-desktop-portal is
  activated on demand). The next runs were immediate.
- The `tauri-plugin-single-instance` plugin must be registered first, before the other plugins.

## Files Changed
| File | Change summary |
|---|---|
| `web/api/app/lifecycle.py`, `models.py`, `main.py`, `tests/test_supervised.py` | refusal, `active-runs.json`, tests |
| `ekos/crates/cli/src/commands/console.rs`, `app.rs`, `commands/setup.rs` | stop modes, `--json`, `set_up`, tests; callers |
| `packaging/wix/ekos.wxs`, `deploy/e2e/package.sh` | `console stop --force` |
| `desktop/src-tauri/` (`Cargo.toml`, `src/ekos.rs`, `src/main.rs`, `splash/index.html`) | single instance, status, setup-first screen, quit |
| `web/ui/src/pages/{Graph,Commands,Config,Settings,Review,Sources}.tsx`, `conflicts-shared.ts`, `Review.test.tsx` | lists filled in with `select` |
| version files, `CHANGELOG.md`, `README.md`, `TODO.md` | v1.6.1 |
