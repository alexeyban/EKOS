# Devlog 336 — EKOS.DEV in the installers; v1.6.0

**Date:** 2026-11-18
**PRs:** none (pushed to `main`, tag `v1.6.0`)
**Branch:** main

---

## Summary

RFC 0261 is accepted, and the EKOS.DEV desktop app from devlog_335 now ships inside every installer
the release already builds: the Windows .msi, the macOS .pkg, and the Linux .deb and .rpm. As §2.8
of the RFC decided, the release workflow builds `ekos-desktop` for the four targets that have a full
package. `scripts/package-full.sh` places it beside `ekos` in `bin/`, and each installer script adds
a launcher. Nothing is a separate download. The app now finds `ekos` where the installers put it,
and on Windows it no longer flashes a terminal for each `ekos console …` it runs. The workspace
version is 1.6.0. The release also carries the DuckDB work (RFC 0259) and RFC 0260's checks.

---

## EKOS.DEV in the release

### Problem / motivation
The D0 spike was built and measured on Linux only, with `cargo run`. Nobody could install it.

### What was built
| Component | Change |
|---|---|
| `release.yml` `desktop` job | `cargo test` and `cargo build --release` of `desktop/` on ubuntu-22.04 (x86_64 and arm64), macos-15 and windows-latest, with WebKitGTK 4.1 dev packages on Linux and a static CRT on Windows. Artifact `desktop-<target>`; it is not named `ekos-*`, so it is never a release asset |
| `package` job | needs `desktop` and passes `EKOS_PACKAGE_DESKTOP` to `package-full.sh` |
| `scripts/package-full.sh` | optional `EKOS_PACKAGE_DESKTOP` → `bin/ekos-desktop[.exe]` (an extension's package is unchanged) |
| `scripts/package-linux.sh` | `ekos-desktop.desktop` "EKOS.DEV". deb `Recommends: libwebkit2gtk-4.1-0, libayatana-appindicator3-1`; rpm `Recommends:` by soname |
| `scripts/package-macos.sh` | `/Applications/EKOS.DEV.app`: the binary moves into `Contents/MacOS`, `Contents/MacOS/ekos` links to `/usr/local/ekos/bin/ekos`; `uninstall.sh` removes the app |
| `packaging/wix/ekos.wxs`, `package-msi.ps1` | `-d Desktop=yes` when the full package has the app: `bin\ekos-desktop.exe` plus a Start menu "EKOS.DEV" |
| Installer tests | full Windows package: the app is beside ekos.exe. MSI: the exe and its shortcut. pkg: the bundle, its `ekos` link, system libraries only (`otool -L`), removed by uninstall.sh. deb/rpm: the binary and its menu entry; after installing the recommended libraries, `ldd` finds every library (deb: required; rpm: a warning) |
| `desktop/src-tauri/src/ekos.rs` | `places_for`: `/opt/ekos/bin`, `/usr/local/ekos/bin`, `%LOCALAPPDATA%\Programs\EKOS\bin`, then `~/.ekos/bin`, after "beside the exe" and `EKOS_BIN`. `CREATE_NO_WINDOW` on Windows |
| `desktop/` | version 1.6.0, `icons/icon.ico` (tauri-build needs it for the Windows resource), release profile `strip` + thin LTO |

### Decisions
- **Recommends, not Depends, for WebKitGTK.** A server installing the .deb with
  `--no-install-recommends` (the test containers do) gets the command without a desktop stack. A
  person on a desktop gets the libraries by default.
- **The rpm's weak dependencies are sonames** (`libwebkit2gtk-4.1.so.0()(64bit)`), not package
  names, which differ between Fedora and RHEL. RHEL 9 / Rocky 9 may package only WebKitGTK 4.0, so
  the rpm test only warns when the library is missing there.
- **macOS: a link inside the bundle, not a `PATH` lookup.** An app started from the Finder gets
  `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, so `ekos` in `/usr/local/bin` is not found. The bundle's
  `Contents/MacOS/ekos` link makes "beside the executable" true, and `places_for` is the fallback.

---

## Knowledge Captured
- tauri-build on Windows fails without `icons/icon.ico` (it generates the exe's resource from it),
  even with the bundler off. Linux and macOS builds do not need it.
- A `windows_subsystem = "windows"` app that runs a console program shows a terminal window for each
  call unless the child gets `CREATE_NO_WINDOW` (0x08000000).
- The Tauri tray on Linux loads libayatana-appindicator with `dlopen`. `ldd` does not list it, and a
  machine without it fails when the tray is created, not at start.
- **Not verified yet:** the app starting on Windows and macOS. CI builds and unit-tests it there, and
  the installer tests check placement and linking, but no window has been opened on either system
  (RFC 0261 D0 rows for Windows and macOS stay open).

## Files Changed
| File | Change summary |
|---|---|
| `.github/workflows/release.yml` | `desktop` job; package needs it; installer tests check the app |
| `scripts/package-full.sh`, `package-linux.sh`, `package-macos.sh`, `package-msi.ps1`, `packaging/wix/ekos.wxs` | the app in each package |
| `desktop/` | install-location lookup, Windows no-window, icon.ico, version 1.6.0, release profile |
| `ekos/Cargo.toml`, the three lockfiles | 1.6.0 |
| `ekos/docs/rfcs/0261-ekos-desktop.md` | accepted |
| `CHANGELOG.md`, `README.md`, `TODO.md` | v1.6.0, EKOS.DEV in the installers |
