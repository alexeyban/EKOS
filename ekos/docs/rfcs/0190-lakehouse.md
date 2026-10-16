# RFC 0190 — EKOS Lakehouse: PostgreSQL → Iceberg medallion pipelines

**Status:** Accepted (design); implementation in milestones M0–M5
**Author:** EKOS team
**Created:** 2026-10-15
**Related:** RFC 0154–0167 (Migrate), 0018 (impact), 0036 (dbt-gen), 0149 (extension seam), 0170 (LinkML semantics), 0171 (freshness), 0172 (conflicts), 0180–0182 (roles, read scope, MCP gateway), 0191 (extension seam v2)


## Summary

EKOS Lakehouse generates and maintains medallion pipelines from a PostgreSQL-like OLTP database into Apache Iceberg tables (Trino as engine, Polaris as REST catalog, MinIO as storage). Bronze and silver are generated from the compiled source; gold marts are generated from business stakeholders' written descriptions; every artifact is pipelines-as-code in a Git repo, and every decision is a ledger fact with evidence.

**Promise:** your lakehouse stays in step with your source — schema changes become reviewed pipeline changes within one run, every table carries quality metrics, and every gold number traces back to a source column and a business definition someone approved.

**What is automatic and what is gated:**

| Automatic | Proposed, then approved by a person |
| --- | --- |
| Bronze and silver generation for a new source | First deployment of any layer to production |
| Additive and safe-widening schema changes (new table, new nullable column, int → bigint) | Breaking changes: rename, drop, narrowing, type change, new NOT NULL |
| Quality checks, reconciliation and monitoring on every run | Every gold mart, and every change to one |
| Code-quality checks on every generated change | PII handling and masking rules |

"Automatic" means EKOS writes the change, tests it, and merges it only where policy marks the change class as safe. Everything else arrives as a pull request with impact analysis attached.

**Principle kept from Migrate:** the EKOS core computes and generates; it never moves data and never decides on its own. Trino moves the data; the orchestrator schedules it; people approve what is lossy, breaking or business-facing.

## Architecture

EKOS sits in the control plane: it reads the source catalog, generates pipeline code into a Git repo, and records every run and check. It never touches the data path.

```
 control plane                                   data plane
 ┌───────────────────────────────┐               ┌───────────────────────────────┐
 │ EKOS + Lakehouse extension     │  reads catalog│ PostgreSQL (source)           │
 │  · compile source + dbt project│◀──────────────│                               │
 │  · generate pipeline code      │               │ Trino ── postgresql connector │
 │  · record runs, checks, facts  │               │   │                           │
 └──────────────┬────────────────┘               │   └─ iceberg connector ──▶    │
                │ commits / pull requests        │      Polaris (REST catalog)   │
                ▼                                │      MinIO (object storage)   │
 ┌───────────────────────────────┐  schedules    │                               │
 │ Pipeline Git repo (user owns)  │──────────────▶│ Dagster runs SQL + dbt-trino  │
 └───────────────────────────────┘               └───────────────┬───────────────┘
                ▲                                                │ run-report
                └────────────── ekos lakehouse run-report ◀──────┘
```

The orchestrator and Trino do all data movement. Everything that changes a pipeline arrives as a commit or a pull request, so a team's existing review and CI apply unchanged. The agent pack, if used, sits beside the stakeholders: it reads through MCP and proposes, and cannot merge, approve or run.

## Reuse vs. build

About half the hard parts already exist in Migrate and the compiler; nothing for Iceberg, Trino or Polaris exists yet (a repo search finds only MinIO, for EKOS's own ledger storage).

| Capability | Exists (crate / RFC) | New work for Lakehouse |
| --- | --- | --- |
| Live source catalog, profiling, PII classification | `ekos-pg-live`, RFC 0157 | Run on a schedule, keep profile baselines over time |
| Schema-change detection | Drift to the column, live vs. repo DDL (devlog\_212) | Live vs. last deployed snapshot; change classification (§5) |
| Data-quality rules with affected rows | `ekos-migrate-dq`, RFC 0158 | Emit as dbt tests on silver; trend per run |
| Inferred keys from code joins and `pg_stat_statements` | RFC 0158, devlog\_214–216 | Feed silver keys, dedup and relationship tests |
| Cross-engine reconciliation with planted controls | `ekos-migrate-validate`, RFC 0155–0156 | Trino dialect for canonical hashing; per-run source ↔ bronze checks |
| Statement classifier, risk classes, evidence-pinned approvals | RFC 0160–0161 | Trino/Iceberg statements; approvals on pull requests |
| Business semantics, human-confirmed | LinkML export/import, RFC 0170 | Gold spec resolves terms against confirmed concepts |
| Impact analysis across code and SQL | `ekos_impact`, RFC 0018; dbt analyzer | Applied to the generated dbt project after every change |
| Cited, deterministic report | RFC 0162 | Per-pipeline quality report |
| dbt model rendering | `ekos-dbt-gen`, RFC 0036 (Pentaho IR only) | dbt-trino generator from source facts, not Pentaho IR |
| Trino SQL dialect | — | New, in `sql-dialect-sdk` |
| Iceberg / Polaris integration | — | Table DDL, schema evolution, maintenance jobs, catalog reads |
| Orchestrator project | — | Generated Dagster project (outside the core) |
| Run monitoring | — | Run facts, metrics export, alerts, console tab |

One reuse is worth calling out. EKOS already compiles dbt projects into the ledger. The generated project is compiled back after every change, so EKOS checks its own output with the same lineage and impact tools it uses on human code.

## Bronze, silver, gold

Bronze and silver are generated deterministically from source facts, with no model in the loop; gold is the only layer where a model drafts SQL, and it can only use what the ledger and confirmed semantics contain.

### Bronze — a faithful copy, per source table

- **Load path:** Trino reads PostgreSQL through its `postgresql` connector and writes Iceberg through Polaris. EKOS emits the SQL; it never moves rows itself.
- **Mode per table, chosen from profile facts:** full refresh for small tables; incremental by a watermark column where one is proven monotonic; key-set diff to detect hard deletes. The chosen mode and its evidence are recorded per table.
- **Shape:** source columns as typed by the mapping registry, plus `_ekos_loaded_at`, `_ekos_run_id`, `_ekos_source_lsn`. Append-only with snapshot history; nothing is cleaned here.
- **Iceberg maintenance is part of the pipeline:** snapshot expiry, small-file compaction, orphan-file removal, on a generated schedule.

### Silver — clean, conformed, tested

- Generated as dbt-trino models, one per bronze table, from facts EKOS already has:
  - keys from declared and measured-inferred FKs;
  - dedup to the latest version per key;
  - type fixes from approved transform dispositions;
  - PII columns masked or hashed per approved classification;
  - history as dbt snapshots where a table needs SCD2.
- Every DQ finding becomes a dbt test with the same threshold and evidence query. Every model gets docs from column comments and confirmed semantics.

### Gold — marts from a business description

1. A stakeholder writes a **mart description** in plain language — a Markdown file or a Confluence page — and fills a short structured header: grain, measures, dimensions, filters, refresh need.
2. EKOS **resolves every term** against confirmed LinkML concepts and silver columns. Unresolved terms become questions back to the author, never guesses.
3. A model drafts the SQL under the **anti-invention check**: every table, column, join and predicate must map to a resolved term or a silver object.
4. EKOS adds tests (grain uniqueness, measure reconciliation against silver totals) and opens a pull request carrying the term-resolution table.
5. The stakeholder and a data engineer approve. Only then does the mart deploy.

If the description says "active customer" and no confirmed concept defines it, step 2 stops and asks. That stop is the product working, not failing.

## Source schema changes

A change is detected before the next load, classified by what it can break downstream, and either applied automatically or turned into a pull request with its blast radius. Detection compares the live catalog with the last deployed snapshot at the start of every run; a PostgreSQL event trigger can shorten that to near-immediate, where the source owner allows one.

| Change in source | Default action | Why |
| --- | --- | --- |
| New table | Auto: bronze + silver generated, merged after tests pass | Nothing downstream depends on it yet |
| New nullable column | Auto: Iceberg `ADD COLUMN`, bronze and silver regenerated | Additive; Iceberg evolves the schema without a rewrite |
| Widening Iceberg allows (`int` → `bigint`, `float` → `double`, larger decimal precision) | Auto: promote the column type | Safe promotion in the Iceberg spec |
| New NOT NULL column, new default | PR: old rows need a backfill decision | The fill value is a business choice |
| Column rename | PR: proposed as rename, never as drop + add | Only a person can confirm it is the same data |
| Column or table drop | PR with impact: every silver and gold object and test that reads it | Breaks consumers; history stays in Iceberg snapshots |
| Narrowing or incompatible type change | PR, pipeline paused for that table | May lose data; needs a transform disposition |
| Primary-key or uniqueness change | PR, silver dedup paused | Changes the grain |
| Change in a view or routine bronze reads | PR with V5 differential result | Logic changed, not just shape |

- **Impact first:** every PR carries `ekos_impact` over the compiled dbt project — the exact silver models, gold marts, tests and dashboards-of-record that read the changed column.
- **Policy-tunable:** each row is a policy entry. A team can make renames auto-approved in dev and gated in production.
- **No silent gaps:** while a table's PR is open, its bronze load continues with the old schema where safe; silver and gold for that table show a *schema pending* status in monitoring.

## Data and code quality metrics

Every metric is computed on every run, stored as a ledger fact with its query, and shown as a trend; a metric without its evidence query is not reported. Thresholds start from the profile baseline and live in policy.

### Data quality — per table, per run

| Metric | Layer | How it is measured |
| --- | --- | --- |
| Reconciliation | source → bronze | Row counts, then bucketed canonical checksums (V1–V3) on a sample of buckets, full on schedule |
| Freshness | all | Source change time or LSN vs. latest bronze snapshot |
| Volume anomaly | bronze | Rows added vs. the table's recent distribution |
| Null-rate and distinct-count drift | bronze, silver | Against the stored profile baseline |
| Key uniqueness | silver | Duplicate keys after dedup (target 0) |
| Referential integrity | silver | Orphans per declared or inferred FK |
| DQ rule results | silver | The RFC 0158 rules, as dbt tests, with affected-row counts |
| Mart reconciliation | gold | Measure totals vs. silver at the mart's grain |
| Check health | all | Planted controls fire every run; a missed control marks the run *unverified* |

### Code quality — per change, per project

| Metric | How it is measured |
| --- | --- |
| Test coverage | Share of models with key, not-null and relationship tests; share of gold measures with a reconciliation test |
| Documentation coverage | Share of models and columns with descriptions traced to a source comment or confirmed concept |
| Lineage completeness | Every model resolves to source columns when EKOS compiles the dbt project back |
| Lint | `sqlfluff` (Trino dialect) and dbt project-evaluator rules: zero errors to merge |
| Anti-invention | Gold SQL: every reference maps to a resolved term or silver object (target 100%) |
| Review state | Share of gold marts and breaking changes with a recorded approval |
| Drift from generated | Hand edits to generated models, detected and listed |

The last row matters for self-serve teams. People will edit generated code; EKOS reports it and regenerates around it instead of overwriting it.

## Pipeline monitoring

The orchestrator runs the pipelines; EKOS records each run as facts and answers "is this table right, and if not, why" with the cause attached. Teams keep their own Grafana; EKOS feeds it rather than replacing it.

- **Run facts:** each orchestrator run reports back through one command (`ekos lakehouse run-report`) or a Dagster hook: asset, layer, status, duration, rows written, Iceberg snapshot id, checks passed and failed.
- **Status per table:** healthy, late, failing, *unverified* (a control missed), or *schema pending* (an open change PR). One state, with the facts that put it there.
- **Cause links:** a failing silver test links to the source change, run or data finding that explains it, through the same impact graph used for PRs.
- **Alerts:** webhook first (Slack, Teams, PagerDuty all accept one). Rules: failure, freshness breach, reconciliation divergence, missed control, schema change awaiting approval.
- **Metrics export:** OpenTelemetry metrics so existing Prometheus/Grafana stacks see freshness, duration, row counts and check results without a new tool.
- **Console tab:** a table-by-layer health grid, run history, quality trends, open change PRs, and the quality report per pipeline.
- **MCP read tools:** `ekos_pipeline_status` and `ekos_table_health`, so an on-call agent can investigate a failure but never rerun, merge or approve.

## Milestones

Six milestones; each ends with something a user can run on the sandbox stack, and the order puts the riskiest unknown — Trino as the load path — first.

### M0 — Stack and dialect

Exit: `docker compose up` brings the whole stack, and a planted one-row defect between PostgreSQL and an Iceberg table is caught by V3.

- [ ] `docker-compose.lakehouse.yml`: PostgreSQL, Trino, Polaris, MinIO, Dagster
- [ ] Trino dialect in `sql-dialect-sdk`; statement classifier extended to it
- [ ] Canonical value serialization in Trino SQL, golden fixtures, three-engine agreement (PG / Trino / Rust)
- [ ] **Measure** Trino `postgresql` connector throughput on a large table before committing to it as the load path

### M1 — Bronze

Exit: a Pagila source lands in bronze on schedule, with reconciliation green and controls firing.

- [ ] Load-mode choice from profile facts (full / watermark / key-set diff), recorded with evidence
- [ ] Iceberg DDL, type mapping registry PostgreSQL → Iceberg with lossiness classes
- [ ] Generated Dagster assets for bronze plus Iceberg maintenance jobs
- [ ] Run facts and the reconciliation report

### M2 — Silver

Exit: silver for Pagila and LedgerSMB passes all generated tests, lint and lineage checks.

- [ ] dbt-trino generator from source facts (keys, dedup, transforms, masking, snapshots)
- [ ] DQ rules emitted as dbt tests; docs from comments and semantics
- [ ] Generated project compiled back into the ledger; code-quality metrics

### M3 — Schema-change loop

Exit: every change class in §5 is exercised on the sandbox, and each produces its documented action.

- [ ] Live-vs-deployed snapshot diff and change classifier
- [ ] Auto-apply for safe classes; PR with impact for the rest; per-table pause
- [ ] Git provider integration (GitHub first) for PRs and approvals

### M4 — Gold from descriptions

Exit: three marts built from written descriptions on LedgerSMB, every term resolved or asked back, anti-invention at 100%.

- [ ] Mart description format and intake (Markdown, Confluence)
- [ ] Term resolution against confirmed LinkML concepts; questions for gaps
- [ ] Constrained SQL drafting, reconciliation tests, approval flow

### M5 — Monitoring and self-serve

Exit: a first-time user goes from install to monitored bronze/silver on their own database using only the docs.

- [ ] Health states, alerts, OpenTelemetry export, console Lakehouse tab
- [ ] MCP read tools; agent pack outside the core
- [ ] `ekos lakehouse doctor` preflight; quickstart, runbook, troubleshooting

## Release gates

v1 ships when every gate holds on Pagila and LedgerSMB on the sandbox stack, each enforced in CI.

| Gate | Bar |
| --- | --- |
| Source ↔ bronze reconciliation, unexplained divergences | 0 |
| Planted controls detected every run | 100% |
| Schema-change classes from §5 producing their documented action | All 9 |
| Breaking change merged without approval | 0 (tested adversarially) |
| Generated silver: key, not-null and relationship test coverage | 100% of models |
| Lint errors and unresolved lineage in generated code | 0 |
| Gold anti-invention | 100% of references resolved |
| Gold measures reconciling to silver | 100% |
| Row values or credentials in the ledger | 0 |
| Agent containment: merge, approve, rerun or production write via MCP | Refused at the tool layer, 100% |
| Time from source change to deployed safe change | Measure on the sandbox, then set the bar |
| Bronze load throughput | Measure in M0, then publish as measured |

## Where each part lives

Core EKOS gets the parts that *read and understand*, which Migrate and the analyzers use too. The Lakehouse repo gets the parts that *generate and operate*. Core never depends on the Lakehouse crates.

| In core EKOS (this repo) | In `ekos-lakehouse` |
|---|---|
| RFC 0191: extension commands, their catalogue classes, MCP read-scope decisions | `ekos lakehouse` commands, MCP tools, post-commit step |
| `Lakehouse*` rows in `ekos_kir::custom_kinds::REGISTRY` (as `Binary*`, RFC 0149) | the objects of those kinds |
| `plugins/sql-dialect-trino`; `dbt_analyzer` parses dbt-trino models | Trino client, `TrinoReader: EngineReader`, guarded executor |
| `Dialect::Trino` in `migrate-validate`, with golden and live agreement tests | Iceberg type map, DDL, schema evolution, maintenance |
| serde on `migrate::drift` shapes; `Target::Iceberg`, `EngineKind::Trino` | bronze/silver/Dagster generators, change classifier, gold, monitoring |

## Resolved questions (2026-10-16)

- **PostgreSQL-like sources in v1:** PostgreSQL only. Aurora and AlloyDB are added once a live test passes on each.
- **Event trigger at the source:** polling is the default; an event trigger is opt-in where the source owner allows one.
- **Mart description format:** Markdown with a YAML header; a console form may come later.
- **Who approves gold:** the stakeholder and a data engineer, both.
- **Migrate:** continues alongside. Shared crates change additively only, and Migrate's suites stay green.



## Decisions (defaults — each open to change)

| Decision | Default | Reason |
| --- | --- | --- |
| Load engine | Trino `postgresql` → `iceberg` connectors | Engine-native; EKOS stays out of the data path, as RFC 0154 requires |
| Change capture | Watermark + key-set diff in v1; CDC in v2 | No Kafka or Debezium in the first install |
| Transform code | dbt-trino | Readable, testable, already compiled by EKOS's dbt analyzer |
| Orchestrator | Dagster, generated project, outside the core | Asset model matches medallion layers; replaceable by Airflow later |
| Catalog | Polaris (Iceberg REST) | As specified; any REST catalog should work, tested only on Polaris |
| Where code lives | A Git repo the user owns; every change is a commit or a PR | Reviewable, revertible, works with their CI |
| Gold authorship | Model drafts, anti-invention gates, people approve | Business meaning is never inferred unconfirmed (RFC 0170 limits) |
| Product boundary | A separate public repo, `alexeyban/ekos-lakehouse` (MIT), plugged in through the extension seam (RFC 0149, widened by RFC 0191). Its binary `ekos-lakehouse` adds an `ekos lakehouse` command tree and contains all of core EKOS | Migrate is one-shot; Lakehouse is continuous. A separate repo keeps core small and lets Lakehouse release on its own schedule |

## Alternatives considered

- **Spark instead of Trino for loads.** Higher write throughput, but a second engine to install and tune. Kept as the fallback if M0 throughput fails.
- **Debezium CDC from day one.** Captures deletes and every update, but adds Kafka Connect to a self-serve install. Planned for v2.
- **EKOS as its own scheduler.** Rejected: competes with the orchestrators teams already run and breaks "computes, never decides".
- **LLM-generated silver.** Rejected: silver is fully derivable from source facts, so a model adds risk and no information.
- **Lakehouse inside the core workspace, next to Migrate.** Simpler wiring (no seam work), but every Lakehouse dependency and release would ride on core's. Declined 2026-10-16.
- **A private, open-core repo** like the binary decompiler (RFC 0149). The first draft of the plan; changed to public 2026-10-16.

## Risks

The biggest risk is scope: this is a new product on a new stack, not a finish of Migrate, and it competes with established ELT tools on the parts that are not evidence.

| Risk | Effect | Mitigation |
| --- | --- | --- |
| Trino's PostgreSQL connector reads a table through one connection | Slow bronze loads on large tables | Measure in M0; key-range splitting in generated SQL; Spark fallback |
| Watermark loads miss updates without a reliable change column, and miss deletes | Bronze silently diverges | Watermark only where monotonic is proven; key-set diff for deletes; reconciliation catches the rest |
| Iceberg MERGE and frequent commits create small files | Query slowdown, storage growth | Maintenance jobs generated with every pipeline |
| Polaris and Iceberg REST versions move fast | Breakage on upgrade | Pin versions in the compose file; a version matrix in the live CI job |
| Gold descriptions are vague | Endless question loops, or marts nobody trusts | Structured header required; unresolved terms block, and the gap list is the deliverable |
| Overlap with Airbyte, Fivetran, dlt, Debezium | "Why not just use X?" | Position on what they lack — schema-change impact, cited quality evidence, governed gold — and accept their ingestion where a team already runs it (v2) |
| Solo developer, two products in flight | Neither ships | Decide Migrate's status now (open question 5) |
