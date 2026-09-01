# Devlog 149 — Three partitioned-store bugs found by the full-stack test run and fixed

**Date:** 2026-09-01
**PRs:** commit on branch `rfc/0118-compiled-knowledge-query-engine` → `main`
**Branch:** `rfc/0118-compiled-knowledge-query-engine` → `main`

---

## Summary

An autonomous full-stack test run (`EKOS_FULL_TEST_PLAN_v2.md`, artifacts under
`test-runs/run-20260901T160842Z/`) exercised RFC 0111/0113 (distributed storage), RFC 0118/0119–0126
(compiled-knowledge query engine) and RFC 0013/0115 (MCP) end to end against a partitioned
workspace. It surfaced 8 findings; the three MEDIUM ones are all partitioned-store correctness or
UX bugs, all fixed here:

- **F3** — partitioned/distributed retrieval silently lost cross-partition `ExactName` promotion:
  `ekos query find "Customers"` ranked `schemas/ecommerce.sql` above the `Customers` table.
- **F5** — `ekos mcp serve --workspace <dir>` did not load `<dir>/ekos.toml`, so an MCP server
  against a partitioned/distributed workspace hard-failed every tool call with a misleading
  time-bucket message.
- **F6** — `ekos status` / `ekos ledger status` printed "Ledger not initialised" on every
  partitioned workspace, while `query`/`ekl`/MCP all worked against it.

Nothing in Parts A/B/C was a BLOCKER; the safety-critical check (a write tool against the read-only
distributed gateway) already failed cleanly and explicitly.

---

## PR — F3 / F5 / F6

### F3 — cross-partition `ExactName` arm

**Problem.** `PartitionedLedger::retrieve` and `DistributedLedger` both run each partition's own
`FactLedger::retrieve` (which does BM25 + a within-partition `ExactName` promotion + RRF) and then
RRF-merge the per-partition ranked lists, labelling every list `SignalSource::Bm25`. A per-partition
exact-name promotion is real *inside* the Table partition — `Customers` is #0 there — but once
merged it only ties, at `1/(RRF_K+0)`, with `schemas/ecommerce.sql` (#0 of the File partition, whose
DDL is saturated with "customers"), and loses the deterministic `KirId` tiebreak
(`efe3d294… < f8dd66c6…`). Net: the exact RFC 0120 regression the `ExactName` arm exists to fix,
silently reintroduced by the partition boundary. Single-`FactLedger` workspaces were never affected.

**Fix.** `PartitionedLedger::retrieve` now also builds a union of every partition's candidates and
adds `exact_name_matches(&req.raw, &union)` as its own `(SignalSource::ExactName, …)` list to the
cross-partition `rrf_fuse` — mirroring what `DistributedLedger::search_ranked` **already did**
(the gateway had this; the local partitioned path never got it). So `Customers` now gets an
ExactName contribution *plus* its Bm25 contribution and wins.

**Verified.** `ekos query find "Customers"` on the partitioned store → `Customers` #1,
`schemas/ecommerce.sql` #2. New unit test
`retrieve_promotes_an_exact_name_match_across_partitions`.

### F5 — `ekos mcp serve --workspace <dir>` now loads `<dir>/ekos.toml`

**Problem.** `--config` defaulted to `./ekos.toml` independently of `--workspace`. Config resolution
only fell back to `<ws>/ekos.toml` when the workspace arrived via the `EKOS_WORKSPACE` env var, never
via the `--workspace` flag. Running `ekos mcp serve --workspace <partitioned-ws>` from any other cwd
opened the partitioned ledger with default routing config → *"partitions were created with
time-bucket = 'weekly', but it is being opened with 'monthly'"* on every call.

**Fix.** Extracted `resolve_config_path(explicit, env_config, is_mcp, mcp_workspace, env_workspace)`
in `bin/ekos.rs`. Precedence: `--config` → `EKOS_CONFIG` → (MCP only) `--workspace`/`EKOS_WORKSPACE`
`ekos.toml` → `./ekos.toml`. The `--workspace` flag is read out of the nested
`McpCommands::Serve { workspace }` before `cli.command` is consumed.

**Verified** three ways (flag / env / explicit-still-wins). New unit test
`config_path_resolution_precedence`.

### F6 — `ekos status` recognises a partitioned / distributed ledger

**Problem.** `ledger::status` branched on `store::uses_fact_engine`, which only checks for
`facts/manifest.json`. A partitioned store keeps its data under `partitioned/p/<kind>/…` with no
such file, so `uses_fact_engine` was `false` and `status` fell through to the SQLite path →
`config.ledger_path(cwd)` doesn't exist → "Ledger not initialised". MCP `ekos_status` was unaffected
because it goes through `open_store` (which handles all three backends).

**Fix.** A `config.storage.distributed.is_enabled() || store::uses_partitioned(...)` branch is now
**first** in `status`, using `open_store` + `store_display` and the `KnowledgeStore` trait's
`entry_count` / `object_count` / `relationship_count`. Prints
`Ledger: …/partitioned (partitioned, RFC 0111)` (or `(distributed cluster, RFC 0113)`).

**Verified.** `ekos status` and `ekos ledger status` on the run's partitioned workspace →
`Total entries: 1383, Objects: 30, Relationships: 1067`. New unit test
`status_reports_a_partitioned_ledger_instead_of_claiming_it_is_uninitialised`.

---

## Knowledge Captured

- **The federated retrieve merge is a place signal arms silently vanish.** `PartitionedLedger` /
  `DistributedLedger` re-label every per-partition/per-shard list as `Bm25` in the cross-partition
  RRF. Any signal that only ranks *within* a partition (ExactName today; a graph arm later) needs an
  explicit cross-partition arm over the candidate union, or it degrades to a rank-0 tie that loses
  the `KirId` tiebreak. The gateway had this arm; the local partitioned path was written later and
  didn't — the two federated paths must be kept in lockstep.
- **`store::uses_fact_engine` ≠ "has a store"** — it's specifically `facts/manifest.json`. Any
  command that branches on it (there were several: `status`, `repair`) misses the partitioned
  backend. `open_store` / `uses_partitioned` is the backend-agnostic check.
- **`--workspace` and `--config` were decoupled.** An agent host spawning `ekos mcp serve` from an
  arbitrary cwd must be able to pass just `--workspace`; the workspace's own `ekos.toml` has to
  follow it. The failure mode (a routing-config mismatch error) pointed nowhere near the real
  cause.
- **Autonomous end-to-end runs against a *partitioned* fixture find things the unit suite doesn't.**
  All three bugs are in code paths (`PartitionedLedger::retrieve`, CLI status, CLI config
  resolution) that had unit coverage for the *non*-partitioned or *happy* case only. `tests/integration`
  runs the default (non-partitioned) pipeline.

---

## Files Changed

| File | Change |
|---|---|
| `ekos/crates/ledger/src/partitioned/mod.rs` | F3 — cross-partition `ExactName` arm in `retrieve`; import `exact_name_matches` |
| `ekos/crates/ledger/src/partitioned/tests.rs` | F3 — `retrieve_promotes_an_exact_name_match_across_partitions` |
| `ekos/crates/cli/src/bin/ekos.rs` | F5 — `resolve_config_path` helper + `config_path_resolution_precedence` test |
| `ekos/crates/cli/src/commands/ledger.rs` | F6 — partitioned/distributed branch in `status` + regression test |
| `ekos/docs/rfcs/0120-rank-fusion.md` | Documented the cross-partition ExactName arm in the federated-overrides section |
| `test-runs/run-20260901T160842Z/{REPORT.md, metrics/findings.md}` | Marked F3/F5/F6 FIXED |
| `README.md`, `docs/generated/ekos-self-documentation.html` | `ekos status` now reports partitioned/distributed ledgers |
