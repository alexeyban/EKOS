# Devlog 152 — RFC 0129: Web Console Phase 1 (shell + statistics)

**Date:** 2026-09-03
**Branch:** `feat/0129-web-console-phase-1` → `main` (local merge, `[skip ci]`, local gates only)
**RFC:** `ekos/docs/rfcs/0129-web-console-phase-1.md` (Accepted, merged `7d61b27`)

---

## Summary

RFC 0128 left a `web/` skeleton that only worked against a hand-started `ekos mcp serve --tcp`.
Phase 1 makes the console own its inputs and show the shape of a workspace: a persisted workspace
registry, a supervisor that spawns and restarts one MCP server per workspace, and a real dashboard
(counts, storage, objects-by-kind, growth timeline, query-log stats, `doctor`). Two small Rust
additions feed it — `ekos doctor --json` (R5) and `ekos ledger timeline --json` (R6) — plus a
latent logging bug fixed along the way: `ekos status --json` (RFC 0127 R2) had been interleaving
tantivy log lines into its own stdout since it shipped.

Locked decisions (from the maintainer): auth stays a single static `CONSOLE_TOKEN` until the first
browser write path (Phase 3); MCP-server supervision is its own module, separate from the Phase 3
job runner.

---

## PR — R5 / R6 (Rust)

### `ekos doctor --json` (R5)

`--json` on `ekos doctor` emits `{schema_version, ok, checks:[{name, status, detail}]}`, `ok` true
iff no check failed, always exits 0 (the `ok` field is the verdict — a machine consumer never
inspects the exit code). Text output byte-identical. The check-collection logic was extracted into
`collect_checks()` as the testable core; `build_doctor_json()` is a pure serializer.

### `ekos ledger timeline --json` (R6)

`ekos ledger timeline [--json] [--bucket day|week|month] [--since <rfc3339>]` — cumulative
object/relationship counts bucketed over time, for the dashboard's growth chart.

**The implementation is much simpler than the RFC sketched.** The RFC worried about walking
per-entry append timestamps and threading a new `KnowledgeStore` method through four backends. In
fact `KirObject` and `KirRelationship` already carry `created_at` (the analyzer's mint timestamp,
stamped in the same compile run that commits the object), reachable through the ordinary
`all_objects()` / `all_relationships()` calls. So `build_timeline` is: one pass each, sort by
`created_at`, bucket, emit cumulative running totals. Backend-agnostic, **no new trait method, no
per-backend branch, no `Err` case** for partitioned/distributed. `--since` trims which buckets are
shown; totals still reach back to the start of history. A separate `entries` series was dropped —
"entries" means version-rows on SQLite and batches on the fact engine, while objects/relationships
as-of-*t* is consistently defined everywhere.

### Logging fix

`emits_machine_output(&Commands)` classifies the subcommands whose stdout is a document a program
parses — `status --json`, `doctor --json`, `ekl --json`, `ledger status --json`, `ledger timeline`,
`graph export` — and routes their logs to stderr, the same treatment `ekos mcp serve` already got.
Before this, `ekos status --json` printed `INFO tantivy::directory::file_watcher ...` then the JSON,
all on stdout, so `json.loads()` on the console side failed with "Expecting value: line 1 column 1".
This was a real R2 (RFC 0127) bug, not just an R6 concern. Plain `ekos status` etc. are unchanged.

---

## PR — Phase 1 console (`web/api`)

| Module | Role |
|---|---|
| `models.py` | SQLite workspace registry via SQLModel — **one table** (`Workspace`). `Run`/`Schedule`/`User` come with their phases. `Session(expire_on_commit=False)` so detached rows stay readable after the `with` block. `EKOS_CONSOLE_WORKSPACES_JSON` is now a one-time **seed** for an empty registry, not the source of truth. |
| `supervisor.py` | `McpSupervisor`: one `ekos mcp serve --tcp 127.0.0.1:<port>` per workspace, port from an ephemeral base, a fresh `secrets.token_hex(32)` R4 token per process written `0600` under a per-run temp dir. Readiness = it answers `tools/list`. Crash → exponential backoff (1,2,4,…,cap 30s), 5 strikes then `failed`. `aclose()` SIGTERMs all children on lifespan teardown. `ClientPool` folded in — one `EkosMcpClient` per ready handle. |
| `readproc.py` | The read-only subprocess seam. Allowlist of **exactly three** argv shapes (`status --json`, `doctor --json`, `ledger timeline --json` + `--bucket`/`--since`). `create_subprocess_exec` with `cwd=<workspace root>` (these verbs have no `--workspace` flag — that's a `mcp serve` arg), never a shell, 1 MiB / 20 s caps. Explicitly **not** the Phase 3 job runner. |
| `_proc.py` | ~30 lines shared by the two above: `spawn(argv, cwd=)` + `terminate(proc, grace=)` (SIGTERM → SIGKILL). |
| `routes/workspaces.py` | `GET` / `POST` (validates the path has `ekos.toml` + `.ekos/`, then `supervisor.ensure`) / `DELETE` (`supervisor.stop` then drop the row). Each list entry carries live `server` status. |
| `routes/stats.py` | `/stats` (R2 via readproc), `/health` (R5 via readproc), `/stats/timeline` (R6 via readproc), `/stats/kinds` (`ekos_ekl "FIND Object COUNT GROUP BY kind"` over the running MCP server), `/stats/queries` (reads `<ws>/.ekos/query-log.jsonl`, RFC 0114, aggregates by tool + cache-hit rate + p50/p95). |

### Decisions worth remembering

- **`ASYNC240` disabled project-wide** (`ruff.toml`), with rationale: the console does tiny
  local-fs ops (a 64-byte token file, `Path.is_dir()`, reading the query log) inline in async
  handlers; a threadpool hop costs more than the sub-millisecond block. Same tradeoff as the sync
  SQLModel session. Phase 3's heavy I/O uses `create_subprocess_exec` and never blocks.
- **`readproc` runs with `cwd`, not `--workspace`.** `status` / `doctor` / `ledger timeline` take
  the workspace from the working directory. Only `mcp serve` has a `--workspace` flag. First live
  test caught this immediately (`error: unexpected argument '--workspace'`).

---

## PR — Phase 1 UI (`web/ui`)

`react-router-dom` (routes `/` and `/w/:id`) + `recharts`. `App.tsx` became `Layout.tsx` (header +
the token card, now collapsible) with `<Outlet/>`.

- **Workspaces page** — register form (id / name / path) + a list where each row shows a
  server-status chip (`ready` / `starting` / `failed (n retries)`) and a remove button. Polls
  every 4 s so a `starting` server visibly flips to `ready`.
- **Dashboard** — four stat tiles (entries / objects / relationships / evidence), a cumulative
  growth area chart (objects + relationships by day), objects-by-kind horizontal bar, a storage
  bar, query-log stats, and the `doctor` checklist with status dots.

`client.ts` gained `apiPost` / `apiDelete` and now unwraps FastAPI's `{detail}` error body.
`types.ts` is still a hand-stub — the `openapi-typescript` generation is wired (`npm run gen:api`)
but not run in CI (RFC 0129 §10 Q2). Bundle is ~640 KB / 188 KB gzipped (recharts); code-splitting
is a Phase 7 item.

---

## Verification

Local gates only (`[skip ci]`, per the maintainer's standing instruction — `feedback-local-tests-skip-ci`):

- Rust: `cargo fmt --check`, `clippy --workspace -D warnings` (exit 0), `test --workspace` (0
  failures), `tests/integration` 4/4. 10 new tests in `commands::{doctor,ledger}`.
- `web/api`: `ruff` clean, `pytest` 24/24 with `EKOS_BIN` set (12 new: readproc allowlist unit;
  supervisor spawn/restart/stop live; the five stats endpoints live against this repo).
- `web/ui`: `tsc -b --noEmit` clean, `vite build` succeeds.
- **End to end** against this repo: started uvicorn with only `EKOS_BIN` + `CONSOLE_TOKEN` + a
  seed. The console spawned its own MCP server on `127.0.0.1:7400`, `state: ready`.
  `/api/workspaces/self/stats` → real R2 (`evidence: 5045`, `last_write`),
  `/stats/kinds` → 16 kinds sorted desc, `/stats/timeline?bucket=month` → one cumulative point
  (5533 / 8364), `/stats/queries` → `{total: 19, by_tool, p50: 106ms, p95: 1183ms}`, `/health` →
  `ok: true`, 6 checks. Vite dev server proxies all of it.

---

## Knowledge Captured

- **`ekos status --json` was polluting its own stdout with log lines since RFC 0127.** Tantivy's
  file-watcher logs on a background thread through the default (stdout) subscriber. Any consumer
  doing `json.loads` on the output would have hit it. Fixed by classifying machine-output
  subcommands and giving them the stderr subscriber. If you add a new `--json` / bulk-export
  subcommand, add it to `emits_machine_output()` in `crates/cli/src/bin/ekos.rs`.
- **The growth-timeline primitive already existed.** `KirObject::created_at` (mint time) +
  `all_objects()` is all R6 needs. Don't reach for a new ledger read path for "X over time"
  questions — bucket the `created_at` you already get back.
- **`status` / `doctor` / `ledger *` take the workspace from `cwd`, not a flag.** Only
  `mcp serve` has `--workspace`. A subprocess wrapper must `chdir`.
- **SQLModel `Session` default `expire_on_commit=True` bites the "return the row" pattern.**
  After `s.commit()` the instance is expired *and* detached, so the caller's attribute access
  raises `DetachedInstanceError`. `expire_on_commit=False` is safe here because `Workspace` has
  no relationships — every column is already loaded.
- **The supervisor and the job runner stay separate on purpose** (RFC 0129 §Motivation). If you
  find yourself wanting to unify them, re-read that section first — long-lived-idle vs
  bursty-heavy-cancellable really are different shapes.

---

## Files Changed

| File | Change |
|---|---|
| `ekos/crates/cli/src/commands/doctor.rs` | R5: `DoctorJson`, `--json`, `collect_checks`/`build_doctor_json` split; 4 tests |
| `ekos/crates/cli/src/commands/ledger.rs` | R6: `Bucket`, `Timeline`, `build_timeline`, `timeline`; 5 tests |
| `ekos/crates/cli/src/bin/ekos.rs` | `--json` on `Doctor`, `Timeline` subcommand, `emits_machine_output()` + stderr logging for machine-output commands |
| `ekos/docs/rfcs/0129-web-console-phase-1.md` | R6 shape corrected (no `entries` series), open questions updated |
| `web/api/app/{models,supervisor,readproc,_proc}.py` | New |
| `web/api/app/routes/{workspaces,stats}.py` | Registry CRUD + 5 stats endpoints |
| `web/api/app/{main,deps,settings,schemas,mcp_client}.py` | Supervisor on lifespan, DB init + seed, registry-backed workspace lookup, new response models, `ClientPool` removed |
| `web/api/pyproject.toml`, `ruff.toml` | `sqlmodel` dep; `ASYNC240` ignored |
| `web/api/tests/` | `test_readproc.py`, `test_supervisor_live.py`, `test_stats_live.py` new; `test_api.py` rewritten for the registry; `conftest.py` fixtures |
| `web/ui/src/{Layout,pages/Workspaces,pages/Dashboard}.tsx` | New (App.tsx removed) |
| `web/ui/src/{main.tsx,api/client.ts,api/types.ts,index.css}` | Router, POST/DELETE, expanded types, dashboard styles |
| `web/ui/package.json` | `react-router-dom`, `recharts` |
