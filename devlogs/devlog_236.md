# Devlog 236 — Semantics review UI and the LinkML viewer/editor in the web console

**Date:** 2026-10-03
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

The RFC 0127 web console has a **Semantics** tab: the human review surface for RFC 0170's
business-semantics hypotheses, and a viewer/editor for the LinkML schema they export. Review is a
queue with each item's evidence lines, links and rationale commits, and Confirm / Reject / Edit.
The LinkML view is a schema browser plus a YAML editor whose edits round-trip into the ledger as
review decisions through a new CLI command, `ekos import linkml` (RFC 0170 Phase 4's round trip).

Every decision still goes through the human-only CLI commands; the console runs them for a
write-role user and records who that user is. Nothing goes through MCP.

---

## What was built

| Layer | Component | Change |
|---|---|---|
| Rust | `cli/commands/import.rs` | New `ekos import linkml <file> [--dry-run] [--json] [--as]`: diff an edited schema against the current export by `ekos_id`, plan decisions, apply all or nothing. 3 tests |
| Rust | `cli/commands/export.rs` | `ekos_id` on every recovered element, `ekos_export_status` on the schema, `--json`; an expert description replaces the generated one |
| Rust | `cli/commands/semantics.rs` | Read commands open the ledger **read-only** (they run beside the console's MCP servers); MCP guard extended to `import::linkml` |
| Rust | `semantic/semantics_review.rs` | Re-confirming an unchanged confirmation is a no-op. 1 test |
| API | `app/semantics_write.py` | The write seam: argv builders (every value inside `--flag=value`), reviewer identity, busy-ledger → 409, private temp file for imports |
| API | `app/routes/semantics.py` | `GET items`, `items/{id}`, `gaps`, `linkml`, `linkml/yaml` (read); `POST items/{id}/review`, `linkml/import` (write); `linkml/validate` (read — a dry run) |
| API | `app/readproc.py` | Allowlist: semantics list/gaps/show (UUID positional only), export linkml JSON/YAML; `read_text`; per-call output cap |
| API | `app/main.py` | SPA fallback: reloading a client-side route serves `index.html`, not 404 |
| UI | `pages/Semantics.tsx`, `SemanticsItemPanel.tsx`, `Linkml.tsx`, `semantics-shared.ts` | Review queue + item panel, gaps & conflicts, LinkML viewer + YAML editor |
| Tests | `api/tests/test_semantics.py` (8), `ui/src/pages/Semantics.test.tsx` (7) | Fake-CLI route tests; component tests |

---

## Implementation details worth remembering

- **Identity.** Under OIDC the reviewer is the signed-in email (or subject). Token mode has no
  identity — one shared write token — so the UI asks for a name and the ledger records
  `token:<name>`: an unauthenticated claim is never stored as if it were a person.
- **Argv safety.** Item ids must be UUIDs; free text (notes, names, descriptions, labels) is passed
  as `--note=<text>`, so a value starting with `-` can never become an option. No shell anywhere.
- **Reviews bypass the job queue.** They are single appends; if a pipeline run holds the ledger's
  write lock the CLI fails fast and the route answers 409 instead of queueing.
- **The YAML editor validates before it applies.** Validate is `import --dry-run --json`; Apply is
  enabled only after a clean validation with no edits since — the Config page's pattern.
- **Matching is by `ekos_id`, never by name**, so renaming a concept class in YAML is an edit of
  that concept, not a new one.

## Decisions

- **The editor edits YAML, the ledger stays the source of truth.** The console never stores a
  schema; Apply turns the edits into review decisions and the next export reflects them. New
  classes and deletions are reported, not imported.
- **No editor library.** A textarea (tab → two spaces) keeps the bundle and the dependency list
  unchanged; structured editing happens in the review panel.
- **`validate` needs only the read role**: a dry run writes nothing.

---

## Knowledge Captured

- **`StaticFiles(html=True)` is not an SPA server.** Reloading `/w/<id>/anything` on the
  production/Compose console returned `{"detail":"Not Found"}`; only `/` worked. Found while
  driving the new page with Playwright. Fixed with an `index.html` fallback for extension-less,
  non-`/api` paths.
- **Read commands must not take the write lock.** `semantics list/show/gaps` and `export linkml`
  opened the ledger writable; beside the console (MCP server per workspace, job runner) that is a
  conflict waiting to happen. They now open it read-only.
- **Playwright `fill()` on ~200 KB of text times out.** Set the value through the native setter and
  dispatch `input` — React picks it up.
- **The uv-fetched Playwright was newer than the installed browsers**; the system `python3`
  Playwright matched them.

---

## Verification

- LedgerSMB, through the real console (API on the built UI, Playwright/Chromium, no console
  errors): opened `AccTransApproved` with its 12 evidence lines and links, confirmed it as
  `token:Ann`; browsed `InventoryPart` and the `OeClassId` enum (the earlier relabel shows as
  confirmed); renamed `AccountEquity` → `EquityAccount` in the YAML editor and validated it into one
  `edit` decision. `ekos ledger audit` shows the console's writes as `stage=semantics-review`.
- CLI round trip: export → edit (rename, reject with note, relabel) → `import --dry-run` (3
  decisions) → apply → re-export → re-import plans 0; `linkml-lint` 0 errors on the result.
- `cargo test --workspace` (157 binaries), clippy `-D warnings`, `fmt --check`; API `pytest` (98
  passed, 36 skipped need `EKOS_BIN`) + `ruff`; UI `vitest` (65), `tsc`, `vite build`.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/import.rs` | New |
| `ekos/crates/cli/src/commands/export.rs`, `semantics.rs`, `mod.rs`, `app.rs` | ids, JSON, read-only reads, wiring |
| `ekos/crates/semantic/src/semantics_review.rs` | No-op re-confirm |
| `web/api/app/semantics_write.py`, `routes/semantics.py` | New |
| `web/api/app/readproc.py`, `main.py`, `routes/__init__.py` | Allowlist, text reads, SPA fallback, router |
| `web/api/tests/test_semantics.py` | New |
| `web/ui/src/pages/Semantics.tsx`, `SemanticsItemPanel.tsx`, `Linkml.tsx`, `semantics-shared.ts`, `Semantics.test.tsx` | New |
| `web/ui/src/main.tsx`, `WorkspaceShell.tsx`, `api/types.ts`, `index.css` | Route, tab, types, styles |
| `ekos/docs/rfcs/0170-business-semantics-linkml.md` | Phase 4 round trip + console |
| `README.md`, `web/README.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md`, `TODO.md` | Capability documented |
