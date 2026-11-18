# RFC 0261 — EKOS Desktop: a native app shell for the console, and one refreshed console UI

**Status:** proposed (2026-11-18) · **Target:** not set
**Depends on:** RFC 0127 (web console), RFC 0180 (roles), RFC 0184 (local mode, keychain), RFC 0185 (full
package, `ekos console`, `ekos setup`), RFC 0186 (command catalogue), RFC 0188 (uninstall page), RFC 0189
(Init & compile), RFC 0198 (extension pages)
**Decisions taken by the owner (2026-11-18):** a Tauri shell around the existing console, not a second UI ·
local projects first, remote servers later · the visual refresh is done once in `web/ui`, for the browser
and the app alike
**Second round (2026-11-18):** Windows is signed through the Microsoft Store only, and there is no Apple
Developer ID for now · the app is named **EKOS.DEV** wherever its name shows · closing the window keeps it
in the tray, and Quit is in the tray menu · the browser shortcut goes when the app ships
**Names:** "EKOS Desktop" is this RFC's working title and `ekos-desktop` the binary; people see "EKOS.DEV".
**Written against:** `main` at `973e0f7` (2026-11-15, v1.5.5). The number assumes nothing has taken 0261
since.

## 1. Problem

EKOS on a person's own machine is a command, a console in a browser tab, and two Start-menu entries that
open a browser. Seven things follow from that:

| # | What is missing today | Where it shows |
|---|---|---|
| P1 | The console has no window of its own. `ekos console open` (the "EKOS Console" shortcut, `console open --start`) hands a one-time-code URL to the default browser | A tab at `127.0.0.1` among other tabs; no taskbar or dock entry; a browser profile without the cookie shows the "paste a one-time code" screen (`Layout.tsx`, local mode) |
| P2 | Nothing owns the console's life. It runs from a login service or by hand, and `ekos console stop` waits 20 s and then ends it by force while a pipeline run is in progress | TODO: "`ekos console stop` ends a pipeline run in progress … the run's `ekos` stage carries on orphaned" |
| P3 | 28 commands are class `local` (`catalogue.rs::CLASSES`) and are never offered from the web: `setup`, `uninstall`, `console *`, `home backup/restore`, `project *`, `init`, … Some have pages of their own (`setup`, `uninstall --ui`), started from a terminal or a shortcut | "Every function from the interface" is true for the 150 other commands (RFC 0186) and not for these |
| P4 | No use of the operating system: a source directory is typed, not picked; a 40-minute run that ends says nothing unless its tab is watched; an evidence `file:line` cannot be opened in an editor; a graph export is a browser download | Sources, Run, the graph's object panel |
| P5 | The Microsoft Store package carries the command alone | `packaging/msix/README.md`: "Started from the Start menu it prints its help and the window closes at once" |
| P6 | Navigation is one header row and one wrapping row of 18 tabs plus extension tabs. No search across pages and commands. The theme follows the OS and cannot be chosen. The design tokens are 8 colours; spacing, type and radius have no scale | `WorkspaceShell.tsx::TABS`, `theme.css` |
| P7 | No updates and no code signing | RFC 0185 "Not built" |

## 2. Design

### 2.1 Shape

```
EKOS Desktop  (ekos-desktop: Tauri 2, Rust + the system webview)
  │  starts or attaches to, asks through `ekos console … --json`
  ├───────────────►  ekos console  (FastAPI, the bundled Python)        unchanged
  │                     ├── NDJSON/TCP ─► ekos mcp serve --tcp            reads
  │                     └── subprocess ─► ekos <command>                  writes
  │  one window (webview)
  └───────────────►  http://127.0.0.1:<port>/     the same built web/ui a browser gets
                        └── desktop bridge: a few native calls (§2.5), each with a browser fallback
```

Four rules:

1. **The shell holds no knowledge logic.** It opens no ledger, links no `ekos-*` crate and reads no
   project file except the home's control directory. Everything it knows it asks the `ekos` binary beside
   it or the console API.
2. **One UI.** The app shows `web/ui` as the console serves it. There is no second front end and no
   desktop-only page except the start-up screen and the app's own settings.
3. **The browser path stays.** `ekos console open`, team mode, the image and the public demo are
   unchanged. The app is another way in, not a replacement.
4. **Python stays.** Moving the console API into Rust would force the `KnowledgeStore: Send` decision
   (RFC 0127 §2.1, tech-debt Wave 2) as a side effect of a UI RFC. Not here.

Why this and not the alternatives:

| Option | Reuse | Cost | Verdict |
|---|---|---|---|
| **Tauri shell** | all of `web/ui` and `web/api` | one small Rust crate; three webview engines to test | chosen |
| Native toolkit (egui, Slint, iced) | none | every page rewritten (38 modules under `web/ui/src/pages`, a force-directed graph, forms from two schemas) and then kept in step with the web console forever | rejected |
| Electron shell | all of it | a bundled Chromium (about 100 MB more per install) and a Node runtime next to Rust and Python; in return one engine on all systems | fallback if D0 shows the system webviews are not good enough |
| Installed web app (PWA) | all of it | none | does not own the console's life, has no tray, no file dialogs, no Store entry |

### 2.2 Repository layout

```
desktop/                      a separate Cargo workspace, like benchmark/ and tests/integration/
  src-tauri/
    Cargo.toml                package ekos-desktop; depends on tauri and plugins only
    tauri.conf.json           one window; no bundled front end (devUrl / the console's address)
    capabilities/default.json the IPC allow-list (§2.4)
    src/{main.rs, lifecycle.rs, tray.rs, bridge.rs, local_pages.rs, update.rs}
    splash/index.html         the start-up screen (static, embedded)
  README.md
web/ui/src/desktop/bridge.ts  the page's side of the bridge (§2.5)
```

A separate workspace keeps GTK/WebKit and the Tauri tree out of `cargo build --workspace`, `clippy` and
the main CI job. `ci.yml` gains a `desktop` job (fmt, clippy `-D warnings`, tests) on Linux, and its
`web-os` job (Windows, macOS) gains a build of the app.

### 2.3 Life of the console

The app finds `ekos` beside its own executable (`<bin>/ekos`), as the console finds its runtime (RFC 0185
§2). It never parses human output; three commands gain `--json`:

| Command | Prints |
|---|---|
| `ekos console status --json` | `{state: running\|starting\|stopped, mode, host, port, since, supervisor, console, active_runs}` |
| `ekos console open --start --no-browser --json` | `{url, code}` after starting the console if needed (both flags exist; `--json` is new) |
| `ekos console stop --json [--wait\|--force]` | `{stopped: true}` or `{stopped: false, active_runs: [{id, workspace, command, since}]}` |

**Start-up.**

1. Single instance: a second start focuses the first window (and hands over a deep link, D6).
2. No `console.toml` in the home → first run, §2.6 "Set up".
3. `status --json`. `running` → attach, and remember the console is **not ours**. Otherwise
   `open --start` → the console is **ours**. The supervisor stays `ekos console run`, so the
   restart-on-crash policy (5 in 10 minutes) stays in one place.
4. Team mode in `console.toml` → v1 shows one screen: "This install is in team mode. Open it in the
   browser", with the button. (Local first.)
5. The window loads `url` with the one-time `code` (`/api/auth/local?code=…`): the cookie is set and the
   console's own redirect lands on `/`. The person never sees a sign-in screen.
6. Until the console answers, the window shows the embedded start-up screen: the stage, the last lines of
   `control/console.log`, and after a deadline the error with "Show log" and "Open the home".

**Closing and quitting.**

- Closing the window never quits: the app stays in the tray (Windows, Linux) or the dock (macOS).
  Schedules and the MCP gateway live in the console, so it keeps running. There is no setting for this.
- Quit is the tray menu's "Quit EKOS.DEV". On macOS the standard application-menu Quit (⌘Q) takes the
  same path. Where a desktop has no tray (§6.8), closing the window minimises it instead of hiding it.
- Quit with runs in progress asks: *keep EKOS.DEV running in the background* / *wait for N runs* /
  *cancel them and quit*. The app never force-ends a console that has active runs.
- Quit stops the console only when it is **ours** and "keep the console running after quit" is off. A
  console started by the login service is left alone.

**`ekos console stop` (closes the TODO item).** The console, on seeing `stop-requested`, refuses while
runs are active and writes `control/stop-refused` (the active runs) unless the marker carries `force`;
`--wait` waits for them. `stop` without a flag prints which runs hold it and exits non-zero. The MSI's
`StopConsole` custom action and `ekos uninstall` pass `--force` after telling the person.

**Tray.** State (stopped / running / N runs / a run failed), Open EKOS.DEV, the projects (each opens
its dashboard), Open in browser, Restart console, Quit EKOS.DEV.

### 2.4 The window and its trust boundary

- The webview loads only the console's loopback origin. Any other navigation, and every new-window
  request, is refused in the app and opened in the system browser.
- IPC is granted to that origin alone (a Tauri capability with a remote URL scope) and only for the
  commands in §2.5. No file-system, shell or HTTP plugin is exposed to the page.
- Every bridge call checks its arguments in Rust against what the app can establish itself: a path to
  open must exist and lie under a source root of a project in this home (read from the home, not from
  the page).
- The local-mode checks of RFC 0184 §3 (loopback address, `Host` header, `SameSite=Lax` cookie) are
  unchanged and still apply to the webview's requests.
- The console sends no `Content-Security-Policy` today. It gains `default-src 'self'` plus what the
  pages need (measured, not guessed); this also benefits the browser.
- Developer tools are off in release builds.

### 2.5 The desktop bridge

`web/ui/src/desktop/bridge.ts` exports plain functions. `isDesktop()` is true inside the app. Each call
has a browser fallback, so no page branches on where it runs.

| Call | In the app | In a browser |
|---|---|---|
| `pickDirectory()` | native folder dialog | the text field, as today |
| `saveFile(name, bytes)` | native save dialog | a download |
| `notify(kind, title, body, link)` | OS notification; a click focuses the window and opens `link` | an in-page toast |
| `openEvidence(path, line)` | the configured editor at the line (`code -g`, `idea --line`, else the default app) | copies `path:line` |
| `openExternal(url)` | the system browser | `target="_blank"` |
| `setActivity({running, failed})` | tray state, taskbar/dock badge | a prefix in `document.title` |
| `appInfo()` | versions, home, log paths, update state | — |
| `consoleControl(restart \| logs)` | through `ekos console …` | — |

First uses: Sources and Workspaces (pick a directory), Run and RunDetail (`notify` when a run ends or
fails, when a review is waiting, when a schedule's run fails), the graph's object panel (`openEvidence`
beside every file and line), graph and documentation exports (`saveFile`).

### 2.6 The `local` commands: every function without a terminal

| Commands (class `local`) | In the app |
|---|---|
| `setup` | **Set up**: the app starts `ekos setup --no-browser --port 0`, and shows the wizard's page (served by the binary, token in the URL) in an app window. On first run it comes before anything else; later from the app menu |
| `uninstall` | **Uninstall EKOS**: Settings already starts `ekos uninstall --ui --detach` in local mode (RFC 0188); the app shows that page in an app window and closes itself before its own files are removed |
| `console run/start/stop/restart/status/open/logs` | the app's own life (§2.3), the tray, and a **Console** pane in the app's settings: state, address, log tail, restart |
| `project add/list/show/remove`, `init` | already in the console: Workspaces (create and remove home projects) and Init & compile (RFC 0189). The app adds the folder dialog |
| `home backup`, `home restore` | app menu: **Back up the home…** (save dialog) and **Restore…** (the app stops the console under §2.3's rules, restores, starts it) |
| `mcp serve` | not run by a person. **Agents** in Set up registers EKOS with Claude Code, Claude Desktop and VS Code (RFC 0185 §3 step 3) |
| `commands`, `extensions` | what the console builds from; no page |
| `coordinator *`, `compile-worker run`, `query-worker serve` | server roles of the distributed mode; not for a desktop |
| `marketing *`, `eval bench *` | the owner's tooling; terminal only, by decision |

**Coverage as a test, not a claim.** `desktop/coverage.toml` names, for every `local` command, one of
`app:<where>`, `console:<page>`, `none:<reason>`. A test reads `ekos commands --json` and fails when a
`local` command has no row or a row names no command (the both-ways pattern of `CLASSES`). The other
five classes are covered by the Commands tab by construction (RFC 0186). `docs/desktop-coverage.md` is
generated from the two, so "every function from the interface" can be checked by anyone.

Console features that have a command but no page of their own yet (RFC 0247 `annotate resolve` on the
Review page, RFC 0260 X10/X11, RFC 0248 §2.5) stay in their RFCs. They are reachable through Commands
today and appear in the generated document as "form only".

### 2.7 One refreshed UI (`web/ui`, for the browser and the app)

**What stays.** React 18, the router and its per-route `lazy`, TanStack Query, plain CSS with variables,
the house palette (violet `--accent-a`, green `--accent-b`), the StepBar, every page's logic and tests.
No CSS framework is introduced: the UI has six runtime dependencies and restyling 30 pages onto a
utility framework would be churn without a reader-visible gain.

**What is added.** Radix primitives for the behaviours that are hard to get right by hand (dialog,
popover, dropdown menu, tooltip, tabs), `cmdk` for the palette, and two fonts shipped in the bundle
(Inter, a monospace): an app that runs offline cannot fetch fonts.

**Tokens.** `theme.css` grows from 8 colours to a small system: a 4 px spacing scale, a type scale,
radii, two elevations, semantic colours (`ok`, `warn`, `bad`, `info`) with their soft backgrounds, focus
ring, and a compact / comfortable density. Light, dark and "follow the system" become a choice kept per
person (`data-theme` on `<html>`; today only `prefers-color-scheme`). `docs/assets/theme.css` stays the
source the tokens mirror.

**The app shell.** The header row and the 18-tab row give way to:

```
┌────────────┬───────────────────────────────────────────────────────────────┐
│ EKOS       │ project ▾   Graph                ⌕ Search or run…  ⌘K   ● you │
│            ├───────────────────────────────────────────────────────────────┤
│ Overview   │ step bar (as today)                                           │
│ Explore    │                                                               │
│ Decide     │ page                                                          │
│ Check      │                                                               │
│ Operate    │                                                               │
│ Configure  │                                                               │
│ Extensions │                                                               │
├────────────┴───────────────────────────────────────────────────────────────┤
│ ● console running · ledger <commit> · 2 runs · MCP ready          v1.x.y   │
└────────────────────────────────────────────────────────────────────────────┘
```

| Group | Pages (today's tabs, nothing dropped) |
|---|---|
| Overview | Dashboard, Sources |
| Explore | Graph, Chat, Transformations, Interaction |
| Decide | Review, Semantics, Conflicts, Questions, Inbox, Notes |
| Check | Integrity, Evals |
| Operate | Run, Commands, History, Schedules |
| Configure | ekos.toml; and, outside a project, Settings, Admin, Channels |
| Extensions | one entry per page the binary declares (RFC 0198) |

The sidebar collapses to icons and remembers its state. Counts that need a person (open conflicts,
waiting reviews, unanswered questions) show as badges on their entries.

The rules that hide entries today (`RESTRICTED_TABS`, `READ_ONLY_HIDDEN`, `extensionTabs`,
`mayOperate` / `mayReview` / `mayAdmin`, `administers`) move unchanged into one navigation model,
`nav.ts`, that the sidebar, the palette and the app's native menu all read. `WorkspaceShell.test.ts`
and `Layout.test.tsx` carry over against it. They hide; the API still enforces.

**The command palette (Ctrl/⌘ K).** One box that goes to a page, switches project, finds an object
(`/search`, opening the graph focused on it), and runs a command: the entries come from
`GET /api/workspaces/{id}/cli`, so the palette offers exactly what the caller may run, opens the same
generated form, and shows the same typed confirmation for `admin` and `external` commands. Nothing runs
from the palette without the form.

**The page pass.** In this order, because it is the order people meet them: Workspaces and the
first-run empty state; Dashboard; Graph with a resizable, pinnable evidence panel; Run, RunDetail and
History; the Decide pages (one list-and-detail pattern for Semantics, Conflicts, Questions, Review);
forms (Sources, the settings generated from the schemas); the rest. Each page gets a loading skeleton,
an empty state that says what to do next, and errors that name the failing request. Keyboard: every
action reachable without a mouse, visible focus, reduced motion honoured, contrast AA in both themes.

**In the app only.** A native menu bar built from `nav.ts` (File, Edit, View, Go, Window, Help), the
standard zoom and reload shortcuts, the window's size and position restored, and on macOS the title bar
merged into the top bar.

### 2.8 Packaging, updates, signing

- **One installer, as today.** The app joins the full package and the installers the release already
  builds; Tauri's own installers are not used. Windows MSI (`packaging/wix`): `bin\ekos-desktop.exe` and a
  Start-menu entry "EKOS.DEV", which replaces the "EKOS Console" shortcut; the browser stays reachable
  through `ekos console open` and the tray's "Open in browser". macOS pkg: `EKOS.DEV.app`, built with
  Tauri's bundler (`app` target only) and installed by the pkg. Linux deb/rpm: the binary, a `.desktop`
  entry named "EKOS.DEV", icons, and a dependency on WebKitGTK 4.1.
- **WebView2.** Part of Windows 11. On Windows 10 the MSI checks for it and says how to get it (or
  carries the bootstrapper — decided in D0).
- **Updates (D4).** Once a day and on request the app asks GitHub Releases for the latest version and
  shows the notes. "Install" downloads the platform's installer, checks it against `SHA256SUMS`, stops
  the console under §2.3's rules and starts the installer. On Linux packages it shows the command.
  Whether Tauri's updater plugin can drive our own MSI and pkg, or the app does this itself, is decided
  by a spike in D4. The Store build has no updater.
- **Microsoft Store (D5).** Tauri's bundler has no MSIX target; the release builds its own
  (`packaging/msix`), with `ekos.exe` only. D5 makes the app the package's `Application` and keeps the
  `ekos` alias. It needs the full package inside the MSIX, which raises questions listed in §6. Until
  D5 the Store package stays the command.
- **Signing (decided).** Windows: the Microsoft Store is the only signed channel; Partner Center signs
  the MSIX. The MSI on GitHub Releases stays unsigned, with the SmartScreen warning it has today. macOS:
  no Developer ID for now, so the pkg and the app are unsigned and not notarised, and Gatekeeper stops the
  first start until the person allows it; `INSTALL.md` says how, as RFC 0185 does for the command. Both
  warnings are more visible for an app than for a command, which is why the Store build moves up (§3).

### 2.9 Three webview engines

Windows uses WebView2 (Chromium), macOS WKWebView, Linux WebKitGTK. The console's browser checks
(`deploy/e2e`, Playwright) run in Chromium only.

- The graph is `react-force-graph-2d`, which draws on a 2D canvas, not WebGL; the heavier risk
  (WebGL on WebKitGTK) does not apply to it.
- WebKitGTK is known to give a blank window on some GPU and driver combinations (most often NVIDIA);
  Tauri documents an environment switch for it. The app applies it only when it detects the case, offers
  it as a setting, and always offers "Open in browser" on the start-up screen.
- The browser stages in `deploy/e2e` run in WebKit as well as Chromium. Playwright's WebKit is an
  approximation of WKWebView and WebKitGTK, not the same build; D0 and each phase's exit run are on the
  real systems.

## 3. Phases

Each phase is merged on its own and may get its own RFC number when it starts, as the web console's did
(0128–0133).

| Phase | Content | Exit |
|---|---|---|
| **D0 Spike** | A window that starts or attaches to the console and signs in, on Linux, Windows and macOS. Measures: cold start; whether a hidden window keeps its event stream; IPC scoped to the loopback origin; WebView2 on a clean Windows 10; one Linux machine with an NVIDIA driver | A devlog with the measurements and go, or go with Electron, per §2.1 |
| **D1 Shell** | `--json` on three commands; `console stop` refuses or waits; life cycle; start-up screen; tray; single instance; external links; the app in the MSI, pkg, deb and rpm | A person installs the package, starts "EKOS.DEV" and is on Workspaces, signed in, without a terminal or a browser, on the three systems. Quitting with a run in progress loses no run |
| **U1 Tokens and shell** | §2.7: tokens, the app shell, `nav.ts` | Every page of today is reachable; the role tests pass against `nav.ts`; the `deploy/e2e` browser stages pass in Chromium and WebKit; the browser console looks the same as the app |
| **D2 Bridge** | §2.5 and its first uses | Add a source by picking a folder; a run that ends while the window is hidden notifies, and the click opens the run; an evidence line opens in the editor |
| **U2 Palette** | §2.7: the command palette | A command the caller may not run is not offered; an `admin` command still asks for its name |
| **D3 Local commands** | §2.6 and the coverage test | `docs/desktop-coverage.md` has no `local` command without a row; set-up, back-up, restore and uninstall done from the app in a fresh home |
| **U3 Page pass** | §2.7: the page pass | Per page: skeleton, empty state, keyboard path, both themes |
| **D4 Updates** | §2.8 | An older install updates itself to a test release on Windows and macOS; a wrong checksum is refused |
| **D5 Store** | §2.8 | The MSIX passes the Windows App Certification Kit locally; submission is the owner's |
| **D6 Later** | Remote servers (team mode, sign-in through the system browser), `ekos://` links, a window per project | — |

D1 and U1 do not depend on each other and can run in parallel; D2 needs D1; U2 needs U1; D3 needs D1.

**Order after the owner's decisions.** D5 (Store) comes before D4 (updates) and right after D1: the
Store is the only signed way to get the app on Windows, and a Store install is updated by the Store, so
D4 then serves the MSI, macOS and Linux only. The phase names stay as they are.

One question D3 raises: the MSI's other shortcut, "EKOS Setup", once Set up is in the app.

## 4. Tests (written first)

- **Rust, `ekos`:** `status|open|stop --json` shapes; `stop` with an active run refuses and names it,
  `--wait` waits, `--force` ends it; the console writes and clears `stop-refused`.
- **Rust, `ekos-desktop`:** the life-cycle state machine against a fake `ekos` (attach, start, ours / not
  ours, team mode, start failure with a log); quit decisions for each combination of ours, active runs and
  settings; navigation policy (loopback origin allowed, every other refused); bridge argument checks (a
  path outside every source root refused); `coverage.toml` complete both ways against the real catalogue.
- **Console API:** the CSP header on pages and no violation reported in the browser stages;
  `stop-requested` honoured only without active runs.
- **UI:** `nav.ts` for each role, restricted reader and read-only showcase (the existing cases, moved);
  the palette lists only allowed commands; bridge fallbacks in a browser; theme choice kept and applied
  before first paint.
- **End to end:** the `deploy/e2e` browser stages in Chromium and WebKit; on the three systems, one scripted
  run per phase exit (install, start, sign-in, add a project by folder, Init & compile, notification, quit
  with a run in progress), recorded in the phase's devlog with logs.

## 5. Not in scope

- The console API in Rust, or removing the bundled Python.
- Remote servers in v1 (D6), mobile, a second UI.
- New console features. Pages for RFC 0247's `annotate resolve`, RFC 0260 X10/X11 and RFC 0248 §2.5 stay
  in their RFCs.
- Tauri's installers, and any installer change beyond adding the app to the existing ones.

## 6. To verify (D0 and D4/D5), not assumed here

1. Tauri's remote-origin capability scoped to `http://127.0.0.1:<port>` with a port chosen at run time.
2. A hidden window keeps its `EventSource` open on all three engines; if not, the shell watches runs
   itself (it would then need its own session).
3. Cold start of the console under the app, and whether the start-up screen needs a progress stage per
   step.
4. WebView2 presence on supported Windows 10 builds (the manifest's minimum is 10.0.17763).
5. Whether Tauri's updater can use installers it did not build (D4).
6. MSIX with the full package (D5): the bundled Python running from a read-only package directory; the
   home in `%USERPROFILE%\.ekos` from a packaged app; the Store's rules on WebView2 distribution and on
   self-updating.
7. Intel macOS has no full package today (no `cryptography` wheels); the app follows the full package, so
   it is Apple silicon only until that changes.
8. The tray on Linux: some desktops show no tray icons without an extension (GNOME is the usual case).
   D0 checks the supported distributions; without a tray the window minimises on close (§2.3).
9. macOS without a Developer ID (D4): whether an installer the app downloaded starts, or Gatekeeper
   stops it again at every update.

## 7. Owner's decisions (2026-11-18)

| # | Question | Decision | Where it lands |
|---|---|---|---|
| 1 | Signing | Windows through the Microsoft Store only; no Apple Developer ID for now | §2.8, order of D5 and D4 in §3 |
| 2 | The app's name where it shows | EKOS.DEV, as reserved in the Store | Start menu, `.app`, `.desktop`, window title, tray |
| 3 | Closing the window | Stays in the tray; Quit is in the tray menu | §2.3 |
| 4 | The browser shortcut once the app ships | Removed | §2.8 |
