# RFC 0258 — Changing an existing dashboard, and whether a dashboard can be trusted

**Status:** Accepted (2026-11-11, on the maintainer's instruction to start work)
**Date:** 2026-11-11
**Depends on:** RFC 0248 (questions to dashboards), RFC 0228 (mart states), RFC 0196 (table health), RFC 0247
(annotations), RFC 0213 (sign-off), RFC 0257 (structured edits)
**Repos:** ekos-lakehouse (`lakehouse ask`, `health`, `lakehouse-ext`); EKOS (console page)

---

## 1. Motivation

RFC 0248 takes a question and publishes a dashboard. Two things a person does next are not covered.

**They want to change it.** "This one, but by month and without the internal customers." Today that is a new
question: the result is a second dashboard, the first one stays, and nothing connects them. A request that points
at an existing dashboard or chart and says what to change has no path.

**They want to know if it is right today.** Everything needed to answer exists, in four places: table health
(RFC 0196: failing, unverified, late, healthy), mart state (RFC 0228: built, reconciled, diverged), the chart
check at publication (RFC 0248 §2.4), and stale annotations (RFC 0247). None of it is on the dashboard. The
person looking at the numbers — often not the analyst who built them — sees a chart and no sign of whether last
night's load failed.

## 2. Goals and non-goals

**Goals**

1. A change request that names an existing dashboard or chart, produces a new **version** of it, and passes the
   gates a new dashboard passes.
2. One computed state per dashboard and chart, with the facts that put it there, and "data as of".
3. That state visible where the numbers are read: in the console and on the dashboard in Superset.
4. Adopting a Superset dashboard EKOS did not publish, as far as its queries can be resolved.

**Non-goals**

- Editing in Superset's own editor and syncing back. A dashboard EKOS publishes is changed through EKOS; a manual
  edit in Superset is detected and reported as drift, not merged.
- Styling beyond chart type, title, order, number format and colour scheme by name.
- Alerts on business values ("revenue dropped 20%"). Health here is about the data's state, not its content.
- Row-level security (open question 2 of RFC 0248).

## 3. Design

### 3.1 What a published dashboard keeps

RFC 0248 commits a YAML bundle per dashboard. For changes, each chart's bundle entry must also hold what it was
compiled from: the structured request (measures, dimensions, filters, period), the resolved terms with the rule
that applied, and the digests of the marts it reads. If RFC 0248's bundle does not hold all three today, phase 1
adds them. A chart without them cannot be changed by request, only replaced.

### 3.2 A change request

The user points at the target — picks it in the console, or pastes its Superset URL — and writes the change. The
model's output is a **delta** to the stored structured request (add or remove a dimension, a filter, a measure;
change the period, the chart type, a title), schema-checked, with no SQL, as in RFC 0248 §2.1. EKOS applies the
delta, resolves every term again, and compiles.

The change is classified by what it needs:

| Class | Example | Path |
|---|---|---|
| `presentation` | title, chart type, order, format | no query change; publish |
| `query` | another dimension, filter or period, all resolving in the marts the chart reads or other existing marts | recompile; chart reconciliation (RFC 0248 §2.4); publish |
| `mart_addition` | needs a new column or mart | RFC 0248 §2.3, additive |
| `mart_change` | needs an existing mart changed | RFC 0248 §2.3, modifying: gates, review, confirmation, daily limit |
| `impossible` / `unsafe` | | as RFC 0248 §2.2 |

Before anything is published the user sees the old and the new chart side by side on the same data, the SQL diff
and the term diff.

### 3.3 Versions, copies and branches

The bundle is already in git (RFC 0248 §2.4), and marts already use the model this needs: a proposal branch
`ekos/gold/<mart>/<id>`, and a merge that is the approval (RFC 0228). Dashboards get the same, with the words a
person knows from GitHub.

| GitHub word | Here |
|---|---|
| commit, history | every publication is a commit of the bundle; a **version** is that commit. The dashboard page lists versions with author, request text and term diff |
| revert | `git revert` and republish, as for marts |
| branch | a change that is not yet the dashboard: `ekos/dash/<dashboard>/<id>`. It is published to Superset as a separate **copy**, titled with its origin and author, so it can be looked at with real data |
| fork | a branch made by someone who is not the author. **Someone else's dashboard is always changed as a copy, never in place** (owner's decision, 2026-11-11) |
| pull request | the copy's author asks for the change to be accepted into the original. The request shows the term diff, the SQL diff and both charts side by side |
| merge | the original's author, or a reviewer, accepts. The merge commit is a new version of the original and is recorded with who and when, as a mart merge is. The copy is then removed from Superset unless its author keeps it |
| diff | between any two versions or branches: terms first, SQL second |

Rules:

- The author may change their own dashboard in place (a new version on its main line) or on a branch, to try
  something without touching what others read.
- A branch is compared and merged at the level of **terms**, not YAML text. Two changes that touch different
  charts, or different terms of one chart, merge; two that change the same term are a conflict the merging
  person resolves by choosing one. Nothing is merged half-way.
- A merge is refused when the original moved on since the branch was made and the result no longer passes the
  checks of §3.2 (every term resolves, every chart reconciles). The branch is then recompiled on the new original
  first.
- A copy has its own health (§3.5) and is listed under its origin in the request feed.
- **Branches of data are narrower than branches of dashboards.** A `presentation` or `query` change costs
  nothing to branch: it reads the same marts. A `mart_addition` on a branch builds the new column or mart under a
  branch-suffixed name, removed when the branch is merged or closed. A `mart_change` is not built per branch: it
  changes a mart others read, so it stays one change through RFC 0248 §2.3 (gates, review, confirmation, daily
  limit, one build at a time), and the branch holds only the dashboard side until that change is published.
- A `mart_change` shows the other dashboards that read the mart (the impact RFC 0248 §2.3 already computes),
  including open branches.
- Abandoned copies: a branch with no change and no view for `[dashboards] branch-ttl-days` (default 30) is
  closed and its Superset copy removed; the commits stay.

### 3.4 Drift

At each health evaluation the published dashboard is read back from Superset and compared with its bundle. A
difference (a chart edited by hand, a dataset's SQL changed) is `drifted`: the dashboard keeps working, its state
says so, and a change request on it is refused until the drift is either adopted (§3.6) or overwritten by
republishing.

### 3.5 Dashboard health

Computed on read from ledger facts, never stored as a decision (the rule of RFC 0196 and 0213). A chart's state
is the first that holds, and a dashboard's is the worst of its charts':

| State | When |
|---|---|
| `failing` | a mart it reads is `failed` or `diverged` (RFC 0228), or a table on its chain is failing (RFC 0196) |
| `unverified` | its last chart reconciliation is missing or older than the mart's last build; or a table on its chain is `unverified` or has a schema change pending |
| `late` | a table on its chain is `late` (RFC 0196) |
| `drifted` | §3.4 |
| `needs review` | an instruction or synonym it was compiled with is `stale` (RFC 0247), or a mart's requirement approval is no longer valid |
| `trusted` | none of the above |

Each state carries its evidence — the table, the run, the divergence, the annotation — and **data as of**: the
earliest "last successful load" among the source tables on the chain.

Surfaces:

| Surface | What |
|---|---|
| `lakehouse dashboards status [--json]` (class *read*) | dashboards and charts with state, evidence, data as of |
| console | the state on the request feed and the review screen of RFC 0248 §2.5, linking to the chain |
| Superset | after every run the `lakehouse/health` asset (RFC 0196) updates a header block on each published dashboard: state, data as of, and a link to the chain in the console. A `trusted` dashboard is marked certified and any other state removes the mark, where the Superset version in use supports certification through its API (to be confirmed in phase 3) |
| alerts | `lakehouse alert` (RFC 0196) gains one rule: a dashboard leaves `trusted`; posted once per transition |
| MCP `ekos_dashboard_health` | read-only, scoped |

The header says what it is: the state of the data pipeline behind the numbers. It does not say the numbers answer
the business question; that is the sign-off of the requirement.

### 3.6 Adopting a dashboard EKOS did not publish

`lakehouse dashboards adopt --dashboard <id>` (class *external*) reads a Superset dashboard's charts and datasets.
For each chart it parses the dataset SQL and the chart's metrics and filters, and tries to resolve them to terms
over the gold marts:

- every atom resolves → the chart gets a bundle entry (§3.1) and can be changed by request;
- some do not → the chart is listed with the unresolved atoms, each as a question to the user;
- the dataset reads tables outside gold and silver → refused, naming them.

Adoption publishes nothing and changes nothing in Superset; it records what EKOS understood. Custom SQL beyond
what the `sql-select` front end parses is reported as not adoptable. This is expected to work for simple
dashboards over the marts and to be partial elsewhere, and the command reports the share.

## 4. Phases and exit

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | Bundle holds request, terms and digests; `presentation` and `query` changes; versions and revert; a non-author's change is a copy on a branch | on the demo question set: five written changes to published dashboards, each a new version, each chart reconciled; a revert restores the previous one; a second user's change leaves the original untouched |
| 2 | Pull request and merge at the level of terms; conflicts; `mart_addition` and `mart_change` through RFC 0248 §2.3; side-by-side preview | two branches changing different charts both merge; two changing the same term conflict and one is chosen; a change needing a mart change passes the gates and the confirmation, and the other affected dashboard is listed |
| 3 | Health: computation, CLI, console, Superset header, alert rule, MCP tool | a planted failed load turns the dashboards on its chain `failing` in Superset within one run, with the table named; recovery returns them to `trusted` |
| 4 | Drift detection | a chart edited by hand in Superset is reported `drifted` and refuses a change request |
| 5 | Adoption | a hand-built dashboard over two gold marts is adopted fully; one with custom SQL is reported with its unresolved atoms |

Phase 3 does not depend on phases 1–2 and can be built first.

## 5. Tests

- A delta that fails the schema is dropped; nothing is published.
- A `query` change whose chart does not reconcile with silver is not published (the RFC 0248 §2.4 rule).
- A non-author's change never alters the original without a merge by its author or a reviewer.
- A merge whose result fails a check of §3.2 is refused and leaves the original at its last version.
- A closed branch's Superset copy and branch-suffixed marts are removed; its commits remain.
- Health precedence: a dashboard both `late` and `failing` is `failing`, with both facts listed.
- "Data as of" equals the earliest last-successful-load on the chain, for a chain with two sources.
- A restricted reader sees no dashboard whose chain leaves their scope.

## 6. Open questions

1. Does the bundle of RFC 0248 already hold the structured request and the resolved terms per chart? If yes,
   phase 1 shrinks to the delta and versions.
2. The Superset header: a markdown component at the top of the dashboard is the simplest carrier and is editable
   by anyone who can edit the dashboard. Is that acceptable, given drift detection would report an edit?
3. Should `late` be judged per dashboard (a monthly report does not care about a two-hour delay)? A per-dashboard
   freshness expectation in the request would replace the single `late-after-minutes`.
4. *Decided (owner, 2026-11-11):* someone else's dashboard is changed as a copy. Versions and branches follow the
   GitHub model (§3.3).
5. Should the same words and the same page (versions, branches, pull request, merge) be given to marts, whose
   proposal branches and merges already exist under other names (RFC 0228)? One vocabulary for both would be
   simpler for an analyst; it is a console change, not a new mechanism.
6. Building a `mart_change` per branch would need a gold schema per branch (RFC 0231 has per-project schemas).
   Left out here for cost; worth revisiting if branches of marts are asked for.

## 9. Implementation log

### 9.1 Phase 3 — dashboard health (2026-11-11, ekos-lakehouse branch `rfc0258`, commit `c8e4d49`)

**What a "dashboard EKOS published" is today (gap to RFC 0248 §2.4).**
- RFC 0248's YAML bundle was never built; `lakehouse gold ask` only triages.
- Dashboards are published by the Superset fork's "Ask your data" page (`ekos-superset`,
  `superset/ekos_ai/service.py::_build`). For each chart it creates a virtual dataset named
  `ai_<6 hex>_<n>` holding the chart's SQL, a chart and a layout. Nothing is recorded outside Superset,
  and no marker says EKOS made it.

Phase 3 therefore adds the minimum health needs, read from Superset's REST API:

- **`lakehouse dashboards sync`** (class *external*, GET only):
  - reads `/api/v1/dashboard/`, `/dashboard/<id>/charts` and `/dashboard/<id>/datasets`;
  - selects the dashboards whose datasets are all the fork's `ai_…` datasets (`select = "all"` or
    `ids = [...]` widen the selection);
  - maps each chart to its dataset (`form_data.datasource`) and parses the dataset's SQL with
    `sqlparser` for the tables it reads (CTE names excluded; one-part names qualified with the
    dataset's schema; a physical dataset reads its own table);
  - records one `LakehouseMetric` fact with `metric: dashboard` per dashboard, under a fixed id, holding
    the charts and the tables they read. A dashboard gone from Superset gets a new version marked
    `removed`.
- **Mapping to marts at read time:**
  - `gold[_tag].m` becomes mart `gold.m`. The mart's silver tables come from its requirement approval's
    `needs` (RFC 0215), and each silver table leads to the bronze table of the same name.
  - `silver_[tag_]<schema>.t` and `bronze_[tag_]<schema>.t` resolve against the tables RFC 0196
    health knows.
  - Any other table (for example the source database itself) is "not a lakehouse table".
- **Bundle still needed:** the RFC 0248 bundle (§3.1: the request, the terms, the mart digests per
  chart) is still needed for phases 1, 2 and 4. When it exists, `sync` should read it instead of
  parsing SQL.

**Health**, in `ekos_lakehouse_health::dashboard`, pure and computed on read. Nothing is stored as a
decision.

| Chart state | Built from |
|---|---|
| `failing` | a table on the chain is failing (RFC 0196); or the mart's latest reconciliation for its latest build has a measure that differs (RFC 0228) |
| `unverified` | a chain table is `unverified` or `schema_pending`; or the mart was built and not reconciled since; or the mart's silver tables are not recorded; or a chain table has no fact; or the chart reads a table outside the lakehouse; or its SQL does not parse |
| `late` | a chain table is `late` |
| `drifted` | **not evaluated** (phase 4) |
| `needs review` | **not evaluated**: no bundle records a chart's terms; approval validity needs the pipeline repository, not the ledger |
| `trusted` | none of the above |

- **Precedence:** a chart takes the worst condition that holds. Every finding is listed, worst first,
  each naming its table and layer. A dashboard is the worst of its charts.
- **Data as of:** the earliest last successful load among the bronze tables on the chain. It is unknown
  if any of them has never loaded.
- **Stand-in for chart reconciliation:** the chart check of RFC 0248 §2.4 does not exist. On a chart
  that reads a mart, the mart's own reconciliation with silver (RFC 0228 §2.5) stands in for it.
- Every chart lists what was not evaluated (`not_evaluated`), so "trusted" is read for what it is.

**Surfaces.**
- **`lakehouse dashboards status [--json]`** (class *read*, ledger only), plus a "Dashboards" panel and
  a "Read dashboards from Superset" action on the Lakehouse console page.
- **`lakehouse dashboards publish-health [--dry-run]`** (class *external*):
  - off unless `superset-header = true` in `[extensions.lakehouse.dashboards]`;
  - adds or replaces a first-row markdown header (`ROW-ekos-health`): the state, data as of, the first
    three findings, a link to `console-url`, and the sentence saying it is the state of the pipeline
    and not a business sign-off;
  - sets `certified_by = "EKOS (pipeline health)"` when trusted, and removes only EKOS's own mark
    otherwise. **Decision, a narrowing of §3.5:** a certification by a person is never removed by
    EKOS; the command reports it.
  - A dashboard with no layout of its own gets no header (a layout holding only the header would hide
    its charts); only the certification is written.
  - Writes send Superset's CSRF token with its session cookie. Superset 4.1.1 accepts `certified_by`
    and `certification_details` on `PUT /api/v1/dashboard/<id>` (`DashboardPutSchema`).
  - Credentials are named, never stored: `superset-token-env`, or `superset-user` +
    `superset-password-env`.
- **`lakehouse alert`:** posts when a dashboard leaves `trusted`, or is first seen in another state,
  once per transition. Previous states are kept in `.lakehouse/dashboard-alerts.json`.
- **MCP `ekos_dashboard_health`:** read-only, in `mcp_tool_is_scoped`.
  - Lakehouse facts carry no source, so a source-restricted reader sees none.
  - Each record keeps `chain_known`, the chain tables that had facts when it was recorded. If one is
    missing on a later read, the reader's scope hid it, and the dashboard is not shown.

**Tests** (the RFC §5 rules that apply to phase 3):
- **Precedence** (late + failing → failing, both facts listed):
  `health::dashboard::tests::late_and_failing_is_failing_with_both_facts_listed`.
- **Data as of over two sources:** `trusted_with_data_as_of_the_earliest_of_two_sources`.
- **Restricted reader:** pure (`a_dashboard_whose_chain_leaves_the_readers_scope_is_not_shown`), and
  through a real `ScopedStore` and the MCP tool, with a source-restricted scope and with a scope that
  allows only the dashboard's own fact (`tests/dashboards.rs::a_restricted_reader_sees_no_dashboard`).
- **Planted failed load → failing with the table named, then recovery → trusted:** a pure test, and end
  to end against a local Superset stub
  (`a_failed_load_turns_the_dashboard_failing_in_superset_and_recovery_trusted`):
  - sync → trusted → certified with a header;
  - a failed bronze load of `public.orders` → `failing`, the alert fires once, the header names
    `public.orders` and the certification is removed;
  - recovery → `trusted` and certified again.
- **Also covered:** mart divergence → failing; a build not reconciled since → unverified; reference
  resolution (catalog, layer tag, Oracle upper case); a dataset's SQL read without CTE names;
  fork-dataset selection; a header added once and then replaced; a person's certification left in
  place.

**Not done in phase 3.**
- The `lakehouse/health` Dagster asset (generated in `bronze`) does not yet call `publish-health` after
  a run. RFC §4's "within one run" needs that one line in the generated asset.
- No real Superset was called: the only local Superset was someone else's container, and Azure was not
  touched.
- Console: only the extension panel. The request feed and review screen of RFC 0248 §2.5 do not exist.
- Open question 2 stands: the header is editable in Superset, and phase 4's drift check must ignore
  `ROW-ekos-health` / `MARKDOWN-ekos-health`.
- Phases 1, 2, 4 and 5 wait on RFC 0248's bundle and publishing.

### 9.2 Phases 1, 2, 4 and 5, and RFC 0248's bundle (2026-11-12, uncommitted worktrees)

Built in ekos-lakehouse (`gold::dashboard`, `lakehouse-ext::{dashboard_change, dashboard_adopt,
dashboard_cli, dashboard_tool}`), in the Superset fork (`superset/ekos_ai/{bundle,service,llm}.py`)
and, for identity, in the EKOS console (`web/api/app/catalogue.py`). RFC 0248 §2.4's bundle is
logged in RFC 0248 §7.

**The bundle (§3.1; open question 1 answered: no, it did not exist; it does now).**
`dashboards/<key>.yaml` in the pipeline repository, `format: 1`, read with unknown fields refused.
Per chart: the presentation (title, chart type, x, group by, metric, order, number format, colour
scheme by name), the structured request (measures, dimensions, filters, period — terms only), the
mart it reads and that mart's digest (`gold::mart_id` of its compiled SQL), the resolved terms with
the rule that applied and where the chart reads each (`sum of gold.sales_by_month.total`, `applied on
gold.sales_by_month.customer`, `built into gold.sales_by_month`), the SQL, the reconciliation, the
Superset ids and the form data as published. A copy also records its origin (dashboard and commit).

**Chart SQL (pure, `gold::dashboard`).**
- A chart is served by a mart whose dimensions and measures mean the same (compared on resolved terms
  and join keys, not text); a finer mart is totalled (`sum(...)`, only for sums and counts); a filter
  on a column a mart carries as a plain dimension is applied over it; every filter the mart has built
  in must be stated by the chart (a new chart inherits them, and the bundle says so).
- Among marts that serve, the order is: the chart's current mart, then one at the chart's own grain,
  then the coarsest, then by name.
- A period is `[from, to)` on a time dimension, each date the start of the unit, applied to the
  truncated value on both sides of the reconciliation.
- **Reconciliation (RFC 0248 §2.4):** per sum or count, the chart's total against the same terms
  compiled over silver (`gold::compile` + `engine_sql`, not through gold). A chart with neither has
  nothing to compare and says so. A difference blocks publication; nothing is written.

**Phase 1 — changes, versions, revert, copies.**
- A change is a delta, schema-checked (unknown fields, SQL characters, one quoted literal at the end of
  a filter only). `--text` asks a local model for the delta; the delta is applied to the stored terms
  before it is accepted and its error goes back to the model (three attempts). Model route: the
  existing `requirements` task (no new EKOS task).
- Classes: `presentation` (SQL kept), `query` (served by an existing mart), `mart_addition`,
  `mart_change` (only filters differ from the chart's mart: the filters a dimension cannot carry move
  into it), `needs_clarification`, `impossible`. Any blocked chart blocks the whole change.
- Versions: each publication commits only the bundle file, as the person (author and committer).
  `versions` lists the main line (`--first-parent`: a merged branch's own commits are not versions),
  with the term diff of each. `diff` is terms first, SQL second. `revert` is `git revert` of a version,
  the restored charts re-reconciled, the dashboard republished, one commit.
- **Someone else's dashboard is always changed as a copy:** a branch `ekos/dash/<key>/<id>`, committed
  with a temporary index (the checked-out tree is not touched), published as a separate Superset
  dashboard titled `<title> (copy by <who>, from <key>)`. The branch's author may change it further.
- `--preview`: old and new chart rows side by side on the same engine, the SQL diff, the term diff;
  nothing is published.
- An adopted chart whose dataset EKOS did not create gets a dataset of its own on its first query
  change; the hand-made dataset is never rewritten.

**Phase 2 — pull request, merge, conflicts, marts.**
- `request-merge` (the branch's author) records the request; `review` shows the change, what moved on
  the original since, and the conflicts.
- `merge` (the original's author or a reviewer from `[extensions.lakehouse.dashboards] reviewers`):
  a three-way merge of term maps (base = the origin commit). A term both sides changed differently is
  a conflict unless `--choose <term>=ours|theirs`; any open conflict merges nothing. The result is
  compiled over today's marts and every changed chart reconciled, or the merge is refused ("change the
  branch again on the new original first"). Then the original is republished, a merge commit is made
  (`-s ours` plus the merged bundle), the copy is removed unless `--keep-copy`, and the branch ref is
  kept under `refs/ekos/dash-merged/…` (`dash-closed/…` for `close`).
- `mart_addition` and `mart_change` go through RFC 0248 §2.3's hard gates:
  - before the build: every term resolves; 0 invented atoms (`gold::check`);
  - after a build into `<mart>__candidate`: the grain is unique (the generated dbt test); the mart
    reconciles with silver (`gold::reconcile_sql`); at most 1,000,000 rows. Only then is the real
    table replaced and the description committed.
  - A modifying change first shows the confirmation (impact: every other dashboard and open branch
    reading the mart; the consent text and its version; the daily limit) and stops. `--confirm`
    records the consent in `.ekos/lakehouse/dashboard-consents.json`; one modifying change per day.
  - An addition on a branch is built as `<mart>__b<id>` and its description committed only on the
    branch; the merge adds it for real on the main line through the same gates and drops the
    branch's table.
  - `mart-build = "trino"` builds on the engine (`drop` + `create table … as`: the sandbox's memory
    connector has no `create or replace`). The default, `dagster`, writes nothing and stops with
    "build it with Dagster" — **not exercised**.
  - **Not built:** RFC 0248 §2.3's AI review step; the per-mart build queue (builds run in the
    command, one at a time per process).

**Phase 4 — drift.** At each `sync`, `change`, `revert` and `merge`, the dashboard is read back: its
title, the charts on it (a chart added or removed by hand), each chart's name, every form-data key EKOS
published, and each dataset's SQL (whitespace-insensitive). The health header (phase 3) is not a chart
and the layout is not compared, so the header never counts as drift; republishing keeps it. A drifted
dashboard refuses a change; `republish` overwrites it from the bundle (a chart added by hand is taken
off the dashboard, not deleted). Health: drift is a `drifted` finding (chart or dashboard level),
below `failing`, `unverified` and `late`.

**Phase 5 — adoption.** `adopt --superset-id <id>` reads each chart: a physical dataset over a gold
mart, or one SELECT over one mart (plain columns, `sum`/`count` of a column, `AND`ed comparisons with
literals), and the chart's metrics, x axis, group by and simple filters. Every other construct is an
unresolved atom and becomes a question. A dataset reading a table outside gold and silver refuses the
adoption, naming it. The bundle records adopted charts (SQL and form data as they are in Superset, no
reconciliation: health says `unverified` until a change republishes them) and the unadopted ones with
their atoms. Nothing in Superset changes.

**Sync and health with a bundle (phase 3 follow-up).** `sync` first commits any bundle the fork wrote
and did not commit (as its author; an invalid one is reported, not committed), then reads every
bundle — the main line's and each open branch's copy (a copy has its own health) — instead of parsing
SQL: each chart reads its mart, the mart's silver tables come from the compiled mart (no requirement
approval needed), its own reconciliation replaces the mart stand-in (missing → `unverified`, a
difference → `failing`, the mart's digest changed since → `unverified`), and drift is recorded. Charts
adoption could not read are still judged by their SQL. `not_evaluated` for a chart with a bundle is
only "stale instructions or synonyms and requirement approval validity" — `needs review` is **still
not evaluated**.

**Console.** The Lakehouse page gains "Dashboard versions" and "Dashboard copies and pull requests"
panels and three actions: find drift, preview a change, change a dashboard (dashboard key + words).
The EKOS console now fills `by` (`--as`) with the signed-in person for every command class, not only
reviews: the person's identity decides between their own dashboard in place and someone else's as a
copy, so a form must not name someone else (`web/api/tests/test_cli_route.py`).

**Exits, measured on a local testbed** (`ekos-0258-trino`: Trino 483, memory catalog `iceberg`;
`ekos-0258-superset`: Superset 4.1.1 with the fork's `ekos_ai` mounted). Data: synthetic,
iDempiere-shaped silver tables (`random.Random(258)`: 600 orders, 1,529 lines, 354 invoices, 25
partners), the three gold marts of `ekos-lakehouse-oracle/pipeline-sales` built from that repository's
own generated dbt SQL, the pipeline repository a copy of it. Not the measured iDempiere run.

| Phase | Exit | Result |
|---|---|---|
| 1 | five written changes, each a new version, each chart reconciled | **met**: presentation (line, `,.2f`), count of invoices added, period 2025, split by customer, `customer <> 'Internal Sales Office'`; versions 2–6, every chart's total equal to silver's (e.g. 1,377,426.01 = 1,377,426.01). Three were interpreted from words by local `qwen2.5-coder:3b`; for two (split by customer, leave a customer out) it gave no usable delta in three attempts (`chart type "bar\|line\|…"`, a filter it wanted to remove that the chart did not have) — both refused by the schema check, nothing changed — and they were written as deltas by hand. `glm-4.7-flash` answered with no text (thinking only); `llama3` (8B) did not answer within 280 s on the 4 GB GPU. |
| 1 | a revert restores the previous one | **met**: the reverted version's terms and SQL equal the version before it; Superset in step (no drift) |
| 1 | a second user's change leaves the original untouched | **met**: bob's change → branch and copy dashboard 2; the original's Superset state byte-identical before and after, its bundle and main line unchanged |
| 2 | two branches changing different charts both merge | **met**: bob (chart 1 period, type) and carol (chart 2 type, format), both merged by alice, copies removed |
| 2 | two changing the same term conflict and one is chosen | **met**: dave `,.1f` / erin `,.3f` on chart 2's number format → one conflict, main unchanged; `--choose …=theirs` merged, erin's title change merged with it |
| 2 | a change needing a mart change passes the gates and the confirmation, and the other affected dashboard is listed | **met**: "USD only" on chart 1 → `mart_change` of `sales_by_month`; first run stopped at the confirmation listing the `customers` dashboard; with `--confirm`: 6 gates passed (mart 2,009,503.40 = silver), mart rebuilt, description committed, consent recorded; a second modifying change the same day refused. Also: an addition on the main line (`dash_a819b4cd`) and one on a branch (`dash_fab51a5e__bc155a6`, made real on merge, branch table dropped). After the mart change, the `customers` dashboard reports "its mart changed since the chart was reconciled". |
| 4 | a chart edited by hand is reported drifted and refuses a change request | **met**: three differences reported (name, `viz_type`, `y_axis_format`), the change refused, `republish` cleared it. Live health showed the drift findings but the state `unverified`, because the testbed has no pipeline run facts (no table is healthy); `drifted` as the state is shown by `health::dashboard::tests::a_bundle_chart_is_judged_on_its_own_reconciliation_and_its_drift`. |
| 5 | a hand-built dashboard over two gold marts is adopted fully; one with custom SQL is reported with its unresolved atoms | **met**: 3/3 charts over `sales_by_month` and `invoices_by_month` (a chart filter `customer != …` included); the custom-SQL dashboard 1/2, the other chart listed with 6 atoms (`sum(total) / nullif(sum(orders), 0)`, `upper(customer)`, `LIKE`, …); a dataset over `bronze_sales_adempiere.c_order` refused naming it. An adopted chart was then changed by request (period 2025, reconciled 1,110,439.71). |

The fork's terms path end to end: "Total sales revenue by month and the number of orders per
customer" → the local model named terms from the vocabulary → EKOS compiled both charts → both
reconciled (2,009,503.40; 305) → Superset dashboard 7 and `dashboards/dash-7.yaml` → `sync` committed
it as admin's version 1 → a change through `lakehouse dashboards change` published version 2.

**Tests.** `gold::dashboard_tests` (14: compilation, roll-up limits, classes, period, the schema, the
bundle round trip, merge and conflicts, diff, form data, mart choice), `lakehouse-ext`
`dashboard_change_tests` (9, in-memory Superset + scripted engine + real git: versions and revert, a
copy that leaves the original byte-identical, refusal on a bad delta and on a reconciliation
difference, merges and conflicts, a mart change with gates, confirmation, impact and the daily limit,
an addition on a branch, drift and republish, sync's bundle records, a fork-written bundle committed),
`dashboard_adopt` (3), `health::dashboard` (bundle reconciliation and drift); the fork's
`test_bundle.py` (6) and `test_llm.py` (+1).

**Not done / limits.**
- RFC 0248 §2.3 AI review; the Dagster build path for dashboard marts; the per-mart build queue.
- `needs review` (stale annotations, approval validity) is not evaluated.
- `branch-ttl-days` closes by the branch's last commit only; Superset view counts are not read.
- No real pipeline run facts on the testbed, so no live `trusted` → `drifted` transition.
- The side-by-side preview is rows and diffs in the command's output and the console action's log;
  the console has no dedicated two-chart page (RFC 0248 §2.5's review screen is still not built).
- A local 3B model turns about half of plain-language changes into a usable delta; the rest are
  refused by the schema check, never published wrong.
