# EKOS.DEV desktop app (RFC 0261, D0 spike)

A native window around the EKOS console. It adds the desktop pieces of the design: a start-up screen, a
tray menu, a quit dialog that knows about runs in progress, and an end-of-run notification. The UI in the
window is the console's own, loaded from `http://127.0.0.1:<port>`. The console's lifecycle goes through
`ekos console …` only.

This is a separate Cargo workspace: Tauri needs system development packages that `ekos/` and its CI must
not depend on.

## Build and run (Linux)

```bash
sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev
cd desktop && cargo run -p ekos-desktop
```

The shell finds `ekos` beside its own executable, else at `EKOS_BIN`, else in the `PATH`. To run it against a checkout's console:

```bash
export EKOS_BIN=$PWD/../ekos/target/debug/ekos
export EKOS_CONSOLE_PYTHON=$PWD/../web/api/.venv/bin/python EKOS_CONSOLE_APP_DIR=$PWD/../web/api
export EKOS_CONSOLE_MODE=local
cargo run -p ekos-desktop
```

## What it does

- **Start-up:** `ekos console status`, then `ekos console start` if the console is down, then
  `ekos console open --json` (`{url, code}`). The window then goes to `/api/auth/local?code=…`. In team mode the window shows "open it in the browser" instead (RFC 0261 §2.3).
- **Its own session:** the shell signs in with a second code and polls queued and running runs every 3 s
  for the tray and the notifications. It works in local mode only.
- **Closing the window** hides it to the tray. **Quit** asks first when a run is in progress, and stops the
  console only if the shell started it.
- **Navigation:** the window loads only the console's origin. Any other link opens in the system browser.
