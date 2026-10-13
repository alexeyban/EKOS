# Devlog 266 — Deployment check, second pass: browser, group claim, MCP client

**Date:** 2026-10-13
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

Devlog 265 left three things unverified: the console pages in a browser, group membership from
a provider's claim, and an MCP client other than the suite's own HTTP calls. `deploy/e2e/run.sh`
now covers all three and passes 143 checks. The browser found that the page of any workspace
outside the `default` tenant was a 404 on a direct load. The group stage needed a setting that
did not exist. Both are fixed, with one smaller fault in the graph page.

---

## What was added to the check

| Stage | What it does |
|---|---|
| `s6_groups.py` (9 checks) | Dex's mock connector signs in one person in the group `authors`. A tenant admin maps a group to that value and grants it viewer; the person, never listed, becomes a viewer; deleting the group ends it on the same session |
| `s7_mcp_client.py` (6) | The MCP project's Python client (`mcp` 2.3.0) connects to the gateway with a token: handshake, tool list, a search call, a refused review call, refusal without a token and on another tenant's project. Protocol negotiated: 2025-11-25 |
| `s8_browser.py` (18) | Headless Chromium through Playwright. Signs in by the provider's form as an admin, a limited viewer and an admin of another tenant; opens every page; fails on a script error, a failed request or missing text |

## Defects found and fixed

| Defect | Found by | Fix |
|---|---|---|
| `/w/acme.shop` answered 404 on a direct load or reload. The UI fallback took the dot in `<tenant>.<project>` for a file extension | browser | A file of the UI is under `assets/` or at the root; nothing else is (`main._SpaFiles`) |
| Sign-in asked only for `openid email profile`. Dex, Keycloak and Okta send groups only for the `groups` scope, so a claim-mapped group had no members and nothing said why | group stage | `EKOS_CONSOLE_OIDC_SCOPES`; must include `openid` |
| A limited viewer's graph page requested the timeline and open conflicts, both 403 for them | browser | The workspace shell renders a page only once it knows whether the caller is restricted; the graph skips the two requests (`seesAll`) |

### Decisions

- **Scopes are a setting, not a fixed list with `groups` added.** Entra ID rejects an unknown
  scope; a provider-specific default would break one provider to help another.
- **The shell waits for the workspace list before rendering a page.** Passing "restricted" down
  was not enough: on a direct load the page mounted and fired its requests before the list
  arrived. The list is one cached request.
- **The browser stage skips when no Chromium is installed**, and says so. The other stages
  need only Docker and `uv`.

---

## Knowledge Captured

- **A workspace id with a dot is a route, not a file.** RFC 0180 introduced `<tenant>.<project>`
  ids; the "has an extension, so it is an asset" rule from RFC 0179 predates it. No test
  loaded such a URL directly; clicking through the app never hits the server for it.
- **Dex's mock connector** (`type: mockCallback`) signs in `kilgore@kilgore.trout` with groups
  `["authors"]` and no form. With it enabled beside the password database, Dex shows a page of
  connector links first, with HTML-escaped query strings (`&#43;` for `+`): unescape them or
  the `state` is lost and the callback fails.
- **The MCP Python client 2.x** takes `Client(streamable_http_client(url, http_client=…))`;
  headers go on the HTTP client (`create_mcp_http_client(headers=…)`). `ping` is deprecated
  there. It worked against the gateway unchanged.
- **Chromium for a test provider on a Compose network:**
  `--host-resolver-rules=MAP dex 127.0.0.1`. A Chromium cached for another Playwright version
  works through `executable_path`.
- **`web/ui/node_modules/.vite` is owned by root on this machine**, so `vitest` prints an
  `EACCES` error after the results. The tests run; read the `Tests` line.
- **A config editor's text is not in `inner_text`**; assert on a button label instead.

---

## Files Changed

| File | Change summary |
|---|---|
| `deploy/e2e/s6_groups.py`, `s7_mcp_client.py`, `s8_browser.py` | New stages |
| `deploy/e2e/run.sh`, `lib.py` | Mock connector, scopes, connector choice in sign-in, stage order |
| `web/api/app/main.py` | UI fallback no longer treats a dotted route as a file |
| `web/api/app/settings.py`, `auth.py` | `oidc_scopes` |
| `web/api/tests/test_ui_fallback.py`, `test_auth.py` | New tests |
| `web/ui/src/WorkspaceShell.tsx`, `pages/Graph.tsx`, `WorkspaceShell.test.ts` | Wait for the caller's rights; skip refused requests |
| `deploy/.env.example`, `docs/deployment.md`, `CHANGELOG.md`, `TODO.md`, `CLAUDE.md` | Updated |
