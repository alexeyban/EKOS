# Microsoft Store package (MSIX)

The `msix` job in `.github/workflows/release.yml` builds an unsigned `ekos-X.Y.Z.0-x64.msix`. It
holds the whole EKOS desktop install (RFC 0261 §10):
- the full Windows package: `ekos.exe`, the EKOS.DEV app, and the web console with its own Python;
- `bin\ekos-login.exe`, the package's startup task;
- `installer.toml` (`kind = "msix"`).

Partner Center signs the package during certification. It is uploaded as the workflow artifact
`store-msix-x64` and is **not** attached to the GitHub Release: an unsigned MSIX cannot be installed
outside the Store.

## What a person gets

- **Start menu.**
  - **EKOS.DEV** is the console in its own window.
  - **EKOS Setup** is the set-up wizard in the same window. The first start of EKOS.DEV offers it too.
- **The `ekos` command** in every terminal, through the package's execution alias. Agents are
  registered with the alias path, `%LOCALAPPDATA%\Microsoft\WindowsApps\ekos.exe`, which stays the same
  across updates.
- **Start at login.** If setup was asked for it, the startup task starts the console at login, with no
  window. It can be turned off in Task Manager → Startup apps, or with `ekos uninstall`.
- **No browser and no terminal window** anywhere in this flow.

## Identity

The manifest carries the identity from Partner Center → EKOS → Product identity. These values must stay
equal to Partner Center's, character for character:

| Field | Value |
|---|---|
| Identity name | `AlexeyBanaev.EKOS.DEV` |
| Publisher | `CN=77359C36-EF40-49A6-8F61-6727869C79BA` |
| Publisher display name | `Alexey Banaev` |
| Display name | `EKOS.DEV` (the reserved app name) |
| Package family name | `AlexeyBanaev.EKOS.DEV_hbn1emg27zd0m` |
| Store ID | `9NX411QFSW9C` |

The listing is <https://apps.microsoft.com/detail/9NX411QFSW9C>.

If a value is ever reset to a `REPLACE_WITH…` placeholder, a manual run of the Release workflow fails
with that message. A tag run only warns, and builds no MSIX.

## Each submission

1. **Build.** Actions → Release → Run workflow, or push a `v*` tag.
   - The Store version is the tag's `X.Y.Z` plus `.0`. A manual run uses `ekos/Cargo.toml`'s version.
   - It must grow with every submission.
2. **Upload.** Download `store-msix-x64` and upload the `.msix` under Packages.
3. **Listing logo:** `ekos-store-300x300.png` in this directory.
4. **Restricted capabilities.** Partner Center asks why each one is needed:
   - `runFullTrust`: EKOS.DEV is a desktop app (Tauri) that runs the `ekos` command and a local web
     console (Python) on 127.0.0.1.
   - `unvirtualizedResources`: the setup wizard adds EKOS as an MCP server to the configuration files
     of Claude Desktop (`%APPDATA%\Claude\claude_desktop_config.json`) and VS Code
     (`%APPDATA%\Code\User\mcp.json`), but only when the person ticks those boxes. Those programs must
     read the real files, not a copy private to this package.
5. **Notes for certification:**

   > Start **EKOS.DEV**. On first start it shows "Run EKOS Setup first". **Open EKOS Setup** walks
   > through setup in the same window: local mode, a first project from any folder, and the console
   > started now. Then the window shows the console, signed in. The `ekos` command works in any
   > terminal (`ekos --help`). The startup task "EKOS console" does nothing unless setup was asked to
   > start the console at login.

## Requirements and limits

- **Windows 10 2004 (10.0.19041) or later**, x64.
- **WebView2**, which is part of Windows 11 and of current Windows 10.
- **About 100 MB**, mostly the console's Python.
- **The program folder is read-only.** Everything EKOS writes goes to the EKOS home
  (`%USERPROFILE%\.ekos`). Removing the app leaves the home, as the MSI does.
- **The MSI and the Store package can both be installed.** Each has its own `ekos`, and the one earlier
  on `PATH` wins. Use one or the other.

`makeappx` runs in CI only; there is no Windows machine here. The `msix` job installs the package and
tests it as described in RFC 0261 §10.
