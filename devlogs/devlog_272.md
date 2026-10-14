# Devlog 272 — Uninstall in the browser, Init & compile, first CI on Windows and macOS

**Date:** 2026-10-14
**PRs:** none (local main; pushed with the v1.1.0 release)
**Branch:** main

---

## Summary

Two console features and the fixes from the first CI and release-build runs of RFC 0184–0187.
`ekos uninstall --ui` and the console's Settings → Uninstall EKOS show what will be removed,
with the projects and their size, and remove EKOS after a typed confirmation (RFC 0188). A
project's bar has **Init & compile**, one run from init to a coverage report (RFC 0189). The
first runs on GitHub found a formatting slip, two tests that assumed `/bin/false`, an image build
broken by an empty build argument, and no Intel-macOS wheels for `cryptography`.

---

## RFC 0188 — Uninstall in the browser

### What was built

| Piece | What it does |
|---|---|
| `setup::uninstall_plan` | Pure: the steps `uninstall` will take, in its order, and the projects in the home with their size |
| `uninstall_server.rs` + `uninstall.html` | `ekos uninstall --ui`: axum on loopback, the setup wizard's token check (`setup_server::token_matches`), `GET /api/plan`, `POST /api/uninstall {purge, confirm}` |
| `console::detach_uninstall_page` | `--detach`: the page in a new process (its own process group, or `systemd-run --user` under the login service), prints its link once it answers |
| `routes/platform.py` | `GET/POST /api/platform/uninstall`: local mode and `install.json` only; starts the page and returns its link, refusing any link that is not `http://127.0.0.1:` |
| `Settings.tsx` | **Uninstall EKOS…** section; follows only a loopback link (`uninstallTarget`) |

### Implementation details worth remembering

- **The page outlives what it removes.** It is served from the binary's memory, so it keeps
  answering while `bin/`, `console/` and (with purge) the home disappear, and shows each step's
  result. It stops 2 s after answering — before Windows' delayed `rmdir` of `bin\` (3 s) finds
  the program still in use.
- **A detached child is not enough under systemd.** `uninstall` runs `systemctl --user disable
  --now ekos-console.service`, and stopping a unit kills every process in its cgroup, however
  it was detached. When the manifest says the login service is systemd and `systemd-run`
  exists, the page starts in a transient unit of its own, its output to a file in the temp
  directory (not the home, which a purge removes).
- **The confirmation word depends on purge**: `uninstall`, or `delete everything`. The word for
  keeping the home does not purge (tested).

### Decisions

- **One uninstaller, two doors.** The console does not remove anything itself: it starts the
  binary's page and sends the browser there. A console removing itself could not report the
  result.
- **Local mode only in the console.** The page binds the console machine's loopback; a team
  owner's browser is elsewhere. A server is removed with `ekos uninstall` on it.

---

## RFC 0189 — Init & compile

### What was built

| Piece | What it does |
|---|---|
| `commands.py` | `init-and-compile`: `init --detect` → build → recover → `resolve --force` → compile → `commit --yes` → coverage; `Command.stage_args`, `stage_argv`, `stages_for` |
| `runner.py` | Stage list from `stages_for(path)` at submit; each stage's argv from the command |
| `InitAndCompile.tsx` | The button in the project bar (operators), a confirm, then the run page |

### Implementation details worth remembering

- The console registers only a workspace that already has an `ekos.toml`, so `init` there only
  makes sure the state directories exist — `init` never overwrites a config. A home project skips
  `init` (`ekos init` refuses a project; `project add` initialised it).
- `resolve --force` only in this command: the plain `pipeline` keeps stopping on identity
  conflicts. The stage argv moved from a special case in the runner (`commit` → `--yes`) to data
  on each command.

---

## CI and release fixes (first runs of RFC 0184–0187 on GitHub)

| Failure | Cause | Fix |
|---|---|---|
| Build & Test: `cargo fmt --check` | `pub mod catalogue;` before `build` in `commands/mod.rs` — per-file `rustfmt` had been run, but not on `mod.rs` | reordered; `cargo fmt --all --check` before committing |
| Web console API (macOS) | `/bin/false` is `/usr/bin/false` on macOS | `shutil.which("false")` in the three tests |
| Release: Build the image | A build arg is an environment variable inside `RUN`; cargo reads `CARGO_BUILD_JOBS` itself and rejects an empty one | unset it when empty |
| Release: Full package x86_64-apple-darwin | `cryptography` 50 publishes no Intel macOS wheels; the package installs with `--only-binary` | no full package for Intel Macs; they get the command (install.sh falls back) |

---

## Knowledge Captured

- **`ARG X=""` exports an empty `X` to `RUN`.** Any tool that reads `X` from its environment
  sees it set-but-empty. Cargo's `CARGO_BUILD_JOBS=""` is a hard error. Locally the build always
  passed because `--build-arg CARGO_BUILD_JOBS=4` was always given.
- **`systemctl --user stop` kills the whole cgroup.** `setsid`/`process_group(0)` does not take
  a child out of its parent's unit; `systemd-run --user` does.
- **`[skip ci]` hid the formatting slip and the macOS path for six commits.** `ci.yml` now has
  `workflow_dispatch` so the matrix can be run on demand after landing local work.
- `cryptography` dropped x86_64 macOS wheels at 50.0; any `--only-binary` bundle for Intel Macs
  needs an older pin or a source build.

---

## Verification

- `deploy/e2e/package.sh`: 19 top-level checks; console stage 26; browser stage 10 (Init &
  compile through coverage); new stage 5, uninstall from Settings in a browser, 6 (the page
  lists the project, the button is disabled before the word, every step done, projects kept).
- The first version of stage 5 found a real fault: the console waited for end of file on
  `ekos uninstall --ui --detach`'s output, but uvicorn's event loop (uvloop) does not mark the
  pipe close-on-exec, so the detached page inherited it and EOF never came. The route now reads
  lines until the link (and does not call `wait()`, which in asyncio also waits for the pipes);
  `--detach` stops waiting as soon as the page process exits (it waited 20 s for a page that had
  died at once, e.g. without `HOME`). A regression test holds the pipe open with `sleep &`.
- Gates: cargo fmt, clippy `-D warnings`, `cargo test --workspace --no-fail-fast` (166 result
  lines, no failure), integration 5, `scripts/audit.sh` exit 0, ruff, pytest 419 + 1 live test
  that failed only while the package check ran beside it (passes alone), vitest 109, tsc.
- CI: the first dispatched run's two console jobs hung in pytest on Linux and Windows with no
  output (buffered `-q` progress, no timeout) and were cancelled after an hour; both now run
  `-v -o faulthandler_timeout=300` under a job timeout, so the next run names the test.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0188-uninstall-from-the-browser.md`, `0189-init-and-compile-button.md` | New RFCs |
| `ekos/crates/cli/src/commands/{setup,setup_server,console}.rs`, `uninstall_server.rs`, `uninstall.html`, `setup_tests.rs`, `mod.rs`, `app.rs` | Uninstall plan, page, `--ui/--detach` |
| `web/api/app/routes/platform.py`, `commands.py`, `runner.py` | Uninstall routes; init-and-compile; stage arguments |
| `web/api/tests/test_{local_mode,console_settings,commands,runner_live,api,tenants,readproc}.py` | Tests; portable `false` |
| `web/ui/src/pages/Settings.tsx` (+ test), `InitAndCompile.tsx` (+ test), `WorkspaceShell.tsx`, `index.css` | Uninstall section, button |
| `deploy/e2e/package.sh`, `package_browser.py`, `package_browser_common.py`, `package_uninstall.py` | Button and browser uninstall in the package check |
| `.github/workflows/ci.yml` | `workflow_dispatch`; job timeouts; `-v` + faulthandler for the console tests |
| `Dockerfile`, `.github/workflows/release.yml` | Empty build arg; no Intel macOS full package |
| README, `docs/deployment.md`, `deploy/package/INSTALL-full.md`, capabilities page, CHANGELOG (v1.1.0), TODO, CLAUDE.md | Docs |
