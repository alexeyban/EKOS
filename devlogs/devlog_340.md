# Devlog 340 — EKOS Setup as an application; no browser, no terminal (RFC 0261 §8, §9)

**Date:** 2026-11-19
**PRs:** none (pushed to `main`)
**Branch:** main

---

## Summary

The owner asked for EKOS Setup as an application rather than a page in the browser. RFC 0261 §2.6
had already planned it as the D3 **Set up** row, and this change builds that row now (RFC 0261 §8):
- **The app runs the wizard.** EKOS.DEV starts `ekos setup --no-browser` and shows the wizard in its
  own window. When setup ends, it goes on to the console, signed in.
- **The shortcuts start the app.** Every installer's "EKOS Setup" entry now runs
  `ekos-desktop --setup`.
- **The browser wizard stays as the fallback.** It is used where the app cannot start, or where the
  package has no app.

v1.6.1 was published earlier in the session, before this change (devlog_339). This ships in the next
release.

---

## Set up in the app

### What was built
| Component | Change |
|---|---|
| `desktop/src-tauri/src/ekos.rs` | `start_setup`: `ekos setup --no-browser`, reads the line `EKOS setup: <url>` (`setup_address`: loopback HTTP only, `&app=1` added), then keeps draining the output on a thread so the wizard's later lines never meet a closed pipe |
| `desktop/src-tauri/src/main.rs` | `--setup` on the command line, or through the single-instance hand-over, or "Open EKOS Setup" on the start-up screen → `run_setup`: the window loads the wizard, titled "EKOS Setup", and waits for `ekos setup` to exit. Then the start-up page loads again, and its steps start the console and sign in. A second request while setup runs only shows the window |
| `desktop/src-tauri/src/nav.rs` | `allowed(url, console, setup)`: the wizard's origin may load only while setup runs |
| `commands/setup.html` | `app=1`: "Open it in the browser" is hidden and off; Done does not close the window; the last page says the app opens the console next |
| `scripts/package-linux.sh` | with the app: `Exec=sh -c "/opt/ekos/bin/ekos-desktop --setup \|\| exec /usr/bin/ekos setup"`, `Terminal=false` |
| `scripts/package-macos.sh` | with the app: "EKOS Setup.app" runs `open -n -a /Applications/EKOS.DEV.app --args --setup`. The `.pkg`'s postinstall opens that app, so a first install lands in the app too |
| `packaging/wix/ekos.wxs` | with the app: the Start menu "EKOS Setup" and the last page's "Open EKOS Setup now" run `ekos-desktop.exe --setup` |
| `release.yml` | the installer tests expect the new targets on Windows, macOS and Linux |

### Decisions
- **The wizard is not rebuilt.** It stays the page `ekos setup` serves, shown in the app's webview.
  One wizard, one set of tests (`setup.rs`'s `plan` and `apply`), and the browser fallback is the
  same page.
- **The Linux fallback is `||`.** A server installed with `--no-install-recommends` has no
  WebKitGTK, so `ekos-desktop` fails at load and the entry runs `ekos setup` instead.
- **`open -n` on macOS.** Without it, `open` hands nothing to an app that already runs. With it, a
  new process starts, and the single-instance plugin passes `--setup` to the running app.

---

## Knowledge Captured
- **Seen on a virtual screen** (`xvfb-run dbus-run-session`, a scratch `HOME`):
  - `ekos-desktop --setup` opened on the wizard, in a window titled "EKOS Setup";
  - ending the wizard's process, standing in for Done, brought back the start-up page, retitled
    "EKOS.DEV";
  - the page then showed "Run EKOS Setup first" again, correctly, since that home was still not set up.
- **Not seen:** a whole setup through to the console in the app window. The wizard cannot be driven
  from outside the webview. The step after it is the start-up path devlog_335 measured.
- **In the built `.deb`,** `ekos-setup.desktop` carries the new `Exec` and `Terminal=false`.

## No browser and no terminal on a desktop install (RFC 0261 §9)

The owner's rule, given while the setup work was being checked: every local application installs and
works as an application, with no browser and no terminal window. Going through every entry point found
four that still opened one:

| Where | Before | Now |
|---|---|---|
| "EKOS Console" (Start menu, Applications, Linux menu) | `ekos console open --start`: the browser, and on Windows a terminal | not installed beside EKOS.DEV, which is the console (RFC 0261 decision 4). Packages without the app keep it |
| Settings → Uninstall EKOS in the app | the page left the window for the browser | `nav::is_local_token_page`: a loopback HTTP page with a ≥32-character `t` token loads in the window |
| Windows login start | `schtasks` ran `ekos.exe … console start`; a console program shows a terminal while it waits | `setup::windows_launcher`: with `ekos-desktop.exe` beside `ekos.exe`, the task runs `ekos-desktop.exe --start-console --home …`. That windows-subsystem program runs `console start` with `CREATE_NO_WINDOW`, before Tauri starts (no window, no single-instance hand-over), and exits |
| MSI uninstall | `StopConsole` ran `ekos.exe` and flashed a terminal | `WixQuietExec64` (`Wix4UtilCA_X64`) with `WixQuietExec64CmdLine` set just before it |

Kept on purpose: "Open in browser" in the tray and on the start-up screen, which are a person's choice,
and outside links.

- **Not verified on Windows.** The quiet-exec stop and the windowless login start were not seen on a
  Windows desktop. CI builds the MSI and the release workflow installs and uninstalls it there. Whether a
  window shows at login has to be seen by a person (TODO).

## Files Changed
| File | Change summary |
|---|---|
| `desktop/src-tauri/src/{ekos,main,nav}.rs`, `splash/index.html` | Set up in the window, tests |
| `ekos/crates/cli/src/commands/setup.html` | the in-app mode |
| `scripts/package-linux.sh`, `scripts/package-macos.sh`, `packaging/wix/ekos.wxs` | EKOS Setup starts the app |
| `.github/workflows/release.yml` | installer tests (§8, §9) |
| `ekos/crates/cli/src/commands/setup.rs`, `setup_tests.rs` | the Windows login task through EKOS.DEV |
| `ekos/docs/rfcs/0261-ekos-desktop.md` | §8 |
| `CHANGELOG.md`, `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | docs |
