# Devlog 301 — Step 5: the extraction plan from write behaviour, and deletes without a marker (RFC 0225)

**Date:** 2026-10-25
**PRs:** none (direct to `main`); ekos-lakehouse devlog_15
**Branch:** `main`

---

## Summary

The lakehouse now uses what EKOS knows about how an application writes. With `application-watermark = "UPDATED"`,
a table whose code always sets `Updated` (RFC 0212's update class) loads incrementally on it, and each such table
gets a delete plan from its delete classes: checked every run, or audited weekly where the code never hard-deletes.
Deletes are found by a **bucketed key diff** — 256 key buckets compared on the source and on bronze, keys read only in
buckets that differ — behind a **mass-delete guard** that applies nothing when more than 5 % of keys (or 10,000)
vanish. On iDempiere: 739 of 901 planned tables become incremental (685 every-run delete checks, 54 audits), and the
counts reconcile exactly with the ledger's classes. Live: single delete, delete + re-insert, a 148-key mass delete
stopped by the guard and applied once accepted, and a retained purge — each reconciled green afterwards. A bug that
would have written a home project's pipeline into its source checkout was fixed on the way.

---

## RFC 0225 — extraction plan and deletes

### Problem / motivation
Every Oracle table loaded in full (no trigger proves iDempiere's `Updated`), and the only delete detection was a full
key-set diff that applied whatever it found — a lost permission would have tombstoned a table.

### What was built (ekos-lakehouse)

| Component | Change |
|---|---|
| `bronze::WriteFacts`, `mode::choose_with`, `plan_with` | facts per table → application watermark (update class `po_only`/`raw_sql_sets_updated`, a key, the column) and `DeletePlan` (cadence, kind, purge policy, contradicts-code) |
| `bronze::render::tombstones_where`, `retains_purges` | tombstones by predicate; `purge` tombstones leave bronze's current state, silver keeps the rows |
| `lakehouse-ext::facts::write_facts` | the latest `WriteBehaviour` per `OWNER.TABLE`, read only when `application-watermark` is set |
| `lakehouse-ext::deletes` + `lakehouse deletes` | bucketed key diff, guard, tombstones, `LakehouseMetric {metric: deletes}`, `<state>/lakehouse/deletes.json` |
| `dagster.py` | runs `lakehouse deletes` after an incremental load with a delete plan; a tripped guard fails the asset |
| `settings::pipeline_base` | a home project's relative `pipeline-dir` resolves in the home, not in its sources |

### Decisions
- **Facts, not a trigger, prove an application watermark — and say what they cannot see**: an update outside the
  application. V3 reconciliation compares whole rows every run, so such a miss shows up the next run.
- **Buckets over the key only**, reusing RFC 0156's checksum queries: an unchanged table costs two aggregate queries.
- **The guard applies nothing and fails the run**; `--accept` is the person's decision (`lakehouse deletes` is an
  admin command in the console catalogue).
- **No new custom kind**: the check is a `LakehouseMetric`, so EKOS's kind registry needs no row.

---

## Knowledge Captured

- **`lakehouse generate` on an EKOS home project wrote `pipeline/` into the source checkout**: `pipeline-dir` was
  resolved against the workspace, which for a home project is the user's sources. The installer relies on that for its
  own projects (their workspace *is* the pipeline's parent, marked by `.lakehouse/project.json`), so the fix keys on
  that marker.
- All 12 iDempiere tables classed `purged` are also user- or application-deleted: a purge policy only applies to a
  purge-only table, so none of them is `purge` in practice.
- A tripped guard is visible in reconciliation: bronze keeps the vanished rows, so V1 diverges until the deletes are
  accepted — the pending state cannot be mistaken for green.
- EKL has no parentheses in `WHERE`; one query per value.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0225-lakehouse-extraction-plan-and-deletes.md` | new RFC with results |
| `README.md`, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | RFC 0225 |
