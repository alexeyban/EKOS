# Devlog 342 — EKOS.DEV in the Microsoft Store package (RFC 0261 §10, D5)

**Date:** 2026-11-20
**PRs:** none (pushed to `main`)
**Branch:** main

---

## Summary

The owner asked for an `.msix` to submit to the Microsoft Store. The package the release built held
only `ekos.exe`. Started from the Start menu, it flashed a terminal and closed, which breaks the
owner's rule of no browser and no terminal window. The owner chose the full app.

The Store package is now the full Windows package, the one the MSI installs, plus a startup task. That
pulls RFC 0261's D5 forward, and its design is RFC 0261 §10. The `msix` job installs the package on a
Windows runner and tests it the way a person would use it.

---

## The full app as an MSIX

### Problem / motivation
A Store app is not an MSI with a different wrapper:
- it runs no installer scripts;
- its folder (`WindowsApps\<name>_<version>_x64__<id>`) changes with every update;
- writes to `%APPDATA%` and `HKCU` go to a private copy, by default;
- a Start menu entry takes no arguments unless the manifest declares them.

So three things the MSI does would have broken:
- the login task named a path the next update removes;
- agents would have been registered with that same path;
- Claude Desktop and VS Code would never have seen the configuration setup wrote for them.

### What was built
| Component | Change |
|---|---|
| `packaging/msix/AppxManifest.xml` | Two applications on `bin\ekos-desktop.exe`: **EKOS.DEV**, and **EKOS Setup** (`uap10:Parameters="--setup"`). The `ekos.exe` alias. A `windows.startupTask` on `bin\ekos-login.exe`. Write virtualization off (`desktop6:*WriteVirtualization`, rescap `unvirtualizedResources`). Minimum Windows is 10.0.19041 |
| `desktop/src-tauri/src/bin/ekos-login.rs` | The startup task, 384 KB, standard library only. It reads `%LOCALAPPDATA%\EKOS.DEV\start-at-login`, which holds the home. Without that file it does nothing. Otherwise it runs `ekos.exe --home <home> console start` with `CREATE_NO_WINDOW` |
| `setup.rs` | `installer.toml` `kind = "msix"`. `Machine::{local_appdata, is_msix, msix_alias, msix_login_file}`. `Layout::launch` / `launch_exe`: agents get the alias. For "start at login", `service_file` returns the login file, `install_service` writes it, the manifest records `service = "msix"`, and uninstall removes the file, never calling `schtasks` |
| `setup_tests.rs` | The Store machine: no scheduled task, the login file names the home, Claude Desktop starts the alias, and uninstall removes the file. The MSI keeps its task |
| `release.yml` `msix` | Needs `package` and `desktop`. It packs the full package, adds `ekos-login.exe` (now in the desktop artifact) and `installer.toml`, and drops `setup.cmd`. The test installs the package and goes through the pipeline, MCP, ASCII `doctor` output, setup from answers (project, Claude Desktop, start at login), the console, `ekos-login.exe` restarting it, `ekos uninstall` and package removal |
| `packaging/msix/README.md`, RFC 0261 §10 | What a person gets, the capability justifications and the certification notes for Partner Center |

### Decisions
- **A separate small program for the startup task.**
  - Considered: `ekos-desktop.exe --start-console`, as the MSI's task runs it. A startup task cannot be
    given arguments.
  - Considered: detecting a startup-task activation in the app. That needs WinRT activation APIs that
    nothing here could test.
  - Chosen: a program whose name says what it does. It is small enough not to matter in a 100 MB
    package.
- **The startup task is enabled, and a file decides.** Whether the console starts at login stays the
  answer in setup, as on every other installer. Windows' own switch is in Task Manager. No WinRT
  `StartupTask.RequestEnableAsync` is needed.
- **Virtualization off, rather than writing agent configuration some other way.** The files are the
  agents' own. A private copy would make "add to Claude Desktop" silently do nothing.
- **The full package unchanged.** `ekos-login.exe` is added only by the `msix` job. The MSI, the zip
  and the other installers do not carry it.

---

## Knowledge Captured
- **A Store app's folder is versioned.** Anything outside the package that has to reach the program
  later needs the execution alias (`%LOCALAPPDATA%\Microsoft\WindowsApps\<alias>.exe`) or a
  package-declared extension, such as a startup task or a protocol. It must never use
  `InstallLocation`.
- **`%APPDATA%` virtualization for a packaged desktop app** is invisible to the app, which reads back
  its own writes, and fatal for any other program that should read them. Turning it off needs
  `desktop6:FileSystemWriteVirtualization` plus the restricted capability `unvirtualizedResources`,
  and Windows 10 2004 or later.
- **An `Application` per Start menu entry.** Arguments come only from `uap10:Parameters`.
- **A console-subsystem execution alias** (`desktop4:Subsystem="console"`) is refused by `makeappx` (error 80080204) unless its `Application` declares `desktop4:SupportsMultipleInstances="true"`. So the alias has its own hidden application (`AppListEntry="none"`) on `ekos.exe`.
- **A file in `WindowsApps` cannot be started by its path**, even by the user who installed the package: "Access is denied". Only an activation (Start menu, alias, startup task) or `Invoke-CommandInDesktopPackage` runs it. A test or a script that needs a package's program must go through one of these.
- **Not verified:** the Windows App Certification Kit (D5's exit), the startup task at a real login
  (CI runs `ekos-login.exe` by its path) and certification.

## Files Changed
| File | Change summary |
|---|---|
| `packaging/msix/AppxManifest.xml`, `packaging/msix/README.md` | the full app's manifest; what it holds and how to submit it |
| `desktop/src-tauri/src/bin/ekos-login.rs`, `desktop/src-tauri/Cargo.toml` | the startup task; `default-run` |
| `ekos/crates/cli/src/commands/{setup,setup_tests,setup_server,uninstall_server}.rs` | the `msix` installer kind |
| `.github/workflows/release.yml` | the `msix` job builds and tests the full app; `ekos-login.exe` in the desktop artifact |
| `ekos/docs/rfcs/0261-ekos-desktop.md` | §10 |
| `CHANGELOG.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | docs |
