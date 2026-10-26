# Devlog 308 — The Oracle/iDempiere plan's exit report; a row-value scanner

**Date:** 2026-10-26
**PRs:** none (direct to `main`)
**Branch:** `main`

---

## Summary

The plan's last deliverable after RFC 0218: every release gate of Part VI.4, measured or cited, in
`evals/oracle-idempiere/exit/README.md`, BLOCKERs first. Two gates were measured for it: model calls per step, summed
from every run log of the project (0 wherever a log exists), and row values in the ledger, with a new example
`value_scan` that searches every current object and relationship for values sampled from the database. Result: 8
gates met, 2 partial, 2 BLOCKERS — 195 conflicts awaiting a person, and three delete scenarios not yet planted.

---

## What was built

| Component | Change |
|---|---|
| `crates/cli/examples/value_scan.rs` | values file → hits per value: kind, name, `origin`, JSON path, the matched string, and for a column its name and type |
| `evals/oracle-idempiere/exit/model_calls.py` | sums each run log's `llm` line by workflow step (terminal + console runs of one workspace) |
| `evals/oracle-idempiere/exit/README.md` | the gates, the evidence, the BLOCKERs |

## Decisions
- **An example, not a command.** The scan reads a whole ledger into memory (2.8 GB peak, 34 s on 163k objects) and
  needs values from the source; it is an audit an operator runs on purpose, like `evidence_locations`.
- **The values never enter the repository.** The report gives the columns and counts only.
- **Numeric and date bounds are reported as a design exception**, not hidden: RFC 0211's `p2` profile keeps them, and
  190 sampled document numbers coincide with id bounds. `p1` keeps none.

---

## Knowledge Captured

- **Most "hits" of sampled values are not leaks.** Of 430 hits, 235 were source files that mention the demo data and
  195 were database objects: numeric bounds, table names, comments, constraint and routine text, and digits inside
  ids and timestamps. Classify by `origin` and by property path before counting anything as a leak.
- **The fact segments are zstd frames with a trained dictionary** — neither `grep` nor the `zstd` CLI reads them; go
  through the store.
- **`graph export` carries no properties**; it cannot answer "is this value in the ledger".

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/examples/value_scan.rs` | new |
| `evals/oracle-idempiere/exit/` | report + `model_calls.py` |
| `TODO.md`, `CLAUDE.md` | exit items, latest devlog |
