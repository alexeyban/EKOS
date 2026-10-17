# RFC 0198 — Console pages from extensions

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-17
**Builds on:** RFC 0149 / 0191 (extension seam), RFC 0186 (command catalogue), RFC 0180 (console
access control), RFC 0185 (`ekos console`)

## Problem

An extension (RFC 0191) can add CLI commands and MCP tools, and the console already runs the
binary that started it (`ekos console` sets `EKOS_BIN` to its own executable), so a console
started by `ekos-lakehouse` sees the Lakehouse commands in its catalogue. It cannot show them as a
page: the workspace tabs (`WorkspaceShell.tsx`) and API routers (`main.ROUTERS`) are fixed lists.
RFC 0196 left the Lakehouse console tab out for exactly this reason.

## Design

### 1. Pages are data, not code

An extension describes pages; the console renders them with one generic page. Nothing an
extension ships runs in the browser or inside the console process: no HTML, no JavaScript, no
Python. A page is panels and actions:

```json
{ "schema_version": 1,
  "extensions": [{
    "name": "lakehouse",
    "pages": [{
      "id": "lakehouse", "title": "Lakehouse",
      "description": "Bronze, silver and gold per table: health, load modes, open proposals.",
      "panels": [{
        "id": "health", "title": "Health",
        "command": ["lakehouse", "status", "--json"],
        "view": { "kind": "table", "rows": "/tables",
                  "columns": [{"key": "/table", "label": "Table"},
                              {"key": "/bronze", "label": "Bronze", "badge": true}] }
      }],
      "actions": [{ "label": "Sync source changes", "command": ["lakehouse", "sync"] }]
    }]
  }]
}
```

- **Panel** — the JSON a command prints, laid out by a **view**: `table` (`rows` is a JSON
  pointer to an array, each column a pointer into a row), `fields` (label → pointer, for one
  object), `text` (a pointer to a string, shown preformatted). A column or field with
  `badge: true` shows its value as a status badge; the tone comes from the value
  (`healthy`/`ok`/`active`/`passed` good, `late`/`unverified`/`pending`/`warning`/`schema_pending`
  attention, `failing`/`failed`/`error`/`paused` bad, anything else neutral).
- **Action** — a catalogue command run as a job, exactly like the Commands tab: the console's
  `POST /workspaces/{id}/cli` with its class, role and typed confirmation (RFC 0186). A page
  cannot give an action an argument; an action with a required argument is refused when the
  page is declared.

### 2. A panel only reads

A panel's command must be in the catalogue, of class **read**, with a fixed argv and no
required argument. That is checked twice: in the binary (`ekos extensions --json` fails on a page
that breaks it, and a test holds every built-in check) and in the console, which re-reads the
catalogue class before every panel request and refuses anything that is not `read`.

### 3. Surfaces

| Where | What |
|---|---|
| `EkosExtension::console_pages()` | default empty; `ConsolePage`, `Panel`, `View`, `Column`, `Field`, `Action` (serde) |
| `ekos extensions --json` | every extension's name, version (`logic_version`) and checked pages; catalogue class `read` |
| console `GET /api/extensions` | the pages, read once from the binary (like the catalogue) |
| console `GET /api/workspaces/{id}/extensions/{ext}/{page}/{panel}` | runs that panel's command in the workspace (`allow("read")`, whole-workspace readers only, as every command route) |
| UI | one tab per page after the built-in tabs; `pages/ExtensionPage.tsx` renders panels and actions |

A restricted reader (RFC 0181) does not see extension tabs: their commands read the whole
workspace, the same reason the Commands tab is closed to them.

## Decided against

- **Extension-supplied UI bundles** (a JS file per extension): code from an extension would run
  with the signed-in person's session. The view kinds cover tables of states, which is what an
  operations page shows; a new kind is a console change, reviewed like any other.
- **MCP tools as panel sources**: the console's MCP client is per workspace and scoped; the
  catalogue already says who may run what, and a read command is the same data.

## Out of scope

Forms (actions with arguments — the Commands tab does that), charts, extension pages outside a
workspace.
