# RFC 0207 — Console pages v3 (selections, code, graph, markdown) and the Pentaho tab

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-18
**Builds on:** RFC 0198 (console pages from extensions), RFC 0186 (catalogue, `/cli`), RFC 0183
(tenant `allow_metered`), RFC 0204 (Pentaho translation report)
**Code:** EKOS (`extension.rs`, `commands/extension_pages.rs`, `web/api/app/extensions.py`,
`routes/extensions.py`, `routes/cli.py`, `web/ui/src/pages/ExtensionPage.tsx`,
`extension-view.ts`); `ekos-lakehouse` (`console.rs`, `pentaho report`)

## Goal

Let an extension page drill down — pick a package, then a step — show code next to the SQL it
became, draw a job graph, render a proposal, and run a command for the selected item, without
giving up RFC 0198's rule that a page is data the console checks against the catalogue.

## Design (all additive; a v2 page is a valid v3 page)

### Selections

`ConsolePage.params: Vec<PageParam>` — named selections the page keeps, each filled from a
**source panel**'s table rows (`rows` pointer, `key` pointer) by a dropdown or a click on a row.
`Panel.params: Vec<&str>` lists the selections a panel needs; the console appends each as
`--<name>=<value>` to the panel's command and does not run the panel until all are chosen.

Rules, checked by `ekos extensions --json` and again by the console:

- a selection's source panel exists on the page, has a table view, and does not itself need
  that selection (no cycles);
- every selection a panel lists is declared, and the panel's catalogue command has a **value**
  option of that name (not a flag, not a path);
- the value is text of at most 512 characters without NUL, passed as one `--name=value` argument.

A panel command stays class `read`. A value the person picks reaches only a read command's own
declared option — the same freedom the Commands tab already gives a reader (RFC 0186), so no new
authority is created.

### Actions with arguments

`Action.binds: Vec<(arg id, selection name)>` passes a selection to a catalogue argument;
`Action.flags: Vec<&str>` sets fixed boolean flags (e.g. `allow-metered`); `Action.confirm`
adds a sentence the person must acknowledge. The checks: every bound argument and flag exists on
the command, and no required argument is left unbound. Actions still run through `POST /cli` with
the command's class, role and typed confirmation.

**Metered calls:** `POST /cli` refuses (409) any command run with `--allow-metered` when the
workspace's tenant does not allow metered calls — enforced on the server, whatever the page says.
The page shows its own consent text first.

### Views

| View | Shows |
|---|---|
| `code { left, right, left_label, right_label }` | two strings side by side (original code / generated SQL) |
| `graph { nodes, edges, id, label, group, badge, from, to }` | a left-to-right DAG; nodes grouped (one lane per group), coloured by the badge value |
| `markdown { pointer }` | a Markdown string, rendered by the console's own minimal renderer (headings, lists, tables, code); raw HTML is shown as text |

## The Pentaho tab (lakehouse)

Selections: **package** (from *Packages*), **step** (from *Steps*, which needs package).

| Panel | Command | View |
|---|---|---|
| Packages | `lakehouse pentaho scan --json` | table: package, transformations, jobs, steps, unknown types, unresolved links |
| Translation | `lakehouse pentaho report --json --package` | fields: state, counts by fidelity |
| Graph | `report --package` | graph: steps by transformation, coloured by fidelity, edges = hops |
| Models | `report --package` | table: model, layer, materialization, fidelity, enabled |
| Steps | `report --package` | table: transformation, step, type, layer, fidelity, gap |
| Step | `report --package --step` | code: original Kettle text ↔ generated SQL |
| Questions | `report --package` | table |
| Proposal | `report --package` | markdown |

Actions: *Scan* (no argument), *Translate* (`pentaho translate`, package bound),
*Translate with a cloud model* (package bound, `--allow-metered`, consent text; RFC 0205). No
approve or merge button: merging the branch is the approval (RFC 0190).

## Tests

Rust: a selection without a source, a cycle, a panel naming an undeclared selection, a selection
that is not a value option, an action binding an unknown argument, all refused; v2 pages
unchanged. Console: a panel with a missing selection is 422, a value with NUL or over 512
characters is 422, a declared selection becomes `--name=value`; `--allow-metered` without the
tenant's permission is 409. UI: the three views render, a row click selects.
