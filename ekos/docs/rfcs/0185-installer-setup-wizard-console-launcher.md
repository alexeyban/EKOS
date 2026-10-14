# RFC 0185 — Installer: a setup wizard in the browser, the console launcher, full packages

**Status:** Accepted — implemented 2026-10-14 (devlog_269)
**Author:** EKOS team
**Created:** 2026-10-14
**Builds on:** RFC 0153 (installer, release matrix), RFC 0178 (the home), RFC 0179 (image), RFC 0184 (console settings, local mode, keychain)
**Part of:** installer and web configuration (0184, this RFC, 0186, 0187)

## Motivation

The web console runs from a checkout (Python, Node) or a Docker image. Neither suits a person
on Windows 11 or macOS who wants EKOS on their own machine, and neither asks where to put it,
which port to use, or how agents should connect. The bare-binary installer (`install.sh`) has
no console at all and no Windows path.

## 1. A full package

One archive per target, in addition to the bare binary a release already has:

```
ekos-<version>-<target>/
  bin/ekos(.exe)
  console/python/          python-build-standalone CPython 3.12 (install_only build)
  console/site-packages/   the console's dependencies, wheels for that platform only
  console/app/             web/api/app
  console/ui/              the built web/ui
  setup.sh | setup.command | setup.cmd     starts `bin/ekos setup`
  INSTALL.md
```

`scripts/package-release.sh --full` builds it. The Python build is pinned by version and
SHA-256 in `deploy/package/python.lock` and verified before unpacking. Dependencies come from
`uv export --frozen --no-dev` and are installed with `uv pip install --target … --python-platform
<platform> --only-binary :all:`, so the three systems' packages can be assembled on Linux from
the binaries the release matrix builds. Hashes are required.

`release.yml` gets a `package` job per target that assembles the full package from that
target's binary and attaches it. `install.sh` takes the full package by default (`--cli-only`
for the bare binary, as before); a new `install.ps1` does the same on Windows without
administrator rights.

## 2. `ekos console`

```
ekos console run       the console in the foreground, started again when it asks to be
ekos console start     `run` in the background; waits until it answers
ekos console stop | restart | status | open | logs
```

- **Finding the runtime.** Relative to the running binary: `<bin>/../console/`. For a
  checkout, `EKOS_CONSOLE_PYTHON` and `EKOS_CONSOLE_APP_DIR` point at a virtual environment's
  interpreter and `web/api`. Neither found: an error that names both and the container.
- **What it starts.** `<python> -m uvicorn app.main:create_app --factory --host <h> --port <p>`
  with `EKOS_HOME`, `EKOS_BIN` (this binary), `EKOS_CONSOLE_UI_DIR`, `EKOS_CONSOLE_SUPERVISED=true`,
  `PYTHONPATH` (the app and its packages) and `PYTHONNOUSERSITE=1`, so a user's own Python
  packages cannot shadow the console's. Host and port come from `console.toml`, overridden by
  `EKOS_CONSOLE_HOST`/`EKOS_CONSOLE_PORT`. In local mode a non-loopback host is refused.
- **Stopping and restarting** go through files in `<home>/control/`, the same on every OS, so
  no signal has to cross from one process to another: `stop-requested` and `restart-requested`.
  The console (when supervised) checks for them every second and shuts down cleanly when one
  appears (RFC 0184's restart button writes the second). `run` then exits, or starts it again.
  `stop` writes the file, waits up to 20 s, and only then ends the processes by force.
  A console that exits on its own is started again up to 5 times in 10 minutes.
- **State.** `control/console.pid` (JSON: supervisor pid, console pid, host, port, start time)
  and `control/console.log`.
- **`open`.** Local mode: writes a one-time code (RFC 0184 §3) and opens
  `http://127.0.0.1:<port>/api/auth/local?code=…`; it also prints the code. Team mode: opens
  the console's address.

## 3. `ekos setup`

A wizard in the browser, served by the binary itself: axum on `127.0.0.1:<random port>`, a
random token in the URL and required on every request, one embedded HTML page, the browser
opened for the person. It stops when the person finishes or after an hour idle.

Steps:

1. **Location** — program directory and EKOS home (both default to `~/.ekos`, on Windows
   `%USERPROFILE%\.ekos`), each checked for write access and free space; put `bin` on `PATH`
   (a marked line in `~/.profile` and the shell's rc file; the user `Path` on Windows).
2. **Console** — mode (local / team), port; for team mode the OIDC issuer (checked by fetching
   its discovery document), client id, client secret, redirect URI, owners, production.
3. **MCP** — project servers' idle minutes and maximum; register EKOS with the agents found on
   the machine for the first project: Claude Code (`claude mcp add -s user`), Claude Desktop
   (`claude_desktop_config.json`, `mcpServers`), VS Code (user `mcp.json`, `servers`). Each is
   shown as the exact entry to add and applied only when ticked; other entries are kept; the
   file is backed up first; a file that is not plain JSON is not touched and the entry is shown
   to copy instead.
4. **First project** (optional) — a name and source directories (`ekos project add --detect`).
5. **Start** — start at login (systemd user unit, launchd agent, Windows logon task; none needs
   administrator rights), start now, open the console.

Applying copies the package into the program directory when it runs from somewhere else,
writes `console.toml` with `toml_edit` (comments kept), stores secrets in the keychain through
the bundled Python (`python -m app.keychain_cli put …`, value on standard input, so the same
code as the console does it), then performs the ticked steps and reports each one.

Running `ekos setup` on an installed system shows what is set and changes only what the person
changes. `ekos setup --answers answers.toml` applies the same answers without a browser (a
server, a script, the tests); `--dry-run` prints what it would do.

The chat model step is added by RFC 0187, which defines its settings.

## 4. `ekos uninstall`

Stops the console, removes the login entry and the `PATH` lines it added, and removes `bin/`
and `console/` from the program directory. The home (projects, the console's database) stays
unless `--purge --yes`.

## Tests (written first)

- Package: layout check (`console/python` runs, imports the app and every dependency).
- `console`: runtime discovery order; the command line and environment it builds; a
  non-loopback host refused in local mode; start → status → restart → stop against the real
  console in a temp home; the restart marker restarts, the stop marker stops, a crash restarts
  and five crashes stop it.
- `setup --answers` in temp directories: `console.toml` contents and comments kept; the `PATH`
  line added once and removed by `uninstall`; the service file contents for each OS (written,
  not activated, in tests); agent config merges keep other entries and refuse a non-JSON file;
  a wrong token is refused by the wizard server; the wizard page is served.
- The browser wizard end to end with Playwright on Linux.

## Not built

Code signing and notarisation (Windows SmartScreen and macOS Gatekeeper warn the first time; the
instructions say how to continue), automatic updates, an uninstaller entry in the Windows
"Apps" list.
