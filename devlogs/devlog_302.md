# Devlog 302 — Step 5 as a reviewed decision: the Extraction page (RFC 0226)

**Date:** 2026-10-26
**PRs:** none (direct to `main`); ekos-lakehouse devlog_16
**Branch:** `main`

---

## Summary

Step 5 of the operator workflow now has a page and an approval. The lakehouse plan applies what step 3 decided —
scope, PII masks, write-behaviour overrides — but only from review subjects whose sign-off is valid, and saves the
result as a proposal. An operator changes tables with a reason and approves the plan on the console's Extraction page;
the approval is a ledger fact with the plan's digest, and `generate` under `require-plan-approval` builds only that
plan. In EKOS, extension page actions can now take typed text (`Action::inputs`), which a reason needed.

---

## RFC 0226 — the extraction plan as a reviewed decision

### Problem / motivation
`lakehouse generate` built whatever a fresh plan said, and nothing from step 3 reached it: an out-of-scope table was
still planned, a confirmed PII column was not masked unless someone also wrote it into `ekos.toml`, and a
write-behaviour override did not change the delete plan. Nobody agreed to the plan.

### What was built

| Component | Repo | Change |
|---|---|---|
| `bronze/src/extraction.rs` | lakehouse | `Change` (mode, delete cadence, purge policy), `TableChange` (who, when, why), `apply`/`apply_all`/`record`: the rules a change follows; a change that no longer fits is stale, not applied |
| `lakehouse-ext/src/extraction.rs` | lakehouse | `step3` (per subject, valid sign-off only), `Proposal`, `finalize` (changes + `ekos.toml` masks first), `digest`, `Approval`, `differences`, state files, step 5, the page's JSON |
| `lakehouse-ext/src/lib.rs` | lakehouse | `fresh()` (plan, `generate`, `sync` share it and save the proposal), `plan_gate`, `lakehouse extraction show|change|approve` |
| `facts.rs` | lakehouse | `record_plan_approval`: `LakehouseApproval`, one id per project, versions = approvals |
| `console.rs` | lakehouse | the Extraction page |
| `extension.rs`, `extension_pages.rs` | EKOS | `Action::inputs`; the checker accepts an input as giving a value argument, refuses a flag or a bound one |
| `ExtensionPage.tsx`, `extension-view.ts` | EKOS | text fields for an action's inputs; `withInputs` |
| `custom_kinds.rs` | EKOS | the `LakehouseApproval` row's note names the extraction plan |

### Implementation details worth remembering
- The digest covers what a build depends on (names, key, mode, delete plan, mappings, renames, table facts, refused
  and excluded tables, masks, settings) and **not** evidence text or notes, which carry row estimates. A test holds the
  digest equal over a 10 → 99,000 row change.
- `generate` re-plans from the source, applies the changes and compares digests. Undoing a change and redoing it
  returns to the approved digest, so the build goes ahead again; per-table digests in `approved.json` name the tables
  that moved.
- `sync` applies step 3 and the changes but is not gated: a source change is what sync exists to handle (RFC 0194's
  own rules and proposal branches).
- `plan` now also carries the deployed lock's renames, as `generate` did, or its digest would never match.

### Decisions
- **Step 3 per subject, only when signed off.** A stale scope sign-off applies nothing from scope rather than the
  decisions that still hold: half-applied review is worse than none, and the page says which subjects were ignored and why.
- **Changes in a state file, the approval in the ledger.** A change has no effect until approved; the approval carries
  the changes with their reasons, so the audit record is one fact.
- **Typed inputs as page data**, not a special form: the value goes through `build_argv` like a bound selection.
- **A change that changes nothing is refused** ("already in full"): the first browser run recorded a no-op change on a
  table already loaded in full, and the plan stayed approved, which read as a bug.

---

## Knowledge Captured

- **Playwright 1.29 (this machine's `python3`) does not resolve `get_by_label` for an `aria-label` input**; use
  `locator('[aria-label="…"]')`. A `<label>` wrapping a checkbox still works with `get_by_label`. The console also has a
  "Run" nav link and buttons whose names contain "Run": match `name="Run", exact=True`.
- A panel's text is not there when a table cell is: wait for the state text itself before reading the summary.
- On the testbed, scope proposes 51 tables `out` by class (`T_*`, `I_*`, `*LOG`); those proposals count as decisions
  once the subject is signed off.
- The console runs a review-class command with the signed-in person as `--as`: a change made in the browser records
  `static:write`, whatever the form said.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0226-extraction-plan-review-and-approval.md` | new RFC with results |
| `ekos/crates/cli/src/extension.rs`, `commands/extension_pages.rs` | `Action::inputs`, checker rules and test |
| `ekos/crates/kir/src/custom_kinds.rs` | `LakehouseApproval` note |
| `web/ui/src/pages/ExtensionPage.tsx`, `extension-view.ts`, `ExtensionPage.v3.test.tsx` | typed inputs |
| `README.md`, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | RFC 0226 |
