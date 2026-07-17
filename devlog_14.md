# Devlog 14 — MCP Server (RFC 0013) + first real multi-project scan

**Date:** 2026-07-16
**PRs:** —
**Branch:** main

---

## Summary

Implemented `ekos mcp serve` (RFC 0013) — a Model Context Protocol server over stdio that exposes
the read-only Runtime to AI agents — and connected the local Claude Code installation to it via
`claude mcp add` (user scope, verified `✔ Connected` by Claude Code's own MCP handshake). Then ran
the first real multi-project scan: a new EKOS workspace at the estate root (`$WORKSPACE_ROOT`) observing
44 project directories (~22K files, dozens of git repos). The scan exposed **three real bugs**,
all fixed with regression tests: duplicate pass names misreported as a dependency cycle
(`compiler-core`), a quadratic co-change blowup on bulk commits (6M `CoupledWith` relationships
from one workspace; `recovery`), and the semantic compiler caching on `{version, config}` alone so
it silently reused a stale CKM after recover re-runs (`semantic` + `cli`). Final pipeline:
22,239 files observed → 599 SQL files + 3,107 commits recovered → CKM with 25 objects /
5,136 relationships → ledger at 53,281 entries, verified end-to-end through the MCP tools.

---

## Part 1 — MCP server (`ekos mcp serve`, RFC 0013)

### Problem / motivation

RFC 0009 gave EKOS an AI answer path (`ekos ask`), but AI *agents* had no way to consume compiled
knowledge. MCP is the standard integration surface for agents (Claude Code, Claude Desktop), and
the Runtime's read-only query API maps 1:1 onto MCP tools.

### What was built

| Component | Description |
|---|---|
| `docs/rfcs/0013-mcp-server.md` | RFC (accepted): stdio transport, tool surface, invariants |
| `crates/cli/src/commands/mcp.rs` | Newline-delimited JSON-RPC 2.0 loop; `initialize`, `ping`, `tools/list`, `tools/call`; 8 unit tests |
| `ekos mcp serve --workspace DIR` | New subcommand; `--workspace` decouples the served `.ekos/` from the process cwd |
| `init_logging_stderr` (`commands/mod.rs`) | MCP server logs to stderr — stdout carries protocol frames only |

MCP tools → Runtime mapping: `ekos_search` → `find_objects`, `ekos_ekl` → EKL interpreter,
`ekos_neighborhood` → `load_neighborhood`, `ekos_state` → `reconstruct_state(_at)`,
`ekos_status` → ledger counts.

### Implementation details worth remembering

- **No new dependencies.** MCP stdio is just newline-delimited JSON-RPC 2.0; hand-rolled dispatch
  over `serde_json` beats pulling in an SDK crate for five tools.
- **Tool errors ≠ protocol errors.** Bad EKL syntax, unknown ids, and a missing ledger return
  `isError: true` inside a normal `tools/call` result, so the agent can read the message and
  adjust. Only malformed JSON-RPC (parse error, unknown method) becomes a JSON-RPC error.
- **The ledger opens per `tools/call`.** The server starts fine before the first `ekos build` and
  reports a readable "run `ekos build` first" tool error until a ledger exists.
- **`ledger_path` is cwd-derived**, but MCP clients launch servers from arbitrary directories —
  hence the `--workspace` flag. Anything long-running that a third party spawns needs its
  workspace passed explicitly, never inferred from cwd.
- **stdout discipline:** `init_logging` writes to stdout, which corrupts the protocol stream. The
  bin picks `init_logging_stderr` for `Commands::Mcp` before dispatch.

### Registration with Claude Code

```
# $WORKSPACE_ROOT = estate root (dir containing ekos.toml + .ekos/);
# the EKOS checkout lives at $WORKSPACE_ROOT/EKOS
claude mcp add --scope user ekos -- \
  "$WORKSPACE_ROOT/EKOS/ekos/target/release/ekos" \
  --config "$WORKSPACE_ROOT/ekos.toml" \
  mcp serve --workspace "$WORKSPACE_ROOT"
```

`claude mcp list` reports `✔ Connected` — Claude Code spawns the binary and completes a real MCP
initialize handshake as its health check, so that line is an end-to-end verification, not a config
echo.

---

## Part 2 — Scheduler bug: duplicate pass names misreported as a cycle

### Problem

`ekos recover` over the 44-project workspace failed instantly with
`scheduler error: dependency cycle detected involving pass ''` — in **both** sequential and
parallel modes.

### Root cause

`PassManager::execution_order` (and `execution_levels`) key the in-degree/adjacency maps by pass
*name*. Recover named SQL passes `sql-analyzer:{path relative to the observe base}`; with multiple
observe paths, two projects holding the same relative path (e.g. `schema.sql`) collide. The
`HashMap` silently collapses the duplicates, `order.len() != passes.len()`, and the code concluded
"cycle". The reported name was empty because every *unique* name did make it into the order — the
`find(|p| !order.contains(...))` found nothing and fell back to `String::default()`.

### Fix

| File | Change |
|---|---|
| `crates/compiler-core/src/pass.rs` | New `SchedulerError::DuplicatePassName`; `check_unique_names()` guard at the top of both `execution_order` and `execution_levels`; regression test |
| `crates/cli/src/commands/recover.rs` | SQL pass ids are now **workspace-relative** (`strip_prefix(cwd)`), not base-relative — unique across projects and better provenance in evidence |

### Worth remembering

- **Any map keyed by a user-influenced name needs a duplicate check before graph math.** The
  false "cycle" diagnosis cost more investigation time than a plain "duplicate name X" would have.
- This is the third real bug found by pointing EKOS at real data (after the identity false-merge
  and the `object_at` gap from devlog 12/13) — the "risk shifted from *will it get built* to *does
  it stay correct on real data*" pattern from devlog 13 keeps paying out.

---

## Part 3 — Scanning the estate root

### Workspace setup

- `ekos.toml` + `.ekos/` at the estate root, one level above all project
  checkouts (generated; one observe path per project directory).
- **Per-project observe paths, not `paths = ["."]`**, for two reasons: `GitObserver` only
  analyzes a scan root that *is* a git repo (no nested-repo discovery), and per-path fingerprints
  make rebuilds incremental per project.
- Ignore patterns extended with `.terraform` (≈4 GB of provider binaries — the single biggest
  scan-size win), `.venv`, `venv`, `__pycache__`, `.idea`, caches. Post-prune scan ≈5 GB.

### Pipeline results

| Stage | Result |
|---|---|
| `ekos build` | 22,239 files observed, 21,967 objects in ledger, 6m01s |
| `ekos recover` (1st full run) | 599 SQL files + 3,107 git commits, 600 passes, 20m50s |
| `ekos recover` (after coupling fix) | 1 pass re-run, **599 skipped as cached, 5s** — first real validation of the Phase 13 incremental cache |
| `ekos compile` | 25 objects, 5,136 relationships, 0.7s (was 6,016,182 relationships / 16m54s before the coupling fix) |
| `ekos commit` | +25 objects, +5,136 relationships, +3,154 evidence records, 48s |
| Final ledger | **53,281 entries, 21,992 objects** |

Verified end-to-end through the MCP server: `ekos_status` reports the counts above;
`FIND Object WHERE kind = 'Table'` returns real schemas (Northwind + project tables);
`ekos_search("orders")` finds files across projects; `FIND Object WHERE kind = 'Person'`
returns the workspace's git contributor.

### Worth remembering

- **UTF-16 SQL files** (`procfwk` project, SQL Server exports) fail `read_to_string` with "stream
  did not contain valid UTF-8" and are skipped with a warning — correct behavior, but a future
  encoding-sniffing pass could recover them.
- `recover --parallel` and sequential both go through the same name-keyed graph, so the duplicate
  bug hit both; "try the other mode" was a useful bisection step that ruled the parallel scheduler
  out.

---

## Part 4 — Coupling blowup: bulk commits made co-change analysis quadratic

The first full compile produced a CKM with 25 objects and **6,016,182 relationships** (a 1.6 GB
`model.json`, a 2.1 GB knowledge artifact, 7 GB RSS during analysis, and 12M validation warnings —
exactly 2 per relationship, both ends dangling in the CKM).

**Root cause:** `GitAnalyzerPass` counts co-change pairs for every commit unconditionally; the
pair count is quadratic in files-per-commit, and bulk commits (vendoring, formatting sweeps,
initial imports — one real import touched ~3,500 files ≈ 6M pairs) carry no coupling signal.

**Fix:** commits touching more than `DEFAULT_MAX_COUPLING_COMMIT_FILES = 50` files are excluded
from coupling analysis (they still produce their `KirEvent` and authorship relationship);
`with_max_coupling_commit_files()` for tuning; pass `version()` bumped to `"v2"` so the Phase 13
cache invalidates; regression test. This is the same threshold approach code-forensics tools
(e.g. CodeScene) use.

Result: 5,136 relationships, compile 16m54s → 0.7s.

---

## Part 5 — Semantic compiler cached on `{version, config}` alone

After the coupling fix, `ekos compile` **skipped itself as cached** and then failed on the
missing `model.json` — the pass declared no `cache_inputs()`, so the Phase 13 manifest never
noticed that the knowledge artifacts (its actual inputs) had changed. Any recover re-run followed
by compile would silently reuse a stale CKM.

**Fix:** `SemanticCompilerPass::with_cache_inputs()` — the compile command enumerates the
knowledge-artifact ids in the store and declares them (sorted) as the pass's cache inputs.
Regression test in `ekos-semantic`.

---

## Knowledge Captured

- **MCP stdio in five tools and zero dependencies:** newline-delimited JSON-RPC, echo the client's
  `protocolVersion`, never answer messages without an `id`, logs to stderr. `claude mcp list` is a
  genuine handshake test.
- **Pass names are load-bearing identifiers** — they key the scheduler graph, the Phase 13 cache
  manifest, and the execution report. Anything that generates pass names from file paths must
  guarantee uniqueness at the *workspace* level, not the scan-base level.
- **Every pass must declare its real inputs in `cache_inputs()`** or the Phase 13 cache will serve
  stale output. The semantic compiler's "inputs" are whatever is in the artifact store at run
  time — that dynamic-discovery pattern is exactly the shape that silently defeats input-based
  cache keys; the caller has to enumerate and declare.
- **Anything quadratic per-commit needs a bulk-commit guard.** Co-change coupling was correct on
  the fixtures and catastrophic on real history — thresholds like max-files-per-commit are not
  tuning, they're correctness on real data.
- **`.terraform` belongs in every default ignore list** alongside `target`/`node_modules` —
  provider binaries are routinely 200 MB–1 GB each and appear once per module directory.
- **Bumping a pass `version()` is the cheap, correct way to invalidate cached outputs after a
  behavior fix** — and it composes: the recover re-run after the coupling fix re-ran exactly the
  one changed pass (5s) while 599 SQL passes stayed cached.
- The scan found real, queryable knowledge immediately: cross-project SQL schemas, per-repo
  contributor sets (`FIND Object WHERE kind = 'Person'`), and file inventories — all reachable
  from Claude Code through the MCP tools.

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0013-mcp-server.md` | New RFC (accepted) |
| `ekos/crates/cli/src/commands/mcp.rs` | New MCP stdio server + 8 tests |
| `ekos/crates/cli/src/commands/mod.rs` | `mcp` module; `init_logging_stderr` |
| `ekos/crates/cli/src/bin/ekos.rs` | `Mcp { Serve }` subcommand; stderr logging for MCP |
| `ekos/crates/compiler-core/src/pass.rs` | `DuplicatePassName` error + guard + regression test |
| `ekos/crates/cli/src/commands/recover.rs` | Workspace-relative SQL pass ids |
| `ekos/crates/recovery/src/git_analyzer.rs` | Bulk-commit exclusion from coupling; pass v2; regression test |
| `ekos/crates/semantic/src/lib.rs` | `with_cache_inputs()` + `cache_inputs()` impl; regression test |
| `ekos/crates/cli/src/commands/compile.rs` | Declares knowledge-artifact ids as semantic pass cache inputs |
