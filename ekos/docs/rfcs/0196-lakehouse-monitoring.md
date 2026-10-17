# RFC 0196 — EKOS Lakehouse M5: monitoring and self-serve

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-17
**Builds on:** RFC 0190 (§ Pipeline monitoring, M5), RFC 0191 (extension MCP tools), RFC 0192–0195
**Code:** `ekos-lakehouse`, crate `health` (pure) and `lakehouse-ext`

## Goal

"Is this table right, and if not, why" answered from one place — the project's ledger — for a
person (CLI), an on-call agent (MCP, read-only), an alerting channel (webhook) and an existing
metrics stack (OpenTelemetry). And a first-time user can go from install to monitored bronze and
silver on their own database using only the docs. Exit: that, executed.

## Design

### 1. Everything health needs is a ledger fact

Already: `LakehouseRun` (bronze loads), `LakehouseMetric` reconciliation (verdict, control),
`LakehouseChange`. Added: `sync` records each table's **schema state** (`active` / `pending` /
`paused`, with the proposal) as a `LakehouseMetric` under a fixed id per table, so the latest
version is the current state; and `run-report --dbt-results target/run_results.json` records
each silver model's last build and its tests as `LakehouseRun` (layer `silver`). The MCP tools
get only the ledger (RFC 0149's `call_mcp_tool`), so this is what makes them possible.

### 2. One state per table and layer, with the facts that put it there

Precedence: **schema pending** (a proposal is open; paused counts here) > **failing** (the last
load or build failed, or its reconciliation diverged) > **unverified** (the planted control
missed) > **late** (no successful load within `late-after-minutes`, default 120) > **healthy**.
Each state carries its evidence: run id and time, verdict and divergences, proposal branch. A
failing table names its probable cause: an open or recent change to it, else the divergence.

### 3. Surfaces

| Surface | What |
|---|---|
| `lakehouse status [--json]` | the table-by-layer grid |
| `lakehouse alert` | evaluates the rules (failure, late, divergence, missed control, schema change awaiting approval) and posts each **transition** once to a webhook (`alert-webhook-env` names the variable holding the URL — a URL with a token is a secret). Slack, Teams and PagerDuty accept the payload's `text`. |
| `lakehouse metrics` | pushes OpenTelemetry metrics (OTLP/HTTP JSON) to `otlp-endpoint`: state per table, freshness, rows written, run duration, divergences, control fired — so Prometheus/Grafana see them through a collector |
| MCP `ekos_pipeline_status`, `ekos_table_health` | read-only, through the ledger only; scoped (RFC 0191), so a restricted reader may call them (Lakehouse facts carry no source, so such a reader sees none) |
| Dagster | a `lakehouse/health` asset after silver runs `alert` and `metrics` |

### 4. Preflight

`lakehouse doctor` adds checks that would otherwise fail later and quietly: the source catalog
exposes every source column (the two connector settings, RFC 0192 §1), the source role cannot
write (RFC 0157's probe; a role that can is a warning), the Iceberg catalog answers, and the
gold model is reachable when `gold-model` is set.

### 5. Docs that are executed

`docs/quickstart.md`, `docs/runbook.md`, `docs/troubleshooting.md` in the repository. The
quickstart's command blocks are run verbatim by `deploy/e2e/quickstart.sh` against a fresh
database the quickstart itself creates — the exit check, and a test that keeps the docs true.

## Found while building it (2026-10-17)

- **`dagster-dbt` gives each invocation its own target directory**: results read from
  `dbt/target/` were an older run's — a false "healthy" for silver, found only when the
  executed quickstart started from an empty directory. Results come from the invocation's
  `target_path`, and a failure to record them fails the asset.
- **`prepare_if_dev` prepares the dbt manifest only under `dagster dev`**; the generated project
  prepares it on every load (EKOS regenerates the dbt project anyway).
- **The orchestrator needs `git`** (`sync` commits and branches); the error now names it.
- MCP tools receive only the ledger — the reason every health input is a ledger fact.

## Decided against, for now

**A console tab.** The web console has no extension mechanism (RFC 0191 left it out); adding one
to core is its own RFC. Grafana (through OpenTelemetry), the CLI and MCP cover monitoring.

## Out of scope

The agent pack (RFC 0190 puts it outside the core), PagerDuty's native event format.
