# RFC 0228 — EKOS Lakehouse: marts — propose, merge, build, reconcile (step 7)

**Status:** Accepted
**Date:** 2026-10-26
**Plan:** Oracle/iDempiere plan v3, Part II.7 (step 7)
**Repos:** ekos-lakehouse (`gold`, `lakehouse-ext`); EKOS docs only
**Depends on:** RFC 0195 (gold drafter), RFC 0196 (run facts), RFC 0215 (approved requirements), RFC 0227 (`lakehouse run`)

## 1. Problem

After step 4 a mart has an approved requirement, and RFC 0195's `gold draft` turns it into a dbt model on a proposal
branch with grain and reconciliation tests; merging the branch is the approval, and the Dagster job builds the
`gold` group. Everything around that happens outside EKOS:

- merging is a git command;
- a build runs the whole job;
- a reconciliation test says only pass or fail, never the two totals;
- nothing shows, per mart, where it stands, what it reads and which parts a model wrote.

The plan asks for a **Marts** page: approved specs, **Build**, and per mart its SQL, tests, measure reconciliation to
silver and lineage, marking what was generated exactly and what is for review.

## 2. Design

### 2.1 A mart's state

`lakehouse marts show [--mart <name>] [--json]` (class *read*) lists every mart that has a requirement (RFC 0215)
or a description in the pipeline repository's `marts/`. Its state is the first of these that holds:

| State | When |
|---|---|
| `requirement` | the requirement is not approved, or its approval is no longer valid |
| `to propose` | approved, and no proposal branch carries this requirement's digest |
| `proposed` | a branch `ekos/gold/<mart>/<id>` carries the approved description and is not merged |
| `merged` | the model `dbt/models/gold/gold__<mart>.sql` is on the repository's current branch, from the approved description; not built since |
| `failed` | the latest gold run (`gold.<mart>`, RFC 0196) failed: its message names the failed tests |
| `built` | the latest gold run passed; no reconciliation yet, or an older one |
| `reconciled` | built, and the latest reconciliation found every measure's mart total equal to silver's |
| `diverged` | built, and a measure's totals differ |

For the selected mart, the page shows:

- the SQL (from the current branch once merged, else from the proposal);
- each term with its **fidelity**: `exact` (compiled from a pattern; joins along declared keys) or `drafted` (prose a
  model wrote, which a person should read);
- the tests (grain uniqueness and one reconciliation per additive measure);
- the latest run and the latest reconciliation with both totals per measure;
- the lineage as a graph: the silver tables, the keys joining them, the mart.

### 2.2 Propose

`lakehouse marts propose --mart <name>` (class *external*: Trino plans the SQL; a prose term may call the local
model) runs `gold draft` on `marts/<mart>.md`. It refuses a mart whose requirement is not approved and valid, so a
proposal always carries an approved description. The result is RFC 0195's branch.

### 2.3 Merge

`lakehouse marts merge --mart <name> [--note]` (class *review*: merging is the approval, RFC 0190) merges the mart's
proposal branch into the repository's current branch (`git merge --no-ff`). It refuses when:

- the requirement is not approved and valid;
- the branch's description differs from the approved one;
- the merge would conflict (nothing is left half-merged).

It records a `LakehouseApproval` fact with the id `gold:<mart>`: who, when, the note, the branch, the merge commit and
the requirement's digest.

### 2.4 Build

`lakehouse marts build --mart <name> [--wait]` (class *external*) launches the Dagster job with only the mart's asset
selected. Its key is looked up from Dagster's asset list by the model name `gold__<mart>`. A mart merged since
Dagster loaded the code is not in that list yet: the build reloads the code location and looks again. dbt then builds the model
and runs its tests (the reconciliation tests read silver, which is already there). `lakehouse run` gains the same
`--asset <model>` option. The run's results reach the ledger through `run-report` as before.

### 2.5 Reconcile

`lakehouse marts reconcile --mart <name>` (class *external*) runs, on Trino, every additive measure's reconciliation
as two numbers: the mart's total from the built gold table, and the same aggregate over silver under the same joins
and filters (the SQL of RFC 0195's reconciliation test, with its totals returned instead of compared). Each measure
becomes a `LakehouseMetric` fact (`metric: mart_reconcile`, the mart, the measure, both totals, equal or not).
Nothing is read but aggregates.

### 2.6 The page and the bar

The Lakehouse extension's **Marts** page:

- the marts with their state;
- for the selected mart: the summary with its SQL, terms with fidelity, tests, reconciliation totals, and lineage;
- actions **Propose**, **Merge** (confirmation), **Build** and **Reconcile**.

Its description says that generated transformations are a template that may need tuning, and which terms were
drafted.

Step 7 on the workflow bar comes from these states: *todo* without an approved requirement; *done* when every mart
with an approved requirement is `reconciled`; *partial* otherwise, with the counts.

## 3. Not in this RFC

Incremental gold models, editing a generated mart in the console, marts not described by a requirement (Pentaho
translations keep their own page), and narrowing the extraction to what the marts read.

## 4. Phases and exit

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | `marts show`, `propose`, `merge`, `reconcile`; `--asset` on `run` and `marts build`; facts; step 7 | unit tests; on iDempiere, the approved marts proposed, merged, built through Dagster and reconciled with their totals |
| 2 | the Marts page | a browser run: a mart from approved to reconciled on the page |

## 5. Results (2026-10-26, iDempiere testbed)

**Phase 1.**
- Three approved requirements from step 4 (`sales_by_month`, `category_region_sales`, `orders_per_customer`) were
  proposed, merged, built through Dagster and reconciled from the CLI.
- Each build selected only the mart's asset (`["gold", "gold__<mart>"]`) and took about 50 s.
- Every measure's mart total equals silver's:

| Mart | Measure | Mart | Silver |
|---|---|---|---|
| `sales_by_month` | `total` | 379.90 | 379.90 |
| `sales_by_month` | `orders` | 3 | 3 |
| `category_region_sales` | `sales` | 358.40 | 358.40 |
| `orders_per_customer` | `count_of_c_order_id` | 7 | 7 |

Step 7 reads "3 of 3 mart(s) reconciled", shown behind the step-3 block as for steps 4–6.

**Phase 2.** A fourth requirement (`invoices_by_month`) went from approved to reconciled with the page's actions only:
Propose, Merge (with its confirmation), Build, Reconcile. Its totals were 541.02 = 541.02 invoiced and 4 = 4 invoices.
The browser run passed 8/8 after the fixes below; step 7 reads "4 of 4".

Found while building it:

- **A file read through the `git` helper lost its trailing newline**, so a proposal's description never matched
  the approval's digest and every proposed mart showed "to propose". Files are read byte for byte now.
- **Dagster did not know a mart merged after it started.** Build reloads the code location when the asset is
  missing (about two minutes on iDempiere, with the dbt manifest parsed again).
- **Starting Dagster on the testbed launched the full job within seconds.** The generated schedules are on by
  default (right in production) and the hourly tick had passed. The run was cancelled before any step started, and
  both schedules were stopped on the testbed. Start Dagster for a project only after deciding whether its
  schedules should run.
- **From the console, Build returns once the run is launched.** The page shows `built` when the run ends (`run-report`
  writes the mart's latest build).
