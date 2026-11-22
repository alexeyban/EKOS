# Devlog 355 — RFC 0268's remaining items: second run, console, chained keyed replace, LLM fill-in

**Date:** 2026-11-22
**PRs:** none (direct to `main`); lakehouse devlog_31
**Branch:** `main`

---

## Summary

RFC 0268's remaining items are done or stated:
- **Second run:** compared. It found that fixture B's original jobs cannot load a change.
- **Console:** the Talend tab shows the verification.
- **Fixture E:** its chained delete-then-insert jobs are keyed replaces.
- **LLM fill-in:** gaps are drafted through the router, measured with two local models; nothing metered was called.
- **C–E in the sandbox:** stated as not verified by execution, since none ships a built job.

---

## RFC 0268, the remaining items (EKOS docs; lakehouse devlog_31)

### What was built

| Part | What |
|---|---|
| Second run | Plan edits make run 2's inputs on the host. The runner runs every job again on run 1's tables. `verify` builds each model's incremental run (table rebuilt, append, merge by key, snapshot `check` closing changed versions) in new `__r2` tables (no `MERGE` in Trino's memory catalog), and compares. |
| `job_errors` | A compared model carries its jobs' error lines from the run's manifest. |
| Console | `lakehouse talend report` attaches the latest verification; a "Verified by execution" panel. |
| Keyed replace | The join on the target anywhere in a chain; the delete with no filter accepted; target pass-throughs dropped. |
| LLM fill-in | Engine-neutral draft code in `etl-dbt`; Talend gaps offered and drafts applied in `talend-rel`; Talend's prompt in `talend-dbt::draft`; `talend translate --allow-metered`; drafts kept for `verify`. |

### Results

| | Result |
|---|---|
| Run 2, `DIM_CALENDRIER` | equal, 2,192 rows |
| Run 2, `ODS_CATEGORIE`, `ODS_TYPE_CLIENT` | differ: the jobs' closing `UPDATE` violates `NOT NULL` and rolls back, while the job exits 0 |
| Fixture E | 19 keyed replaces (10), 27 / 146 enabled (25 / 164), no gate disables |
| `glm-4.7-flash`, local | 2 of 5 gaps of fixture C drafted and passing the checks; a stateful `tJavaFlex` refused, correctly |
| `qwen2.5-coder:3b`, local | 0 of 7: it wrote Java names into SQL, and the checks refused them |

### Decisions

- **A difference caused by the original job is still a difference.** The verification says why, with the job's own
  error line, and does not hide it.
- **Drafting uses the router's `translate` route, as Pentaho's does.** Ollama runs without permission; anything else
  needs `--allow-metered`, which this work never used.
- **C–E are not built here.** Building them needs Talend Studio, and E reads Snowflake and Oracle, which the
  offline sandbox cannot reach. Both are recorded as the reasons.

---

## Knowledge Captured

- **Exit code 0 is not success for a Talend job.** A refused batch is logged and rolled back while the job ends
  normally. Read the error output, as the runner does.
- **PowerShell variable names ignore case.** A loop's `$run` overwrote the script's `$Run`.
- **Showing a small model the Java names of fields made it write them into SQL.** Say which spelling is SQL.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0268-talend-to-dbt.md` | second run and the remaining items, with results |
| `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | ticked; devlog pointer; the second run and drafting |
| ekos-lakehouse (devlog_31) | runner runs, verify run 2, console panel, keyed replace, drafting |
