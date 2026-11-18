# Devlog 335 — RFC 0261 D0: the EKOS.DEV desktop shell on Linux

**Date:** 2026-11-18
**PRs:** none (local commit on `main`)
**Branch:** main

---

## Summary

RFC 0261 (EKOS Desktop: a Tauri shell around the console, and one refreshed console UI) is in the repo, and its
D0 spike has run on one of the three systems: Linux Mint 21.3 (Cinnamon, X11, WebKitGTK 2.50.4).

`desktop/` is a separate Cargo workspace holding a Tauri 2 app. It starts the console, or attaches to one
already running, through `ekos console …`, and signs in with a one-time code, so the person never sees a sign-in
screen. The tray, the quit dialog and the end-of-run notification were built ahead of D1 and all three work.

`ekos console open --json` prints `{url, code}` (§2.3).

Windows, macOS and an NVIDIA machine are still to be measured, and so are §6.1 (IPC scoped to the loopback
origin) and §6.2 (a hidden window's event stream).

---

## RFC 0261 D0 — spike

### Problem / motivation

RFC 0261's D0 asks for a window that starts or attaches to the console and signs in, plus measurements, before
D1 builds the real shell. The owner's brief was the design walkthrough "EKOS.DEV desktop app: UI design
walkthrough" (Claude Doc) and the RFC itself.

### What was built

| Component | What |
|---|---|
| `ekos/docs/rfcs/0261-ekos-desktop.md` | The owner's RFC, as written (proposed) |
| `console.rs::OpenTarget` / `open_target` | `ekos console open --json` → `{"url", "code"}`, code `null` in team mode; pure, tested |
| `desktop/src-tauri/src/ekos.rs` | Runs `ekos` (beside the app's executable, else `EKOS_BIN`, else `PATH`); `status`, `start`, `stop`, `restart`, `open --json`, `logs` |
| `desktop/src-tauri/src/api.rs` | The shell's own session (a second one-time code, its own cookie jar); `RunWatch`; notification and tray texts |
| `desktop/src-tauri/src/nav.rs` | Navigation policy: the start-up page and the console's origin; anything else goes to the system browser |
| `desktop/src-tauri/src/main.rs` | Window, start-up steps, tray, quit dialog, poller |
| `desktop/src-tauri/splash/index.html` | Start-up screen: the design's three steps, `console.log` tail, "Show log", "Open in browser instead", team-mode message |

### Measurements (Linux, Ryzen laptop, release build)

| What | Result |
|---|---|
| Console cold start (`ekos console start`, scratch home) | 3.3 s until `/api/health` answers |
| Window on screen (attaching to a running console) | 0.57 s |
| Sign-in | 303 to `/` with the `session` cookie (`HttpOnly`, `SameSite=Lax`, no `Secure`, so loopback http works); Projects page signed in as "this machine · write" |
| Binary | 22.4 MB release, 15.1 MB stripped |
| Memory | shell 137 MB RSS, WebKit processes 167 MB RSS (Projects page) |
| Close the window | hidden; the app, tray and console stay |
| Tray | an AppIndicator on Cinnamon: status line, Open, one entry per project, Open in browser, Restart, Quit |
| Run-end notification | "Init and compile finished" / "big · succeeded", sent through `org.freedesktop.Notifications` |
| Quit with a run in progress | the three-choice dialog appears; closing it keeps everything running (checked) |

### Implementation details worth remembering

- **Testing the tray without a mouse.** An AppIndicator tray exports its menu over D-Bus. Find the item with
  `org.kde.StatusNotifierWatcher.RegisteredStatusNotifierItems`, then use `com.canonical.dbusmenu.GetLayout` /
  `Event <id> clicked`. Menu ids change every time the menu is rebuilt, so read the layout again before
  clicking.
- **Notifications on a bus.** `dbus-monitor "interface='org.freedesktop.Notifications',member='Notify'"`
  shows each notification's title and body.
- **The start-up page has no race.** It calls `startup_ready` when it loads, and the steps start only then.
  The Rust side reports progress with `window.eval("ekos.step(n,'done')")`.

### Decisions

- **The shell watches runs itself.** It polls `/api/runs?limit=100` every 3 s with its own session instead of
  relying on the page. §6.2 makes this the fallback if a hidden window drops its `EventSource`. It works in
  local mode only.
- **`RunWatch` instead of an active-set diff.** The first run of the test took about 2 s. A run that starts and
  ends between two polls is never seen active, so "it left the active set" would miss it. Every run that has
  ended and was not seen before is announced; the first poll only records the history.
- **The quit dialog's third ("cancel") slot is "Keep running".** On Linux, closing the dialog reports that
  slot (`tauri-plugin-dialog` maps rfd's `Cancel` to the third custom label). The destructive choice must never
  sit there.

---

## Knowledge Captured

- **A parentless dialog opens behind other windows.** `tauri-plugin-dialog` on Linux, with the main window
  hidden: the quit dialog opened behind other apps. The fix shows the window and passes `.parent(&window)`.
  It is not yet verified on screen.
- **Do not drive the owner's desktop blind.** An XTEST click aimed at that hidden dialog hit another app's
  window instead. GUI checks belong on a separate X display (Xvfb) or in a VM, not on the owner's session.
  D1's end-to-end run should use Xvfb plus D-Bus for the tray.
- **The notification's app name is wrong.** It showed `ekos-desktop`, not "EKOS.DEV". The name comes from the
  identifier or `.desktop` entry, so D1's deb/rpm `.desktop` file fixes it.
- **`ekos` on `PATH` may be stale.** On this machine `~/.cargo/bin/ekos` is 0.1.0 and has no `console`
  subcommand. Looking beside the app's executable first (§2.3) avoids it, and `EKOS_BIN` covers a checkout.
- **The Tauri build needs development packages.** `libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`
  and `librsvg2-dev`; only the runtime libraries were installed. Tauri 2.12.6, plugin-dialog 2.8.4,
  plugin-notification 2.8.1. The first build compiled with no changes.
- **The console's UI loads unchanged in WebKitGTK.** The Projects page, sign-in and fonts all render. The
  graph page and the SSE run log were not opened in the window during this spike.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0261-ekos-desktop.md` | New: RFC 0261 (owner's text) |
| `ekos/crates/cli/src/commands/console.rs` | `OpenTarget`, `open_target`, `open(.., json)`; tests |
| `ekos/crates/cli/src/app.rs` | `console open --json` flag |
| `desktop/` | New workspace: Tauri 2 shell, start-up page, icon, README |
| `TODO.md` | D0 Linux ticked; D0 other systems and D1 items added |
