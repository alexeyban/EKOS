# Devlog 249 — Disputed claims: `ekos ask`, table links, and the console Conflicts tab

**Date:** 2026-10-11
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

Devlog 248 left two gaps in RFC 0173 and one open item in RFC 0172. All three are closed. `ekos ask` now tells the model which claims are disputed. A label conflict is linked to the table its column belongs to, so a question about the table surfaces it. The web console has a Conflicts tab where a write-role user settles a conflict, and the graph rings every object that has an open one (RFC 0174).

---

## RFC 0173 amendment — `ekos ask` and table links

### Problem / motivation

- `ekos ask` reported the kept side of a disagreement as fact, because only the MCP query tools marked disputes.
- On LedgerSMB, asking about the `oe` table did not surface the two `oe_class_id` label conflicts. They dispute `EnumMeaning` objects, not the table.

### What was built

| Component | |
|---|---|
| `AiRuntime::with_disputes(lookup)`, `DisputeLookup` | marks the evidence set inside `gather_evidence` |
| `reason::DISPUTED_MARKER`, `render_evidence` | a disputed claim's line ends with the marker; each `RSN008` becomes a `Note:` line |
| `conflicts::column_links` (semantic) | a second `Disputes` link from a label conflict to its column's table, with `attribute = columns.<col>.meaning` |
| `commands/conflicts.rs` | `commit_step` writes the links; `open_items_for` reads a link's own attribute and subject name; `dispute_lookup` for `ekos ask` |

### Implementation details worth remembering

- **The system prompt is unchanged.** Only the evidence block changes, and only when a claim is disputed. A test asserts that the prompt with an empty lookup is byte-identical to the prompt with none. That is why this shipped without an RFC 0138 eval run.
- **The `Note:` lines exist because of a failed first test.** "what is the orders table" compiles to search and graph claims only. None states an attribute, so nothing was marked and the model saw nothing. The entity-level `RSN008` is now rendered into the evidence block.
- **The table link names the attribute on the relationship, not on the conflict.** The conflict's own `attribute` stays `label`, so its id, signature and any review are unchanged. Re-committing an existing workspace only appends the links.
- **Unique table name only.** Two tables with the meaning's table name get no link.

### Verified on LedgerSMB (a scratch copy, re-committed)

`ekos_query "what are the fields of `oe`"`: both `oe.columns` claims flagged with the two label conflicts; `name`, `kind`, `description` and `sql_comment` not flagged; one `RSN008` for `oe (columns.oe_class_id.meaning)`.

---

## RFC 0174 — Console conflicts view

### What was built

| Component | |
|---|---|
| `web/api/app/routes/conflicts.py` | `GET …/conflicts` (read), `POST …/conflicts/{id}/resolve` (write) |
| `web/api/app/conflicts_write.py` | `resolve_argv`: one decision, conflict named by UUID, values inside their flags |
| `web/api/app/readproc.py` | `conflicts list --json` on the read-only allowlist |
| `web/ui/src/pages/Conflicts.tsx`, `conflicts-shared.ts` | the tab: list, claims with `path:line`, pick / both valid |
| `web/ui/src/pages/Graph.tsx`, `GraphCanvas.tsx` | an amber ring and a tooltip line on objects with an open conflict |

### Decisions

- **The same write seam as the Semantics tab.** The route runs the human-only CLI command for a write-role user and records who decided. Nothing goes through MCP.
- **The ring is drawn from the list of open conflicts**, not from a new graph-export field. The graph payload and its cache key are unchanged.

### Verified against the real CLI

On the scratch LedgerSMB copy, through the API with the real `ekos` binary: the list returned the 3 open conflicts, and a both-valid decision on `oe.oe_class_id = 1.label` was recorded as `token:Console Test` and came back `dismissed` with its note.

---

## Knowledge Captured

- **A flag on a claim does not reach a reader who only sees claims that state no attribute.** Check the rendered output for the question shapes people ask, not only the one the feature was designed around.
- **`"oe columns"` resolves to a Perl sub named `columns`**, not to the `oe` table. Short two-word questions pick the exact-name object. Use a backticked name when testing.
- **Vitest prints `EACCES … node_modules/.vite/vitest` in a sandboxed shell** after all tests pass. It is the results cache, not a test failure.
- **`mcp.rs` may not contain `Resolution::`**, so nothing in this change touches it.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/runtime/src/{ai,reason,lib}.rs` | `with_disputes`, `DisputeLookup`, `DISPUTED_MARKER`, note lines, 1 test |
| `ekos/crates/semantic/src/conflicts.rs` | `column_links`, 1 test |
| `ekos/crates/cli/src/commands/{conflicts,ask}.rs` | link writing and reading, `dispute_lookup`, 1 test |
| `ekos/docs/rfcs/0173-*.md`, `0174-console-conflicts-view.md` | amendment; new RFC |
| `web/api/app/{routes/conflicts,conflicts_write,readproc,main}.py`, `tests/test_conflicts.py` | routes, argv, allowlist, 5 tests |
| `web/ui/src/pages/{Conflicts,conflicts-shared,Graph,GraphCanvas}.*`, `api/types.ts`, `main.tsx`, `WorkspaceShell.tsx`, `index.css` | the tab, the ring, 5 tests |
| `README.md`, `web/README.md`, `CHANGELOG.md`, `CLAUDE.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | documented; TODO items ticked |
