# Devlog 277 — console pages from extensions, and what an extension needs to install itself

**Date:** 2026-10-17
**PRs:** none (local commits on `main`, `[skip ci]`)
**Branch:** main
**Design:** RFC 0198 (console extension pages), RFC 0199 (Lakehouse console tab and installer)

---

## Summary

The web console can now show pages an extension declares: panels built from the extension's
own read commands and buttons that run its catalogue commands — data, not code, checked twice
against the catalogue. EKOS Lakehouse uses it for a **Lakehouse** tab, and installs with one
command (its own repository, devlog_9 there). Making that install work needed three small core
changes: `ekos setup` installs and starts the console under the running binary's name, the
full-package script packages any binary, and a connection's password can come from an
owner-only file, which is what a console job has instead of a shell's variables.

---

## RFC 0198 — console pages from extensions

### What was built
| Piece | What |
|---|---|
| `extension.rs` | `console_pages()`; `ConsolePage`, `Panel`, `View` (table / fields / text), `Column`, `Action` |
| `commands/extension_pages.rs` | `ekos extensions --json`: every page checked against the catalogue |
| `web/api/app/extensions.py`, `routes/extensions.py` | `GET /api/extensions`; `GET /api/workspaces/{id}/extensions/{ext}/{page}/{panel}` (`allow("read")`) |
| `readproc.read_checked_json` | the read runner for an argv the catalogue check accepted |
| `web/ui/src/pages/ExtensionPage.tsx`, `extension-view.ts` | one generic page; tabs after the built-in ones |

### Implementation details worth remembering
- A panel's argv must start with a catalogue command of class **read**, and everything after it
  must be that command's own flags; no value comes from the page or the request. Checked in the
  binary and again in the console before every panel request — a page that names `sync` is a 404
  and nothing runs (tests in both).
- Actions are not a new route: they post to `/workspaces/{id}/cli` like the Commands tab, so
  class, role, typed confirmation and the job runner all apply unchanged.
- `GET /api/extensions` is on the authz test's DYNAMIC list (authenticated, no workspace data).
- Restricted readers get no extension tabs: the commands read the whole project.

### Decisions
- No extension-supplied JavaScript: it would run with the signed-in person's session. Three view
  kinds cover operational pages; a new kind is a console change.

---

## RFC 0199 — the core part

| Change | Why |
|---|---|
| `setup::Machine::program` (the running binary's stem) | `ekos-lakehouse setup` copied `bin/` but planned services and follow-up commands for `bin/ekos` |
| `package-full.sh`: `EKOS_PACKAGE_PROGRAM`, `EKOS_PACKAGE_INSTALL_MD` | the Lakehouse full package is EKOS's, with its own binary name and INSTALL.md |
| `ConnectionRef::secret_file` + pg-live `resolve_secret` | the variable still wins; else the file's first line; refused when group/other can read it |
| UI: status chips do not wrap in tables | `incremental` broke over two lines |

---

## Knowledge Captured
- **A console job has its tenant's environment, not the console user's shell.** Tenant grants
  are tied to metered model calls (`allow_metered`), so a database password does not belong there;
  an owner-only file the project names works in console jobs and in containers alike.
- **The first browser load of an extension page raced the workspace's MCP server start** in the
  dev console; the panel query was still fetching when the screenshot was taken. Wait for data,
  not for the page.
- `node_modules/.vite` owned by root (an old container build) breaks `vitest run`; run with
  `--cache=false` rather than changing ownership of the user's files.
- Playwright from `uv run --with playwright` installs a newer version than the browsers in
  `~/.cache/ms-playwright`; launch with `executable_path` from that cache, as
  `deploy/e2e/package_browser_common.py` does.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0198-console-extension-pages.md`, `0199-lakehouse-console-installer.md` | the RFCs |
| `ekos/crates/cli/src/{extension.rs,app.rs}`, `commands/{extension_pages,catalogue,setup,setup_tests,setup_server,uninstall_server}.rs`, `tests/extension_commands.rs` | pages, `ekos extensions`, setup under its own name |
| `ekos/crates/{migrate/src/connection.rs,pg-live/src/{session,sandbox,lib}.rs}` | `secret_file` |
| `web/api/app/{extensions,readproc,main}.py`, `routes/extensions.py`, `tests/{test_extensions,test_authz_routes}.py` | console API |
| `web/ui/src/{WorkspaceShell,main}.tsx`, `pages/{ExtensionPage,extension-view}.ts*`, `index.css` | console UI |
| `scripts/package-full.sh` | any binary |
| `docs/deployment.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md`, `TODO.md` | docs |
