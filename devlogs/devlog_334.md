# Devlog 334 — Gold marts on DuckDB (RFC 0259), keeping a lakehouse correct under change (RFC 0260), the DuckDB demo's chapter 9

**Date:** 2026-11-15
**PRs:** none — committed to `main` after local gates
**Branch:** main (EKOS); main (ekos-lakehouse); ekos-4.1.1 (ekos-superset); the demo repository
`ekos-lakehouse-duckdb` is local only

---

## Summary

- **RFC 0259** puts the gold marts on DuckDB.
  - EKOS knows DuckDB as a dialect (D1), a dbt-duckdb project (D2), a canonical form for the validator (D3), a
    read-only probe (D4) and a `[[databases]]` kind (D5).
  - ekos-lakehouse builds gold with dbt-duckdb straight from Iceberg silver and publishes one file.
  - The Superset fork guards DuckDB's file and network reads.
- **RFC 0260** covers what chapter 9 of the DuckDB demo needs once the lakehouse is running:
  - a source change's downstream impact, down to marts and dashboards (X1);
  - `change_measure`, with each measure's total before and after an edit (X6, X8);
  - a mart edit's gates run on the engine that builds the mart (X7);
  - `lakehouse spec`, a specification written from the project's records (X9).
- **The three change scenarios ran end to end** from a snapshot of the finished demo. They found eight gaps,
  each fixed (RFC 0260 §2.5).
  - The gap in EKOS core: the SQL check let a join wrapped in `DATE_TRUNC` through, along with
    `'Unknown' AS customer` standing in for a missing column.
- **Two fixes came out of the demo's live runs:**
  - migrate-validate's V2 ordered PostgreSQL text by the database collation;
  - code-vs-database conflicts treated the marts file as a second application database.

---

## RFC 0259 — gold marts on DuckDB

### Problem / motivation
Gold marts are small, read often and read by one BI tool. A Trino server is the wrong shape for that; DuckDB
serves a mart from one file and reads the same Iceberg silver through the same Polaris catalog. Nothing in
EKOS knew DuckDB.

### What was built

| Item | Where | What |
|---|---|---|
| D1 | `plugins/sql-dialect-duckdb` | `SqlDialectParser` registered as `duckdb`, with facts and generation notes |
| D2 | `recovery/src/dbt_analyzer.rs` | dbt-duckdb projects are read as DuckDB |
| D3 | `migrate-validate` | DuckDB's canonical form as a registered dialect |
| D4 | `ekos sql check --duckdb <file>` | read-only DuckDB probe (pinned DuckDB 1.5.6) |
| D5 | `[[databases]] kind = "duckdb"` | read-only catalog and profiles; the console's Sources form adds a DuckDB file |
| L1–L7 | ekos-lakehouse | `gold-engine = "duckdb"`: dbt-duckdb reads silver from Iceberg and writes the marts file; publish by rename; reconcile across engines |
| S1–S5 | ekos-superset | the guard's DuckDB rules: `read_*`, `ATTACH`, `INSTALL`, `SET` refused; the database opened read-only |

### Decisions
- The M0 spike decided the design: DuckDB attached the Polaris catalog and returned exactly Trino's answer
  through delete files, so **dbt-duckdb builds gold**. The fallback, Trino building gold for DuckDB to read, was
  not needed.

---

## RFC 0260 — keeping a lakehouse correct under change

### Problem / motivation
Chapter 9 of the demo plan (v2): the source changes (scenario A), new tables arrive with new logic (B), and a
person changes what a number means (C). Four pieces were missing (X1, X6/X8, X7, X9).

### What was built (ekos-lakehouse)

| Item | What |
|---|---|
| X1 | A proposal's impact lists each committed mart term that reads the changed column (`gold::impact::terms_reading`, whole-identifier match, `via` keys and filters included), and each recorded dashboard chart that reads that mart. A change no mart reads says so |
| X6 | `change_measure { name, definition }` is always modifying and must stay additive (sum / count). CLI `--change-measure <name> --definition '…'` |
| X7 | When gold runs on DuckDB, the plans and grain gates run in DuckDB over the compiled SQL with refs rewritten to the attached catalog (`gold::duckdb::engine_sql`), in an in-memory DuckDB with Iceberg attached read-only. Reconciliation against silver stays in Trino |
| X8 | Every edit's preview shows each measure's total before and after, e.g. `revenue: before 324286.35 → after 323863.20 (−423.15)` |
| X9 | `lakehouse spec [--out] [--json]` (class read) writes purpose, marts with their resolutions, sources, extraction, schedule in words, quality checks with dbt test counts, sign-offs and open items |

### The chapter 9 dry runs, and the eight fixes (§2.5)

| # | Found | Fix |
|---|---|---|
| 1 | EKOS's check passed a join wrapped in `DATE_TRUNC`, and `'Unknown' AS customer` in place of a missing column; `DATE_TRUNC('YEAR', '2026-01-01')` reached DuckDB | **EKOS `sqlgen`**: a side of `=` that is one column inside functions or casts is that column, for L002. New **L013** (warn): a constant under a column's name. New **L014** (error): `date_trunc` / `date_part` on quoted text. The fork blocks L013 like L002 |
| 2 | One table without a grant failed `lakehouse sync`, so in a scheduled run every table | Tables without `SELECT` (`has_table_privilege`) are refused in the plan with the reason |
| 3 | A stale scope sign-off planned all 902 tables | A stale scope keeps the deployed plan's tables |
| 4 | New tables entered the deployed plan at the next sync | Held outside a signed scope that never saw them; under `require-plan-approval`, sync leaves them to plan → approve → generate |
| 5 | `varchar(40)` → `varchar(60)` was silent | A source-only widening is a `widening`, applied and listed |
| 6 | Silver impact always empty | Read from the generated model YAML in the repository |
| 7 | Dagster ran the code location from before `generate` | `generate`, a committing sync and `lakehouse run` reload it when `HEAD` moved |
| 8 | Additive edits needed a second approval | An additive edit to an approved requirement carries the approval forward, recorded as the editor's approval with the edit in its note |

Four more from the same runs:

| Found | Fix |
|---|---|
| Two proposals merged back to back conflicted in `.generated.json` | The lock's hash is out of the manifest (always rewritten), and silver has one YAML per model, so proposals for two tables share no line |
| `generate --from-lock`, the installer's upgrade step, rendered silver **without step 3's masks** | It takes the masks the last plan read |
| X4: a stale sign-off gave only counts | **EKOS** `ekos review show <subject> --changed` lists the added and changed items and names the removed ones; console `?changed=true` |
| X5: only whole-plan approvals | `lakehouse extraction approve --table`; `generate` builds the approved tables, keeps the deployed version of a table with an unapproved change, leaves an unapproved new table out, and names each (`held:`) |

Each fix was checked against the restored snapshot. The demo's `RUNLOG.md` and `runs/2026-11-15-fixes/` hold the
output.

---

## Fixes

### V2 ordered PostgreSQL text by collation (a1d4ce0e)
V2 compares each column's minimum and maximum canonical text. PostgreSQL ordered that text by the database
collation and Trino by code point. On an `en_US.utf8` source, every table with amounts or free text diverged at
V2, while V1 and V3 agreed (`min(totallines::text)` is `'100.00'` under `en_US.utf8` and `'10.80'` in `"C"`).
PostgreSQL text is now ordered `COLLATE "C"`.

### The marts file as a second application database (bb2a21f2)
A project with a second live database (the DuckDB marts) made that database a claimant in every
`model_table_missing` conflict, re-opening decided ones, and turned its tables into `table_without_model`
conflicts. The application's databases are now those holding at least one model table. Claim paths name the
engine (`postgres://`, `duckdb://`) instead of always `oracle://`.

---

## Also since devlog 333
- Native installers (MSI, pkg, deb, rpm): `ekos setup` / `uninstall` leave the program to the OS installer
  (47059de3 and the MSI fixes). v1.5.5 released.
- RFC 0254: `null_not_handled` and `cast_will_fail`, a questionnaire export to .docx/.xlsx (e194d7c3).
- RFC 0252: relationship discovery proposes no second parent; on iDempiere 265 candidates (2 true) → 13 (the
  same 2) (95789aa2). The §8 measurements and a SQLite flavor were added (30a3c98d).

---

## Knowledge Captured
- **A join hides behind a function.** `analyze.rs` recorded a join only when both sides were plain columns, so
  `DATE_TRUNC('MONTH', a.m) = DATE_TRUNC('MONTH', b.m)` was no join and L002 never ran. Wrapping a column keeps
  every row that matched, so the join multiplies rows the same way.
- **A model repairs toward what passes.** Told that its join multiplied rows, the model dropped the joined
  table's column and wrote a constant under its name. A check that blocks one wrong shape needs to block the
  shape the repair reaches for (L013).
- **Same question, three passes, three outcomes:** wrong fan-out rows, refused (L002), then failed in DuckDB.
  An Ask answer is a sample: a guard must hold on every path, not on the one seen.
- **"No scope" is not a safe default after a scope existed.** A stale sign-off meant "ignore the subject", and for
  scope that meant every table. When a sign-off goes stale, keep what was last approved.
- **Dagster loads definitions when the code location loads**, not per run. A pipeline that regenerates its own
  definitions must reload the location, or the next run builds the old pipeline.
- **One unreadable table should cost one table.** A catalog read that measures data must check
  `has_table_privilege` first: a customization's new table arrives without the read-only role's grant (`ALTER
  DEFAULT PRIVILEGES` covers only tables created after it).
- **A hash manifest of generated files is a merge hotspot.** A file every branch rewrites (the lock) or
  one file shared by all tables of a schema (the silver YAML) changes the same manifest line on every branch.
  Keep per-table output in per-table files, and leave out of the manifest what is never kept anyway.
- **A re-render path must take every input the normal path does.** `generate --from-lock` skipped the masks
  that only the fresh-plan path set; the result was a silver layer without PII masks, committed.
- **DuckDB refuses `date_trunc(VARCHAR, VARCHAR)`**, as do PostgreSQL (`date_trunc(unknown, unknown)` is not
  unique) and Trino. SQLite keeps dates as text, so L014 names only `date_trunc` / `date_part`, which SQLite
  lacks.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/crates/sqlgen/src/analyze.rs` | joins through functions and casts; constants under a name; date functions on quoted text |
| `ekos/crates/sqlgen/src/lint.rs` | L013, L014; tests for L002 through a function |
| `ekos/docs/rfcs/0260-keeping-a-lakehouse-correct-under-change.md` | new: RFC 0260 with §2.5, the dry-run gaps and fixes |
| `ekos/plugins/sql-dialect-duckdb/`, `ekos/plugins/duckdb/` (catalog, guard, probe, session), `ekos/crates/migrate-validate/src/duckdb.rs`, `ekos/crates/recovery/src/dbt_analyzer.rs`, `web/api/app/routes/sources.py`, `web/ui/src/pages/Sources.tsx` | RFC 0259 D1–D5 (earlier commits, listed for the record) |
| `ekos/crates/migrate-validate/src/dialect.rs` | V2 text order `COLLATE "C"` (a1d4ce0e) |
| `ekos/crates/semantic/src/live_conflicts.rs` | application databases only; engine-named claim paths (bb2a21f2) |
| `ekos/crates/cli/src/app.rs`, `ekos/crates/cli/src/commands/review.rs` | `review show --changed`, `changed_since` (RFC 0260 X4) |
| `web/api/app/readproc.py`, `web/api/app/routes/review.py`, `web/api/tests/test_review.py` | `--changed` allowed from the console |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md` | L013 / L014, `review show --changed`, RFC 0260 |
