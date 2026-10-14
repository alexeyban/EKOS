# Devlog 273 — v1.1.0 released; upgrading a real install; stage time limits

**Date:** 2026-10-14
**PRs:** none (main, pushed: 96e8b49 … 82a268f; tag v1.1.0 on 89faa70)
**Branch:** main

---

## Summary

EKOS v1.1.0 is published: full packages (command + web console + Python) for Linux x86_64 and
aarch64, macOS on Apple silicon and Windows, command-only archives for six targets, the
console image, and an install check against the published release — every release job green.
Getting there took four CI rounds, which found a test hang under Python 3.12, a Node version
too old for jsdom, real Windows bugs, an MCP port collision and four new rustls advisories.
Upgrading the maintainer's own install afterwards found two more: `install.sh` could not
replace a running binary, and a chained run's commit stage was killed after 30 minutes on a
95 000-object project.

---

## The release

| Round | Commit | Found | Fixed |
|---|---|---|---|
| 1 | 9110a17 (dispatched) | `cargo fmt` (mod order), `/bin/false` on macOS, empty `CARGO_BUILD_JOBS` breaks the image, no Intel-macOS `cryptography` wheels, console tests hung with no output | devlog_272 |
| 2 | b25f91b | the hang again — now with `-v` and `faulthandler_timeout` | — |
| 3 | 96e8b49 | the hang named: a fake server never closed its connections (3.12 `wait_closed`); 13 Windows-only test assumptions | test fixes |
| 4 | b455bbc | jsdom 30 needs Node ≥ 22.22; Windows: SPA path `\`, CRLF in logs, `\\?\` paths; MCP port in use | Node 22 everywhere, three Windows fixes, free-port check |
| — | 7b69573 | Audit: RUSTSEC-2026-0346…0349 (rustls), published the same day | rustls 0.23.46 in all three workspaces |
| 5 | 89faa70 | last Windows failure: `ekos status --json` canonicalized to `\\?\` | `home::canonical_dir` there too → **all green, tagged** |

The release workflow then passed every job; the image job, which failed on the first run, built
with the build-arg fix.

---

## After the release: upgrading a real install

### Problem / motivation

The maintainer's own `~/.ekos` (installed by the wizard earlier the same day) was upgraded to
v1.1.0 while an Init & compile ran on their 95 000-object project.

### What went wrong, and what was built

| Found | Cause | Fix |
|---|---|---|
| `cp: cannot create regular file …/bin/ekos: Text file busy` — new console beside the old binary | a pipeline stage was running from `~/.ekos/bin/ekos`; a running program cannot be overwritten | `install.sh` copies beside it and renames over it (`place_binary`); test case 10 runs a copy of `sleep(1)` from the install directory and reinstalls |
| `install.ps1` would delete `bin\` and `console\` halfway on Windows | `Remove-Item -Recurse` on a locked file | moves both aside first; if either is locked, puts back what it moved and refuses (not run on Windows) |
| "commit failed (exit 124)" | every stage of a chained run had a single command's 30-minute limit | `run_stage_timeout_seconds` (default 6 h, Settings → Runs) for pipeline and Init & compile stages; RFC 0189 amended |
| `ekos console stop` during the run ended it | stop waits 20 s, then force-ends the console; the stage process carried on orphaned and the run was recorded `interrupted` | not fixed — TODO: stop should refuse or wait while runs are active |

### Decisions

- **Rename, not stop, in `install.sh`.** Stopping the console from the installer would end a
  pipeline run — the very thing that happened by hand. A rename leaves running processes on the
  old file and the next start on the new one.
- **A setting, not a bigger constant.** A project's commit time grows with its size and the
  machine; six hours is a default an owner can change on the Settings page.

---

## Knowledge Captured

- **Check for active runs before stopping someone's console**:
  `sqlite3 <home>/control/console.db "select id, command, status from run where status in
  ('queued','running')"` and `ps` for `<home>/bin/ekos` processes other than `console run` /
  `mcp serve`.
- **A killed commit leaves facts but no committed manifest.** `ekos status` then reports
  "no committed source manifest yet"; running the commit again completes it (the ledger is
  append-only and re-committing the same facts changes nothing).
- **`--version` cannot tell a release binary from a local build of the same version.** Compare
  bytes with the release asset (`cmp` against `tar -xzO` of the downloaded archive).
- **Run `scripts/audit.sh` right before tagging.** Four rustls advisories appeared between the
  local audit and the tag on the same day.
- **Python 3.12 vs 3.13 differ in `asyncio.Server.wait_closed`.** CI's 3.12 waits for every
  connection; reproduce CI-only hangs with `uv run --python 3.12 --isolated
  --with-requirements <(uv export --frozen) --with-editable . pytest`.

---

## Verification

- CI on 89faa70: Build & Test, Integration, Benchmark, Web console (Linux: 409 pytest, 109
  vitest), Web console API on macOS (409) and Windows (392 passed, 0 failed), Audit — all green.
- Release v1.1.0: 6 binaries, 4 full packages, publish, image, `Verify install.sh` — all green.
- `scripts/test-install.sh` passes, and its new case fails without the `install.sh` change.
- The maintainer's install: binary byte-identical to the release asset; console restarted with
  the stage-limit fix; no runs were active.
- The stage-limit test fails without the change. Not verified: a commit longer than 30 minutes
  through the console end to end (the maintainer's next Init & compile is that check).

---

## Files Changed

| File | Change summary |
|---|---|
| `web/api/tests/test_mcp_client_unit.py` + 8 test files | 3.12 hang; Windows-only assumptions skipped with reasons |
| `web/api/app/main.py`, `_proc.py`, `supervisor.py` | Windows path separators, CRLF, free MCP port |
| `ekos/crates/cli/src/home.rs`, `commands/{project,source,ledger,graph}.rs` | `canonical_dir` (no `\\?\`) |
| `.github/workflows/{ci,release}.yml`, `Dockerfile` | Node 22 |
| `ekos/Cargo.lock`, `tests/integration/Cargo.lock`, `benchmark/Cargo.lock` | rustls 0.23.46 |
| `install.sh`, `install.ps1`, `scripts/test-install.sh` | replace a running program |
| `web/api/app/{settings,console_file,runner}.py`, `tests/test_runner.py` | `run_stage_timeout_seconds` |
| `ekos/docs/rfcs/0189-init-and-compile-button.md` | amendment: a stage's time limit |
| CHANGELOG (Unreleased), `docs/deployment.md`, TODO, CLAUDE.md, capabilities page | docs |
