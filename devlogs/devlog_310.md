# Devlog 310 — The end-to-end video demo: a new iDempiere project from analysis to reconciled marts (RFC 0231)

**Date:** 2026-10-27
**PRs:** none (direct to `main`); ekos-lakehouse devlog_22; demo repo `ekos-lakehouse-oracle` `video/`
**Branch:** `main`

---

## Summary

A narrated end-to-end video demo, recorded from one real run: a **new** project, `idemsales`, on the iDempiere/Oracle
testbed, taken through all seven workflow steps in the console, from analysis to three gold marts reconciled against
silver. Step 3 was done by a named demo operator, scoped to the 33-table order-to-invoice slice. The recording found
two product defects, both fixed during it. **RFC 0231** gives each lakehouse project its own silver and gold schemas.
lakehouse-docs' column lineage now matches Oracle's upper-case names. The demo repo gets `video/`: a 7:52 MP4 (a
release asset), 13 narrated scenes, about 60 console screenshots, source / code / ER / lineage diagrams drawn from
compiled facts, a recording runbook, the run log and a publishing kit.

---

## The demo run

| Step | Result on `idemsales` |
|---|---|
| 1 Analyse | Init & compile from the console, 49 min (commit 38.5 min, the machine swapping), **0 model calls**; census = compiled: 903 tables / 160 views / 62 routines; 675,032 ledger entries |
| 2 Documentation | 239 pages for the slice, published to the demo wiki under `Sales` |
| 3 Review | the demo operator: 816 tables out of scope in one command, 17 blocking `mandatory_but_nullable` conflicts settled as *both valid* with the measured count, 1 identity match, 6 of 6 subjects signed |
| 5 Extraction | plan for 33 tables, `C_BPARTNER` changed to a full load with a reason, approved |
| 4 Requirements | two marts from Markdown; one in plain words drafted by local llama3, 2 unresolved terms + a join-path question answered by the operator, all approved |
| 6 Bronze + silver | Dagster from the console: 36 steps in 5.1 min, 68 assets, 895 checks; 2 accepted-values warnings |
| 7 Marts | Propose → Merge → Build → Reconcile ×3: 379.90 / 3 orders, 541.02 / 4, 358.40, each equal to silver |

## RFC 0231 — each project's own silver and gold schemas

### Problem
The second project's silver landed in `silver_adempiere`, the first project's schema. Silver was always
`silver_<schema>` and gold always `gold`, whatever the namespace prefix, so the second project rebuilt 33 of the
first one's silver tables.

### What was built
A **layer tag**: the prefix without the default `bronze_` lead. Silver is `silver_<tag><schema>`, gold is `gold_<tag>`
(`bronze_sales_` → `silver_sales_adempiere`, `gold_sales`). The default prefix is unchanged (see lakehouse devlog_22).

### Decisions
- Old tables are not moved or dropped: an operator drops them. A generator that drops tables would be a silent
  destructive act.
- Asked the maintainer before fixing: the recording stopped at the defect.

## Column lineage on Oracle

lakehouse-docs keyed upstream columns with their case: dbt's SQL says `linenetamt` and Oracle's catalog says
`LINENETAMT`, so every gold column lost its upstream. The lookup is now case-insensitive and maps back to the
declared name.

## The video kit (demo repo `video/`)

| File | What |
|---|---|
| `tour.py` | Playwright phases driving the console as the operator (resumable; exact `[aria-label]` locators) |
| `diagrams.py` | Mermaid pages → PNG, aggregate graph export → Graphviz code map, compact ER from the deployed plan (fdp layout) |
| `lineage_diagram.py` | column lineage source → bronze → silver → gold per mart, from lakehouse-docs' own model |
| `dagster_schedules.py` | stop the generated schedules and cancel runs over GraphQL |
| `make_steps.py` | 13 scenes: shots + narration; diagrams fitted into 16:9 rather than stretched |
| `RUNBOOK.md`, `RUNLOG.md` | how to record or re-take each scene; what happened during this one |

---

## Knowledge Captured
- **Step 4 cannot come before step 5.** `requirements draft` resolves against the deployed plan, so the plan must be
  approved and generated first, yet the bar offers step 4 next. The demo ran 5 → generate → 4 → 6 → 7 (TODO).
- **The dashboard's growth chart times out on a large ledger:** `ekos ledger timeline` takes over the console's 20 s
  on 676k entries (TODO).
- **A demo's own compose file needs everything the testbed's had:** `EKOS_HOME`, and the project's source root
  mounted read-only (EKOS refuses a project whose sources are missing). Two failed launches.
- **Playwright's `get_by_label` hangs on a page with ~900 labelled rows** (the Review page): use exact
  `[aria-label="…"]` selectors.
- **`pkill -f <pattern>` kills its own shell** when the pattern is on the command line: use `pgrep '^python3 …'`.
- **The video skill scales every shot to 16:9:** a 5.7:1 lineage diagram came out squashed. Fit diagrams into a
  white 1920×1080 frame first. Graphviz `dot` spreads a 33-table ER over 4,000 px; `fdp` gives a readable near-square.
- **Read every number in the narration off the screen it plays over.** Rounded or unseen numbers ("675 thousand",
  "twenty one paths") were caught and replaced before rendering.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0231-per-project-silver-and-gold-schemas.md` | new RFC |
| `TODO.md` | RFC 0231 + video demo done; step-4 ordering, timeline timeout, two code-list warnings |
| `CLAUDE.md` | highest RFC 0231, latest devlog 310 |
