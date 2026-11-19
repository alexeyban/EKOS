# Devlog 338 — The console page pass (RFC 0261 U3)

**Date:** 2026-11-19
**PRs:** none (pushed to `main`)
**Branch:** main

---

## Summary

Every page of the web console now uses the shell from devlog_337. The pass covers the Dashboard, the
Decide pages, the Configure forms, and the Explore, Check and Operate pages. Each page has:
- the same header;
- a loading skeleton;
- an empty state that says what to do next;
- errors that name the request that failed.

The Decide pages share one list-and-detail layout, and the Configure pages share one form layout.
Charts and the graph canvas follow the light or dark theme. Behaviour, API calls and permission
checks are unchanged. The owner's local-mode reviewer fix is committed separately, just before this
change (`3719367c`). The two places it missed (Inbox and Questions) are finished here.

---

## The page pass

### Problem / motivation
RFC 0261 §2.7 asks for a page pass after the shell: "per page: skeleton, empty state, keyboard path,
both themes". Before this change:
- pages said "loading…" or nothing;
- empty lists were bare;
- the Dashboard's charts and the graph canvas used fixed colours that ignored the theme;
- the Decide pages each built their own version of a list and a panel.

### What was built
| Component | Change |
|---|---|
| `web/ui/src/ui.tsx` (new) | `PageHead`, `Skeleton`, `Empty`, `ErrorNote`, and `QueryView` (a query's loading, error, empty and data states in one place) |
| `useChartColors.ts` (new), `theme.ts` | Chart and canvas colours read from the tokens, and read again when the theme changes. `withAlpha` handles hex and `rgb()` tokens |
| Dashboard | Header with backend and last write. "Nothing is compiled yet" with Init and compile. Charts in the theme's colours. Integrity errors tile links to Integrity |
| Decide (`pages/decide-kit.tsx`, `styles/decide.css`) | `SplitView`, `PickRow`, `DetailPanel` and `ReviewerField` for Review, Semantics, Conflicts, Questions, Inbox and Notes, with LinkML and Ossie restyled to match. The panel is sticky and stacks under the list at ≤760px. LinkML's "Load file…" is reachable by keyboard |
| Configure (`styles/forms.css`) | One field and card system (`fm-*`) for Sources, Settings, ekos.toml, the generated settings form, Admin, Channels and Schedules. Large components split up (Admin, Sources). Channels' forms are real `<form>`s. Schedules' selects got labels |
| Explore, Check, Operate (`styles/explore.css`) | Graph title in its toolbar; emoji buttons replaced by icons. Chat bubbles with the composer docked, and its attach button reachable by keyboard. Run, History and Commands follow RunDetail. Extension tables select by a real button per row, no clickable `<tr>` |
| Graph canvas | Ground, labels, links, controls, timeline and banner from the tokens. Kind, impact and disputed colours keep their meaning |
| `index.css` | ~465 lines of rules no page uses any more removed, each class checked against every `.ts`/`.tsx` file and the `deploy/e2e` scripts. The old colour aliases are replaced by the tokens |
| Reviewer name | Inbox and Questions require the name in every non-OIDC mode. `whoAmI` gives `local:<name>` in local mode. `Me.mode` includes `"local"`. Two tests cover it |

### Decisions
- **Three agents, one brief, separate files.** Each group of pages kept its CSS in its own file
  (`styles/<group>.css`, loaded after `index.css`) with its own class prefix (`dk-`, `fm-`, `x-`), and
  edited no shared file. Shared changes and unused selectors came back as reports, and the lead applied
  them. Nothing collided.
- **Visual pass only.** Request bodies, permission checks and the reviewer marking are unchanged.
  Tests were changed only where the markup changed, with the same assertion meaning: empty-state
  titles, a skeleton instead of "loading…", an alert role, a row button instead of a cell, and
  renamed diff classes.

---

## Knowledge Captured
- **Screenshots against fixtures.** Seven pages throw when an answer lacks a field they read:
  Graph, Commands, ekos.toml, Settings, Review, Sources and Conflicts. Comparing against a build of
  the previous commit showed that the same seven threw before the pass, so it is not a regression.
  Two that the pass made strict (Channels, Interaction) were made tolerant again. To tell a fixture
  problem from a regression, build `HEAD` in a worktree and run the same fixtures against both.
- `pkill -f "vite preview --port 4179"` kills the shell that runs it, because the pattern is in its
  own command line. Stop a preview by the PIDs whose `cwd` is the UI directory.
- Killing the `npm exec` PID of `vite preview` leaves the `node` child holding the port. Start it as
  `node node_modules/.bin/vite preview` to get one process.
- An `npm install` typed with the `!` prefix runs in the session's working directory. Suggested
  from the repository root, it put `package.json`, `package-lock.json` and `node_modules/` there.
  They are untracked, are not part of this change, and can be deleted.

## Not verified yet
- The `deploy/e2e` browser stages in Chromium and WebKit, which are U1's exit. The selectors they use
  were kept (`section.commands-form`, `#arg-query`, `pre.argv`, `section.chat-main`, the chat classes,
  and the Send and Run button names).
- What was checked: 257 unit tests, typecheck, build, `sonar-local.sh`, a scan for the rules
  SonarCloud caught last time, and headless screenshots of 22 pages in both themes (fixtures).

## Files Changed
| File | Change summary |
|---|---|
| `web/ui/src/ui.tsx`, `useChartColors.ts`, `theme.ts`, `theme.test.ts` | page primitives, theme colours |
| `web/ui/src/styles/{decide,forms,explore}.css`, `main.tsx` | per-group page styles |
| `web/ui/src/pages/*.tsx` (36 pages), `pages/decide-kit.tsx`, `people-shared.ts` | the page pass |
| `web/ui/src/pages/*.test.tsx` (6) | same assertions, new markup; local-mode reviewer tests |
| `web/ui/src/index.css`, `shell.css`, `Icon.tsx`, `WorkspaceShell.tsx` | unused CSS removed, tokens, `info` icon, `Me.mode` |
| `TODO.md`, `docs/generated/ekos-self-documentation.html` | U3 |
