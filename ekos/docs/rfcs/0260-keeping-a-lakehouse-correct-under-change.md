# RFC 0260 — Keeping a lakehouse correct under change

**Status:** accepted (2026-11-14) · **Target:** ekos-lakehouse 0.2.0, EKOS 1.6.0
**Depends on:** RFC 0194 (source changes), RFC 0228 (marts), RFC 0248 (change classes), RFC 0257 (mart edits),
RFC 0258 (dashboard health), RFC 0259 (gold on DuckDB)
**Plan:** `lakehouse-duckdb-demo-plan_v1md` (plan v2, 2026-11-14), items X1, X6, X7, X8, X9

## 1. Problem

The demo's chapter 9 shows a lakehouse that stays correct after day one: the source changes (scenario A), new
tables arrive with new logic (B), and the user changes what a number means (C). Four pieces are missing:

| # | What is missing today | Where it shows |
|---|---|---|
| X1 | A source-change proposal (RFC 0194) lists the **silver** columns and tests that read the changed column, and nothing further down. Which mart, which measure or dimension, which chart and which dashboard depend on it is not said | Scenario A4: a dropped column. Without the list, the proposal shows a paused table and nothing more |
| X6 | A measure can be added or dropped, not changed. "Revenue must exclude credit memos", or "revenue is the line amount, not the grand total", is two edits, two previews, no single diff | Scenario C2 |
| X7 | The engine gates of a mart edit (it plans, the grain stays unique) always run in Trino, also when gold runs on DuckDB, where the mart is actually built | Scenario C: "the edit is checked where it will run" |
| X8 | A modifying edit's preview has row counts per step and the edited mart's totals against silver, but not the measure's total **before and after** the edit | Scenario C2's approval screen |
| X9 | There is no document that states what was agreed: goals, marts in words, resolved terms, sources, load modes, schedule, checks, open questions, approvals with dates | Chapters 1 and 4: the user approves a specification |

## 2. Design

### 2.1 X1 — downstream impact of a source change

`sync::impact` keeps its silver lines and adds, for each change on `table.column` (or on the whole table for
`key_change` / `new_table`):

1. **Marts.** Every committed mart description (`marts/<mart>.md` at `HEAD`) is read with
   `ekos_lakehouse_gold::parse`. A term reads the column when its definition, a filter, or a `via` key names
   `table.column` as a whole identifier (case-insensitive; `c_invoice.dateinvoiced` does not match
   `c_invoice.dateinvoiced2`). The line names the mart, the section (dimension, measure, filter, join) and the
   term with its definition: ``gold `sales_by_category_month`: dimension `product_category` =
  m_product_category.name via … ``. The matching is pure (`gold::impact::terms_reading`), tested on its own.
2. **Dashboards.** The ledger's dashboard records (RFC 0258, `dashboards::read`) list each chart and the tables
   its dataset reads. A chart that reads an affected mart (`<gold schema>.<mart>`) gives a line:
   ``dashboard `CFO Dashboard` (1): chart `Revenue by Product Category Per Month` reads gold `sales_by_category_month` ``.
   Removed dashboards are skipped.

The lines go where the silver lines go: the proposal file (`proposals/<id>.md`, section "Impact") and the
`sync` output under the proposal. A change that no mart reads says so ("no gold mart reads it"), so the
absence is stated, not implied. Column-level chart use is not claimed: a dashboard chart is listed when it reads
the mart, because the dataset's SQL may read any of its columns.

Nothing here regenerates a mart. The decision (drop the dimension, or define it from another column) is the
person's, made with `lakehouse marts edit` or `requirements edit` and the usual approve → propose → merge →
build → reconcile; a mart description that names a column the plan no longer has does not resolve, so the
mart cannot be rebuilt silently on a missing column.

### 2.2 X6 + X8 — changing a measure, with the totals before and after

A ninth edit, `change_measure { name, definition }`, on an Aggregate step:

- `apply`: the measure must exist; its definition is replaced in place (its position and name are kept, so the
  mart's columns keep their order); an unchanged definition is refused ("nothing to change").
- `classify`: always **modifying** — the values of an existing column change.
- `constraint`: like `add_measure`, the new definition must resolve to an additive aggregate (`sum` / `count`,
  not distinct), because the reconciliation gate needs one.
- CLI: `lakehouse marts edit --mart M --step N --change-measure <name> --definition '<new>'`, or
  `--edit '{"edit":"change_measure",…}'`.

For **every** edit, the preview gains a `totals` block (X8): for each additive measure present on both sides,
its total over the mart **before** the edit and **after** it, both computed now by the engine the gates run on
(the mart's own SQL, never the stored table, so before and after see the same silver). A measure only on one
side is listed with that side alone. The text output prints them as
``revenue: before 308227.75 → after 301940.10 (−6287.65)``. They are shown, never a gate: a modifying change
is expected to move them; the person confirms with the numbers in front of them.

### 2.3 X7 — the engine gates on DuckDB

When the plan's gold engine is `duckdb`, the gates that run the edited mart run in **DuckDB**, the engine that
builds it:

- *plans*: `EXPLAIN` of the edited mart's SQL;
- *grain*: the duplicate-grain count;
- *totals*: each measure's total over the edited (and, for X8, the current) SQL.

The SQL is the compiled mart's dbt SQL with every silver `ref` replaced by the attached catalog's table
(`"lh"."silver_…"."<iceberg table>"`, `gold::duckdb::engine_sql`), the same names dbt-duckdb's sources
resolve to. It runs in an in-memory DuckDB through the same Python `duckdb` module the publish step uses, with
the Iceberg catalog attached **read-only** from the same settings and environment variables as the generated
`profiles.yml` (secrets only ever named; external access is needed to reach the catalog and is limited to the
`httpfs` / `iceberg` extensions already installed — autoinstall stays off). The reconciliation gate keeps
comparing the edited SQL with silver in Trino, both sides on one engine as in RFC 0257: it checks the mart's
logic against silver, and the built file is checked across engines again at publish (RFC 0259). The gate's
detail names the engine: "DuckDB plans the edited mart". On Trino gold nothing changes.

Step row counts (RFC 0257 §3.3) stay on Trino: they count silver steps, which live there.

### 2.4 X9 — the specification

`lakehouse spec [--out <file.md>] [--json]` writes one Markdown document from what the project already holds,
nothing typed for it:

1. **Purpose** — the marts' prose (each description's text after its header).
2. **Marts** — per mart: grain, dimensions, measures, filters as written, each term with its resolution
   (`gold::resolve_with`), open questions if any, and the requirement's approval (by, at, state).
3. **Sources** — the project's `[[sources]]` and `[[databases]]` (names, kinds; never a credential).
4. **Extraction** — the plan's tables with load mode, delete handling, masked columns, and the plan approval
   (by, at, digest, state).
5. **Schedule** — the `bronze-schedule` cron in words and what the scheduled job runs (sync → bronze →
   deletes/reconcile → silver/gold dbt → gold publish on DuckDB → health/alert).
6. **Quality checks** — the dbt tests per layer (counts by kind) and each mart measure's reconciliation rule.
7. **Review** — the sign-off subjects with their state (signed / stale), by and at.
8. **Open items** — every unanswered question and every stale approval.

Every date is an approval's own timestamp; nothing says "approved" unless an approval record does. The PDF is
the console's or a browser's print of the Markdown (the same renderer as the docs pages); no PDF library is
added. The document is a read command (catalogue class `read`).

### 2.5 What the chapter 9 dry runs found, and the fixes

The three scenarios ran end to end from the chapter 8 snapshot (demo `RUNLOG.md`, 2026-11-15). Eight gaps
came out of them; each is fixed here.

| # | Found | Fix |
|---|---|---|
| 1 | Ask: two marts joined on `DATE_TRUNC('MONTH', t1.month) = DATE_TRUNC('MONTH', t2.invoice_month)` passed EKOS's check (the analyzer saw no join), and `'Unknown' AS customer` stood in for a column the mart lacks; `DATE_TRUNC('YEAR', '2026-01-01')` reached DuckDB | `sqlgen`: a side of `=` that is one column inside functions or casts whose other arguments are constants is that column, so L002 sees the join; **L013** (warn): a constant returned under a column's name; **L014** (error): `date_trunc` / `date_part` given quoted text for the date. The Superset fork blocks L013 like L002 |
| 2 | One new table without a grant (`permission denied`) failed `lakehouse sync`, so in a scheduled run every table | `Source::unreadable`: PostgreSQL tables without `SELECT` (`has_table_privilege`) are left out of the read and refused in the plan with the reason; a refused table is no schema change |
| 3 | With the scope sign-off stale, `lakehouse plan` used no scope and planned all 902 tables | A stale or revoked scope sign-off keeps the deployed plan's tables and plans nothing else (without a deployed plan, the current decisions apply) |
| 4 | New tables went into the deployed plan at the next sync, before any review or plan approval | Under a signed scope, a table the review never saw and the deployed plan lacks is held ("not in step 3's signed scope"); under `require-plan-approval`, sync leaves new tables to plan → approve → `generate` (`waiting` in its output) |
| 5 | `varchar(40)` → `varchar(60)` in the source was neither applied nor reported (both land as Iceberg `varchar`) | A source-only widening (larger length or precision, limit removed, character type → `text`) is a `widening`, applied and listed; a source-only narrowing still changes nothing for bronze |
| 6 | The silver lines of the impact list were always "no compiled silver column reads it": they read a ledger compiled from the pipeline repository, which a project does not compile | The silver lines come from the generated model YAML in the repository (`meta.ekos.source`, the column, its dbt tests, relationships tests pointing at it); the ledger's view is added when present |
| 7 | After `generate`, Dagster still had the old code location: the new tables had no bronze assets and their silver models failed | `generate`, a sync that committed, and `lakehouse run` reload Dagster's code location when the repository's `HEAD` is not the commit last loaded (`.lakehouse/dagster-loaded`); a failure is a warning |
| 8 | Every mart edit, additive too, made the requirement need a second approval | An additive edit (gates passed) to an approved requirement with no open question carries the approval forward: recorded as an approval by the editor, its note naming the edit and the earlier approval. A modifying edit still needs approval |

Four more came out of the same runs and are fixed too:

| Found | Fix |
|---|---|
| Two proposals merged back to back conflicted in `.generated.json`: every proposal changed the hash of `lakehouse.lock.json` and of the schema's one silver YAML | The lock is not in the manifest (it is always rewritten, so its hash protects nothing); silver has one YAML per model, beside its SQL. Checked live: the two scenario A proposals merge one after the other |
| `generate --from-lock` (the installer's upgrade step) rendered silver without step 3's masks | It takes the masks the last plan read (`ekos.toml`'s, then step 3's) |
| X4: a stale sign-off stated its delta in counts, with no way to list it | `ekos review show <subject> --changed`: the items added or changed since the sign-off, the removed ones named; the console's `GET …/review/<subject>?changed=true` |
| X5: the approval was of the whole plan only | `lakehouse extraction approve --table <schema.table>` approves those tables on top of the last approval; `generate` builds the approved tables, keeps the deployed version of a table whose change is not approved and leaves an unapproved new table out, each named (`held:`). A change of the plan's settings still needs the whole plan |

Smaller differences from the plan, not changed: deletes leave full-load tables with the replacement (no
tombstones; a delete plan shows only on an incremental table); GardenWorld's lines have no discount, so the
scenario C additive measure is the quantity sold.

## 3. Tests

- X1: `terms_reading` on whole-identifier matching, `via` keys, filters, a whole-table change; `impact` with a
  temporary repository holding a mart description and a store holding a dashboard record → the mart and chart
  lines; a column no mart reads → "no gold mart reads it".
- X6: `apply` / `classify` / `constraint` for `change_measure` (unknown name, unchanged definition, non-additive
  definition refused; always modifying); the flag parser.
- X7: `gold::duckdb::engine_sql` against a plan (refs → attached tables; a gold ref kept); the gate verdict's
  engine name.
- X8: the totals block from before/after rows, a measure on one side only, the text line with the difference.
- §2.5: L002 through a function or cast, L013, L014 (`sqlgen`); the fork blocks L013; a source-only
  widening and narrowing (`changes`); an unreadable table refused once (`plan_cmd`); a table the signed
  scope never saw, a stale scope with and without a deployed plan (`extraction`); new tables held under plan
  approval, silver lines from model YAML (`sync`); the reload decision and answer (`run_cmd`); one YAML per
  model and no lock in the manifest (`silver`, `generate_files`); `changed_since` (`ekos review`); per-table
  approval and what `generate` builds under it (`extraction`).
- X9: `spec` rendering from a plan, two mart descriptions, approvals, a stale sign-off → every section; no
  secret from `[[databases]]` in the output.

## 4. Not in scope

Column-level lineage from a Superset chart's SQL to a mart column; regenerating a mart automatically after a
source change.
