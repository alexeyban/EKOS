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
