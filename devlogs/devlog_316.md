# Devlog 316 — Azure Container Apps for real: a lease that ping-ponged, ignore patterns recover did not honour

**Date:** 2026-11-01
**PRs:** none — committed to `main` directly (76aba12, d43702f, 4008f79, ba5238e, 21230c3); private
repository `inbox0216/ekos-deploy` (Azure DevOps) for the deployment and its record
**Branch:** `main`

---

## Summary

RFC 0232's Azure module ran with `compute = "container"` on a real subscription, through the same
Azure Pipelines as the VM run (devlog_315): a Container App with git sync beside it, the home on
premium Azure Files NFS, the image from the private ACR by managed identity. HTTPS, sign-in, a project
configured from the browser, backup to Blob (8 s) and the compile all worked, and the run found
four EKOS bugs the offline checks could not: a provider rule Terraform only enforces at apply, file
names in `ignore-patterns` read anyway by recover, a 500 on a slow knowledge server, and — the
important one — **the instance lease livelocking during a revision rollout**. All four are fixed and
the lease fix was verified by a second rollout on the cloud.

---

## The lease ping-pong (21230c3)

### Problem
The lease's rule was *newest starter wins*: a new console takes the lease, the old one sees it at
its next heartbeat and shuts down. On Container Apps a rollout runs both revisions until the new one
is healthy, and **the platform restarts a container that exits**. The restarted old console was now
the newest starter: it took the lease back, the new console shut down and was restarted, and so on
every ~30 s (generations 3 → 7 in three minutes). Neither revision stayed up long enough to be healthy,
so the platform never retired the old one.

### What was built
The lease record gains `displaced`: the host the holder took the lease from while that host's
console was alive. `InstanceLease.take_over` first checks `displaced_here()`: if the live holder
displaced *this host*, the console logs *standing by until it stops* and polls until the holder is
gone (released or stale), then takes over. A console on a third host (the next rollout) or one
restarted on its own host (a VM) is not held back.

### Verified
A rollout between two `21230c3` revisions: one takeover (4 → 5), the old console stood down, was
restarted by the platform 14 s later and logged *took over from this host; standing by*; the new
revision became healthy and the old one was stopped (Log Analytics lines in the private repo's
`demo/container/out/08-rollout-lease.txt`).

### Decisions
- *Host*, not instance id: a restarted container gets a new pid and uuid but keeps its replica's host
  name, which is exactly what tells "the old one, restarted" from "a newer one".
- Standing by instead of exiting: an exit is what the platform answers with a restart; waiting keeps
  the failover (the holder's heartbeat going stale still hands over).
- The first rollout *onto* the fix still ping-pongs, because the old side runs the old rule; the old
  revision was deactivated by hand, and `docs/deployment.md` says so.

---

## `ignore-patterns` naming files (d43702f, 4008f79)

### Problem
Excluding Sakila's nine data scripts (`mysql-sakila-insert-data.sql`, …) from the console's
`ekos.toml` editor: the console warned each "matches nothing", the build did skip them (27 files
observed, not 36), but recover read all 34 SQL files and committed the same 142k objects. Recover's
direct walks (the SQL analyzers', dependency scan, crate topology, CI, dbt) used `is_ignored_dir`,
which skipped matching *directories* only; the observers (`ScanContext::is_ignored`) match any path
component, file names included. The lint called every `name.ext` a path or glob.

### What was built
`is_ignored_dir` skips any entry below the walk's root whose name is a pattern; the lint flags only
paths, globs and a bare extension (`.log`). A test pins that recover's walks skip a named file.
Result on the cloud: 25 SQL files, 2,983 objects, 5 min 51 s instead of 3 h 33 min — the same
tables, relationships and integrity numbers.

---

## Smaller fixes

| Commit | Fix |
|---|---|
| 76aba12 | Container Apps environment states `logs_destination = "log-analytics"`: azurerm 4.x refuses `log_analytics_workspace_id` without it, at apply only; a plan test asserts it |
| ba5238e | A `TimeoutError` nothing caught (an MCP call to a workspace's knowledge server) is a 504 with a message, not a 500 |

---

## Knowledge Captured
- **A platform that restarts exited containers turns "newest starter wins" into a livelock.** Any
  takeover protocol over shared storage must tell a restarted loser from a newer deployment; the
  replica's host name does it on Container Apps (and Kubernetes pods).
- **Provider apply-time checks are invisible to `terraform validate` and mocked `terraform test`**:
  `logs_destination` passed both. Only a real apply finds them.
- **`az acr build --no-logs --no-wait --query runId` prints nothing.** Look the run up with
  `az acr task list-runs --top 1`.
- **Ledger commit on Azure Files NFS costs ~90–105 ms per object**, about 4× a premium managed disk
  (3 h 31 min for 142k objects, 311 s for 2,983). Latency-bound; the batching TODO stands.
- **EKOS has two kinds of directory walk**: the observers' (`ScanContext`) and recover's own direct
  walks. A filtering rule must hold in both; this time only one honoured file names.
- **`az containerapp exec --command` needs a terminal**: run it under `script -qec "…" /dev/null` from
  a non-interactive shell.
- **Container Apps keeps console logs of a stopped revision only in Log Analytics**
  (`ContainerAppConsoleLogs_CL`); `az containerapp logs show` returns nothing for it.

---

## Files Changed
| File | Change summary |
|---|---|
| `web/api/app/cloud.py`, `web/api/tests/test_cloud.py` | lease `displaced`, `displaced_here`, standby in `take_over`; 2 tests |
| `web/api/app/main.py`, `web/api/tests/test_graph_unit.py` | 504 handler for `TimeoutError`; test |
| `ekos/crates/cli/src/commands/recover.rs` | direct walks skip ignored file names; test |
| `ekos/crates/cli/src/commands/config.rs` | lint no longer flags whole file names |
| `deploy/terraform/modules/azure/container.tf`, `tests/plan.tftest.hcl` | `logs_destination`; assertion |
| `docs/deployment.md`, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | the Container Apps run, NFS commit cost, lease rollout note |
| *(private)* `inbox0216/ekos-deploy` | image pipeline run-id fix, module/image refs, `demo/container/` record |
