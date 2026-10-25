# Devlog 296 — Workflow step bar and the sign-off gate (RFC 0213 Phase 2)

**Date:** 2026-10-25
**PRs:** none (direct to `main`)
**Branch:** `main` — 9a9ac8d (EKOS), 27ed7c0 (ekos-lakehouse)

---

## Summary

Phase 1 let a person sign the source model off; Phase 2 makes the rest of the workflow respect it. `ekos workflow
status` shows the seven operator steps with their states, steps 4–7 are `blocked` until every review subject is
validly signed, and `commands::workflow::require_signoff` is the gate an extension's step command calls before writing.
ekos-lakehouse uses it for `generate`, `sync` and `gold draft` (opt-in). Agents read the state through
`ekos_signoff_status`. The review summary, which reads the whole ledger, is now cached under a key every write moves:
24.5 s → 1.8 s on iDempiere.

---

## RFC 0213 Phase 2

### Problem / motivation
A sign-off nobody checks gates nothing. The console needs one structure for its step bar, extension commands need
one function to refuse with, and every Phase 1 command took 25 s / 2.5 GB, too slow for a page that polls.

### What was built

| Component | What it does |
|---|---|
| `semantic/src/workflow.rs` | `Step`, `StepState` (todo/partial/done/stale/blocked), `SubjectStatus`, `STEPS`, `review_step`, `assemble` (fills 2/4–7, step 1 todo blocks all, step 3 not done blocks 4–7), `gate` (names each unsigned/stale/revoked subject) |
| `EkosExtension::workflow_steps`, `WorkflowContext` | an extension reports steps 2 and 4–7; other numbers ignored |
| `commands/workflow.rs` | `ekos workflow status [--json]` (class read), `steps`, `require_signoff`, `require_signoff_in` |
| `commands/review.rs::subject_statuses` | cached summary, used by `review list`, `signoff show`, the bar and MCP |
| `ekos_signoff_status` | read-only MCP tool, `SCOPE_REFUSED_TOOLS` |
| ekos-lakehouse `workflow.rs` | steps from `LakehouseMart` / `schema_state` / latest `LakehouseRun` per table; `require-signoff` gate |

### Implementation details worth remembering
- **Cache key = everything the summary is computed from**: ledger `entry_count` (both backends count real writes
  only: the fact ledger pushes a batch only when the content signature changes, SQLite counts `entries`), object and
  relationship counts, current conflict ids, current semantics ids, `[review]`, and two versions. A memo, not stored
  validity; `approve` recomputes from the ledger, so a stale cache can never be signed.
- The extension's own step detail is kept after the block reason ("needs a valid sign-off …; plan and land the
  tables: `lakehouse sync`"), so the operator sees both what is missing and what comes next.
- Step 1 uses RFC 0171 freshness; with freshness unknown but a non-empty ledger it is `done` with the note.

### Decisions
- **The extension gate is opt-in** (`require-signoff`, default false): ekos-lakehouse's PostgreSQL users have never
  run a review, and a default-on gate would stop every `generate`.
- **All six subjects are required by default**, including an empty glossary: signing "there is no glossary" is a
  statement too. `require_signoff` takes a narrower list for a step that needs less.
- **`ekos_signoff_status` is refused under a read scope**: it summarises every table of the project.

---

## Knowledge Captured

- Freshness covers files only. After the Phase 1 Oracle column was dropped, step 1 stayed `fresh`: a live database is
  read every build and is outside the manifest (RFC 0211). Only a rebuild would update the ledger.
- `KnowledgeReader::entry_count` is the cheap "did anything change" key for any whole-ledger derived view; no
  timestamp or head id is exposed on the trait.
- ekos-lakehouse: `cargo clippy --all-targets` caught `field_reassign_with_default` in an example that plain
  `cargo clippy --workspace` never built.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/semantic/src/{workflow,review,lib}.rs` | step bar + gate (4 tests); `SignState: Deserialize` |
| `ekos/crates/cli/src/commands/{workflow,review,mcp,catalogue,mod}.rs`, `extension.rs`, `app.rs` | command, cache, MCP tool, hook (3 tests) |
| `deploy/e2e/package_console.py` | the chat may call `ekos_signoff_status` |
| `ekos/docs/rfcs/0213-review-and-sign-off.md` | Phase 2 results |
| `README.md`, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | RFC 0213 Phase 2 |
| ekos-lakehouse `crates/lakehouse-ext/src/{workflow,lib,settings}.rs`, README, devlog_13 | steps + gate |
