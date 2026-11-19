# Devlog 337 — The refreshed console: sidebar shell, palette, themes (RFC 0261 U1/U2)

**Date:** 2026-11-19
**PRs:** none (pushed to `main`)
**Branch:** main

---

## Summary

RFC 0261 §2.7 describes one refreshed UI for both the browser console and EKOS.DEV. This change
lands its first two UI phases. **U1** brings the tokens, a per-person theme and the app shell, built
on one navigation model. **U2** is the command palette. Three pages from the **U3** page pass are
also redone: Workspaces, RunDetail and the graph's evidence panel. The 18-tab row and the header row
are replaced by:
- a sidebar of grouped pages, with badges;
- a top bar with the project switcher, the breadcrumb, the search box and an identity menu;
- a status bar.

Every page that existed is still reachable, and the same role rules decide what is shown.

The release workflow does not rebuild the console UI for the installers from this commit, so v1.6.0
still ships the old look. The next release carries this one.

---

## The console shell

### Problem / motivation
The console had grown to 18 tabs in one row, and its look did not match the UI mockups for EKOS.DEV.
The rules that hide entries were spread over the header, the tab row and the pages. The browser read
only `prefers-color-scheme`, and the fonts came from the system.

### What was built
| Component | Change |
|---|---|
| `web/ui/src/nav.ts` (new) | The single navigation model: groups Overview, Explore, Decide, Check, Operate, Configure and Extensions. `RESTRICTED_TABS`, `READ_ONLY_HIDDEN`, `administers` and the extension tabs moved here unchanged. The sidebar, breadcrumb and palette all read it. `WorkspaceShell.tsx` re-exports the names it used to own |
| `Layout.tsx` | The sidebar: collapses to icons and remembers that; badges for Review, Conflicts and Questions. The top bar: project switcher, breadcrumb, search box, theme button, identity menu (sign out moved there). The status bar: console health, ledger object count, runs in progress, MCP server state, version. The workflow step bar sits above the page |
| `Palette.tsx` (new) | Ctrl/⌘ K, on a native `<dialog>`. Covers pages, projects, objects (`/search`, opening `graph?focus=<id>`), and the commands from `GET /workspaces/{id}/cli`, which open their form on Commands through `?cmd=`. Nothing runs from the palette. `rank` and `paletteRows` are pure and tested |
| `theme.css`, `theme.ts`, `public/theme-init.js` | Tokens: spacing, type scale, radii, elevations, semantic `ok`/`warn`/`bad`/`info` with soft backgrounds, focus ring. Light, dark or follow-the-system, kept per person (`localStorage` `ekos-theme`), set as `data-theme` on `<html>` before first paint by a small external script |
| `shell.css` (new), `index.css` | Shell and shared page styles. `index.css` loses the rules the shell replaced |
| `Icon.tsx` (new) | The one place icons are named (`lucide-react`), so pages ask for `"graph"`, not a component |
| `format.ts` (new) | `commandTitle`, `relativeTime` |
| Pages | Workspaces: project cards with server state and the next workflow step. RunDetail: header chip, where a staged run is, time per command (`run-events.ts`). ObjectPanel: confidence words, a "disputed" mark (`Graph.tsx` passes it). Commands: opens a form from `?cmd=` |
| Dependencies | `lucide-react@0.469.0`, `@fontsource-variable/inter@5.1.1`, `@fontsource/jetbrains-mono@5.1.1`. Fonts are in the bundle, so an offline app needs no network for them |
| `scripts/sonar-local.sh` | eslint's exit code 1 no longer fails the run (see Knowledge Captured) |

### Decisions
- **No Radix or `cmdk` yet**, though §2.7 names them. The palette and menus use the native `<dialog>`
  and `<details>`, which give focus trapping, Escape and the backdrop for free. They ship with no new
  runtime dependency beyond the icons and the fonts. If a primitive turns out to be hard to get right
  by hand (tooltips, a resizable panel), it can still come from Radix in U3.
- **The rules moved, not rewritten.** `nav.ts` is a move of the hiding rules. The tests that pinned
  them (`ExtensionPage.test.tsx`, `Workspaces.test.tsx`, `Layout.test.tsx`) carry over unchanged in
  meaning, and new `nav.test.ts` and `Palette.test.ts` pin the model. The API still enforces every
  rule.
- **The theme script is a file, not inline.** It keeps a strict CSP possible without a hash, and it
  runs before React, so a dark theme never flashes light.

---

## Knowledge Captured
- `scripts/sonar-local.sh` piped eslint under `set -o pipefail`. eslint exits 1 for **any** finding
  in a file, including ones on lines nobody changed, which the changed-lines filter then drops. Here
  that was a `react-hooks/exhaustive-deps` disable comment in `Graph.tsx` naming a plugin the local
  config does not load. The script printed "fix the findings above" with nothing above. Now only
  exit 2 (eslint itself failing) is an error. I checked that a planted finding is still reported.
- jsx-a11y flags a `click` handler on `<dialog>` (S6847/S1082), even when it only closes the dialog
  on a backdrop click. Escape already closes a modal dialog (`cancel`), so the backdrop click is
  attached in an effect with `addEventListener` instead.
- The sidebar's review badge read `workflow.data.steps.find`. A workflow answer without `steps`
  threw inside the shell, and the router's error page replaced the whole console, all for a badge.
  Badge data is now read defensively.
- To screenshot the UI without a console, serve `vite preview` and answer `**/api/**` from fixtures
  in Playwright. A page that reads a large document (the Dashboard's stats) needs a full fixture, or
  it shows its error boundary.

- The first SonarCloud scan of this work was red (reliability C), and `sonar-local.sh` had passed it.
  The bug was S5850 in `format.ts`: in `/[zZ]|[+-]\d\d:?\d\d$/` the `$` binds only to the
  offset, so a `z` anywhere counted as a zone. The local S5850 check reads Python only. The scan also
  found `.dataset` over `setAttribute` (S7761), a second `:root` block (S4666), `var` (S3504) and the
  reduced-motion `!important` (S9423, kept with `NOSONAR`: it must beat every animation). None of
  these has a local stand-in yet.

## Not verified yet
- The `deploy/e2e` browser stages (Chromium and WebKit) against the new shell, which is U1's exit
  criterion. What was checked: unit tests (254), typecheck, build, the Sonar rules, and headless
  Chromium screenshots of Projects, the palette, a project's Run page (light and dark) and a phone
  width, with fixtures.

## Files Changed
| File | Change summary |
|---|---|
| `web/ui/src/{nav,Palette,Icon,format,theme}.ts(x)`, `shell.css`, `public/theme-init.js` | new: navigation model, palette, icons, helpers, theme |
| `web/ui/src/Layout.tsx`, `WorkspaceShell.tsx`, `main.tsx`, `index.css`, `theme.css`, `index.html` | the shell, tokens, fonts |
| `web/ui/src/pages/{Workspaces,RunDetail,ObjectPanel,Commands,Graph}.tsx`, `run-events.ts` | page pass (part) |
| `web/ui/src/**/*.test.ts(x)` | new `nav`, `Palette`, `format`, `RunDetail` tests; `Layout`, `ExtensionPage`, `Workspaces`, `run-events` tests updated |
| `web/ui/package.json`, `package-lock.json` | lucide-react, Inter, JetBrains Mono |
| `scripts/sonar-local.sh` | eslint exit code |
| `TODO.md`, `README.md`, `docs/generated/ekos-self-documentation.html` | U1/U2 |
