# Devlog 269 — Installer: setup wizard, console launcher, full packages (RFC 0185)

**Date:** 2026-10-14
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

Second of four phases (RFC 0184–0187). EKOS now installs with its web console on a machine
that has nothing else: a full package carries the binary, a pinned standalone Python with the
console's dependencies, the console and its built UI. `ekos setup` opens a wizard in the
browser (or applies an answers file), `ekos console` runs and supervises the console, and
`ekos uninstall` undoes what setup recorded. On Linux the whole path is checked end to end by
`deploy/e2e/package.sh` (29 checks). Windows and macOS packages are built by a new release job
and have not run on those systems yet.

---

## RFC 0185

### What was built

| Piece | What it does |
|---|---|
| `commands/console.rs` | `ekos console run` (supervisor), `start` (detached, waits for `/api/health`), `stop`, `restart`, `status`, `open` (local one-time code), `logs`; runtime found beside the binary or via `EKOS_CONSOLE_PYTHON`/`EKOS_CONSOLE_APP_DIR` |
| `web/api/app/lifecycle.py` | Under a supervisor, the console watches `control/stop-requested` / `restart-requested` and shuts down cleanly |
| `commands/setup.rs` | `Answers` → `plan` (pure list of `Action`s) → `apply` through `Effects`; PATH lines, agent JSON merges with backups, systemd / launchd / schtasks entries, `control/install.json`; `uninstall` |
| `commands/setup_server.rs`, `setup.html` | The wizard: axum on loopback, token on every request, state / path check / issuer check / plan / apply / finish |
| `web/api/app/keychain_cli.py` | Setup stores secrets through the bundled Python, value on stdin |
| `scripts/package-full.sh`, `deploy/package/python.lock`, `INSTALL-full.md` | One target's full package; Python verified by pinned SHA-256; wheels for the target platform via `uv --python-platform`; starters |
| `install.sh`, `install.ps1` | Full package when the release lists one, then the wizard (`EKOS_NO_SETUP`, `EKOS_CLI_ONLY`); Windows installer |
| `release.yml` | `package` job (5 targets, assembled on Linux); `verify` starts the installed console |
| `deploy/e2e/package.sh` (+ two Python stages) | Assemble → setup from answers → console (sign-in, setting + restart, pipeline, search) → wizard in Chromium → uninstall |

### Implementation details worth remembering

- **No signal crosses processes.** Stop and restart are files the console polls once a second
  when `EKOS_CONSOLE_SUPERVISED`; it then raises `SIGTERM` in itself so uvicorn shuts down
  through the lifespan (MCP servers stopped). The supervisor treats exit 0 without a file as a
  deliberate stop (Ctrl-C reaches both), non-zero as a crash (5 in 10 minutes and it gives up).
  Same code on Windows, where there is no SIGTERM to send to another process.
- **`setup` plans before it acts.** The wizard's Review page is `plan`'s output; `apply` stops at
  the first failed step and records what it did after every step.
- **Every outside effect goes through `Effects`** (systemctl, launchctl, schtasks, `claude`,
  powershell, the browser), so unit tests and the package check never touch the machine's
  services; the package check also points `HOME` at a scratch directory.
- `claude mcp add` refuses an existing name: setup removes it first, so a second run works.

### Defects found by the package check, fixed

| Defect | Fix |
|---|---|
| The Settings page's **Restart now** signed the person out: with no session secret, each start chose a random one | A console with a home keeps a generated key in `control/session.key` (0600); production mode accepts it |
| The check's own source-tree comparison assumed `ls` order | `LC_ALL=C` |

### Decisions

- **Program and data share the home by default** (`~/.ekos/bin`, `~/.ekos/console`, `data/`,
  `control/`): one directory to find, back up or remove; the wizard can split them.
- **Agents get a stdio server per project** (`ekos --home H --project P mcp serve`), not the
  console's gateway: it needs no token and no running console. The gateway stays for team use.
- **The chat model step is left to RFC 0187**, which defines its settings; a wizard writing keys
  nothing reads would be misleading.

---

## Knowledge Captured

- **Rust 2024's `impl Trait` capture rules** made a
  test helper returning `impl Fn` borrow its argument slice; `+ use<>` says it captures nothing.
- **`cargo fmt --all` formats every file in the workspace**, including someone's uncommitted
  work. Format your own files with `rustfmt --edition 2024 <file>`. (Here the four files were
  already formatted; their diff stats were unchanged.)
- python-build-standalone `install_only` archives unpack to `python/`; on Unix the interpreter
  is `python/bin/python3` (a symlink — copy with links kept), on Windows `python/python.exe`.
- `uv pip install --target … --python-platform <triple> --only-binary :all: --require-hashes`
  assembles another OS's dependencies on Linux; the console's tree (incl. `keyring`, `uvicorn`)
  has wheels for all five targets.
- A Starlette session signed with a per-process random secret is invalid after any restart;
  any feature that restarts the server must keep the key.

---

## Verification

`deploy/e2e/package.sh`: 29 passed (Linux x86_64, release binary from the working tree,
console from the working tree). Rust: fmt, clippy clean; workspace tests and the console suite
(see the commit). `scripts/test-install.sh` passes with the new full-package cases. Not run:
Windows, macOS, `install.ps1` (no pwsh here), real login-service activation.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0185-installer-setup-wizard-console-launcher.md` | New RFC |
| `ekos/crates/cli/src/commands/{console,setup,setup_server,setup_tests}.rs`, `setup.html` | New |
| `ekos/crates/cli/src/{app.rs,commands/mod.rs}`, `Cargo.toml`, `ekos/Cargo.lock`, `tests/console_live.rs` | Wiring, deps, live test |
| `web/api/app/{lifecycle,keychain_cli}.py`, `main.py`, `routes/platform.py`, `settings.py` | Watcher, keychain command, kept session key |
| `web/api/tests/{test_supervised,test_console_settings}.py` | Tests |
| `scripts/{package-full,package-release,test-install}.sh`, `install.sh`, `install.ps1` | Packages and installers |
| `deploy/package/{python.lock,INSTALL-full.md}`, `deploy/e2e/package*.{sh,py}` | Pin, guide, package check |
| `.github/workflows/release.yml` | `package` job, console check in `verify` |
| `README.md`, `docs/deployment.md`, capabilities page, `CHANGELOG.md`, `TODO.md`, `CLAUDE.md` | Docs |
