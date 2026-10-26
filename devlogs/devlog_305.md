# Devlog 305 — Step 7: marts proposed, merged, built and reconciled (RFC 0228)

**Date:** 2026-10-26
**PRs:** none (direct to `main`); ekos-lakehouse devlog_19
**Branch:** `main`

---

## Summary

Step 7 of the operator workflow is built: the last step of the seven. `lakehouse marts propose|merge|build|reconcile|show`
and the console's **Marts** page take each approved requirement from a proposal branch, through a merge that records
the approval, a Dagster build of only that mart, to a reconciliation on Trino that shows each measure's total in the
mart and in silver. On iDempiere four marts went from approved to reconciled, every total equal, the fourth entirely
on the page.

---

## RFC 0228 — marts: propose, merge, build, reconcile

### Problem / motivation
RFC 0195 proposed gold models on branches. Merging was a git command, a build ran the whole job, and the
reconciliation tests said pass or fail without the totals. Nothing showed a mart's progress, its lineage, or which
parts a model wrote.

### What was built

| Component | Change |
|---|---|
| `gold::reconcile_sql` / `fidelity` / `lineage` | both totals per additive measure; `exact`/`drafted` per term; tables and keys → mart |
| `lakehouse-ext::marts` | the eight states from requirement to reconciled; byte-exact files from git; per-mart state files |
| `lakehouse marts show\|propose\|merge\|build\|reconcile` | classes read / external / review / external / external |
| `lakehouse run --asset` | one asset by name, with a code-location reload when it is missing |
| `run-report` | writes each gold mart's latest build for the page |
| facts | `LakehouseApproval` `gold:<mart>` (the merge), `LakehouseMetric` `mart_reconcile` per measure |
| workflow, console | step 7 from the marts; the Marts page (SQL beside the requirement, terms, tests, totals, lineage graph) |

### Decisions
- **Merging is the approval** (RFC 0190), now done by EKOS as a review-class command, so it is a person's and recorded.
- **Reconciliation returns numbers**, read through the read-only Trino reader, aggregates only.
- **The page reads state files**, not the ledger: the store has no read by kind, and a scan of iDempiere's ledger per
  page view would be slow. The ledger still records every run, merge and reconciliation.

---

## Knowledge Captured

- **`sync::git` trims its output**: fine for refs, wrong for file contents whose digest matters.
- **Generated Dagster schedules default to RUNNING**: starting `dagster dev` for a project whose hourly tick has
  passed launches the full job at once. On the testbed it was cancelled with `terminateRun(...,
  terminatePolicy: MARK_AS_CANCELED_IMMEDIATELY)` (a NOT_STARTED run cannot be terminated otherwise) and both schedules
  stopped with `stopRunningSchedule(id:)`.
- **Dagster's asset list is fixed at code-location load**: `reloadRepositoryLocation` then poll the repositories
  until the asset appears.
- dagster-dbt keys a gold model `["gold", "gold__<mart>"]` (the translator's group prefix); look assets up by their last
  path part.
- Console page checks must wait for the panel's content, not its title: each panel loads on its own.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0228-marts-propose-merge-build-reconcile.md` | new RFC with results |
| `README.md`, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html`, `docs/operator-workflow.md` | step 7 |
