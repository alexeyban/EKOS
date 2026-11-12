# Devlog 333 — RFC 0256 completed, deleted code and broken configs, live suites, codes over time, dashboard changes

**Date:** 2026-11-12
**PRs:** none — committed to `main` after local gates
**Branch:** main (EKOS); main (ekos-lakehouse); main (ekos-superset)

---

## Summary

- **RFC 0256** is complete.
  - Observed access is read on PostgreSQL, Oracle and ClickHouse; the other engines say they provide no usage
    statistics.
  - Roles link to Kettle jobs by connection user, and iDempiere's dictionary processes are actors on Oracle and
    PostgreSQL.
  - The console has a system-level graph.
  - The extraction-plan warning passed its exit in a browser.
- **Three long-standing defects are fixed:**
  - a broken `ekos.toml` silently ran on defaults;
  - deleted code kept its objects alive through every recover and compile;
  - two foreign keys between the same tables were merged into one.
- **Done in parallel workstreams, each in its own worktree and merged here:**
  - every RFC 0255 live suite was run;
  - RFC 0252 gained codes-over-time, monthly usage and usage evidence;
  - the dashboard bundle (RFC 0248) and changing dashboards (RFC 0258 phases 1, 2, 4, 5) were built in
    ekos-lakehouse and the Superset fork.

---

## RFC 0256 — the interaction map, completed

### What was built
| Component | What |
|---|---|
| Observed access | Oracle `V$SQL` (always partial), ClickHouse `system.query_log` (per `owners`); SQL Server / Informix recorded as "no usage statistics", stated on every surface |
| Since when | `first_observed`, `calls_since_previous` on each observed cell (open question 2) |
| Links | Kettle `<connection><username>` → `connection_user` on steps; `link_hypotheses_with` tries the connection first; `link_basis` shown on the role item |
| Kettle writes | `Update`/`InsertUpdate`/`Delete`/`SynchronizeAfterMerge` are sinks on their lookup table |
| Findings | `read_only_consumers` (no question); a trigger no longer explains the write that fires it |
| Processes | `AppProcess` from `AD_Process`/`AD_Scheduler` (Oracle, then PostgreSQL); actor kind `process` through its procedure or class |
| Console | Systems graph on the Interaction page; "no usage statistics" banner |
| Lakehouse | Oracle engine `usage_stats` → partial; warnings show "first seen" |

### Measured
- **PostgreSQL, ClickHouse, Oracle:** each live workload became exactly the observed cells.
- **Phase 4 exit:** passed in a browser, 17/17 (RFC 0256 §9 "Remaining tasks").

---

## Fixes

### Deleted code kept counting
- **Recover:** the file collectors picked a deleted file's last observation; `present_files` drops it.
- **Compile:**
  - It kept knowledge artifacts of passes that no longer run, and earlier runs of multi-input passes.
  - `PassManager` now records each pass's outputs (cached passes from the manifest).
  - `recover` writes `current-knowledge.json`, and `compile`/`resolve` read only that selection.
  - `resolve` used to read every artifact ever written (510 instead of 248).
- **Interaction map:** it read code from the append-only ledger; it now counts only code the latest compiled
  model holds.

### A broken `ekos.toml` ran on defaults
- The config was loaded before logging started, so the warning went nowhere: a missing `owners` dropped every
  database without a word.
- It is now an error naming the file; `config validate`, `doctor` and `init` still run.
- Unknown keys are reported (`serde_ignored`), on stderr and in `config validate`.

### Foreign keys between the same two tables
- `dedup_relationships` keyed by (from, to, kind). On iDempiere, 3,686 declared keys became 3,100; on BIRD
  `european_football_2`, 31 became 7.
- The key now includes the constraint or `fk_desc`.

---

## RFC 0255 — every live suite run
- **Infrastructure:** a private stack (PostgreSQL with Pagila/m3, Trino, Polaris, SeaweedFS) and the Oracle XE
  testbed.
- **Results:** trino 6/6, capabilities, bronze, changes, oracle_live 5/5, the fixture engine end to end 6/6, and
  migrate-validate conformance 5/5.
- **Fixes needed:** only test wiring (hard-coded ports; the Oracle delete test now needs a named scratch schema).

## RFC 0252 — leftovers
- **New codes:** `code_history`, and `new` for data-only values. On LedgerSMB both datable values came from the
  code first, as expected.
- **Per-month usage:** the CLI sparkline and the console chart. Every month equals a direct query.
- **Usage evidence:** SQL Server Query Store, Informix `sysmaster` and PostgreSQL statistics. EKOS's own sessions
  are left out (SQL Server: `DATEFIRST 5` as the tag).
- **Not done:** the Informix CSDK path needs an IBM account.

## RFC 0248 / 0258 — dashboards (ekos-lakehouse, ekos-superset)
- **The bundle (RFC 0248):** `dashboards/<key>.yaml`, written by the fork's new terms mode. EKOS compiles the
  SQL (`ekos_dashboard_compile`) and reconciles each chart with silver before anything is created.
- **Phase 1:** versions, revert, and copies on branches.
- **Phase 2:** term-level merge with conflicts; mart changes go through the gates.
- **Phase 4:** drift and republish.
- **Phase 5:** adoption.
- **Measured** on a synthetic testbed (RFC 0258 §9.2).
- **Not built:** the AI review and the build queue.

---

## Knowledge Captured
- **Builds and logging:** a config loaded before logging starts must report its errors itself; a
  `tracing::warn!` there is silent.
- **Append-only stores:** "newest per key" never removes anything. To forget a deleted file, record what the
  latest run produced and read only that.
- **Unknown fields:** `serde(deny_unknown_fields)` is on the top level and some sections only. `serde_ignored`
  finds the rest without breaking configs from other versions.
- **ClickHouse:**
  - `query_log` keeps no data for an insert sent as a data block (`INSERT INTO t VALUES` with no tuples).
  - `normalizeQuery`'s `?..` lists do not parse; sample the raw statement and drop it.
- **Oracle:**
  - `V$SQL` loses cursors the shared pool ages out, so an observation is partial by construction.
  - `DBA_TAB_MODIFICATIONS` counts since the last statistics gathering, a different window.
- **SQL Server:** Query Store keeps no application name and drops leading comments of auto-parameterised
  statements; a session setting (`DATEFIRST`) is the only reliable tag.
- **Host firewall:** it drops container → host traffic. A Superset container that must reach a host Ollama or MCP
  server needs host networking.
- **Disk:** `ekos/target` grew to 408 GB; `cargo clean` it before running several worktrees in parallel.

---

## Files Changed
| Area | Files |
|---|---|
| Recover output | `compiler-core/src/{current_knowledge,pass,cache,scheduler}.rs`, `semantic/src/lib.rs`, `cli/src/commands/{recover,compile,resolve}.rs` |
| Config | `compiler-core/src/config.rs`, `cli/src/app.rs`, `cli/src/home.rs`, `cli/src/commands/config.rs` |
| Interaction | `kir/src/interaction.rs`, `cli/src/commands/interaction.rs`, `recovery/src/pentaho_analyzer.rs`, `semantic/src/review.rs`, `web/ui/src/pages/Interaction.tsx` |
| Dictionary | `observation-sdk/src/app_dictionary.rs`, `ora-live`, `pg-live`, `plugins/{oracle,postgres}`, `recovery/src/oracle_catalog_analyzer.rs`, `identity/src/lib.rs`, `kir/src/custom_kinds.rs` |
| RFC 0252 | `observation-sdk/src/{usage,signals}.rs`, `semantic/src/{data_usage,business_semantics}.rs`, `cli/src/commands/{profile,semantics}.rs`, `plugins/{sqlserver,informix,postgres}`, `web/api/app/routes/profile.py`, `web/ui/src/pages/UsageOverTime.tsx` |
| Console | `web/api/app/{catalogue,sources_write}.py`, `web/ui/src/pages/Sources.tsx` |
| Docs | RFC 0248/0252/0255/0256/0258 logs, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` |
