# Devlog 169 — Eval reports/history in the Web Console; SonarCloud badges

**Date:** 2026-09-05
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct, per this repo's local-tests-only workflow)

---

## Summary

Surfaced RFC 0138's `ekos eval run` reports in the Web Console (RFC 0127): a new read-only
`GET /workspaces/{id}/evals/reports[/{filename}]` route reading the same `evals/reports/*.json`
files the CLI already writes, a new `eval-run` entry in the existing command allowlist (so
triggering a fresh run reuses the console's existing job-runner/SSE-log machinery for free — no
bespoke trigger UI needed), and two new UI pages (`Evals` list + `EvalDetail`) wired into the
per-workspace tab bar. Ran both `web/api` and `web/ui` locally end-to-end and verified through the
real dev-server proxy against this repo's own registered workspace and its three real saved
reports from earlier today's sessions. Also added SonarCloud badges to the root `README.md`.

---

## What was built

| Component | Role |
|---|---|
| `web/api/app/routes/evals.py` (new) | `GET /reports` (list, oldest-first summaries), `GET /reports/{filename}` (full detail) — pure file reads, same relationship to `evals/reports/` that `routes/config.py` has to `ekos.toml` |
| `web/api/app/commands.py` | New `eval-run` allowlist entry (`dataset`/`agent`/`category`/`limit` params, `is_write=True`, 3600s timeout) — the existing generic `Run` UI auto-generates its trigger form from this, no new frontend code needed to *start* a run |
| `web/ui/src/pages/Evals.tsx` (new) | History table — dataset, agent, PASS/FAIL chip, pass count, answer-correctness/hallucination-rate, timestamp |
| `web/ui/src/pages/EvalDetail.tsx` (new) | Full report: an 11-tile metrics grid (the five headline scores + tokens/latency/cache/RSS/CPU) plus a per-scenario pass/fail/hallucinated breakdown |
| `web/ui/src/WorkspaceShell.tsx`, `main.tsx` | New "Evals" tab, `w/:id/evals` + `w/:id/evals/:file` lazy routes |
| `README.md` | SonarCloud badges (maintainability/reliability/security rating + quality gate) at the top |

## Implementation details worth remembering

- **No bespoke "run a new eval" UI was needed.** The console's existing `Run` page
  (`pages/Run.tsx`) already renders one form per allowlisted command, auto-generated from its
  `params` dict — adding `eval-run` to `commands.py` was the entire "let users trigger a run from
  the browser" feature. The dedicated `Evals`/`EvalDetail` pages stay purely read-only, matching
  what was actually asked for (reports/history), and reuse the pre-existing job-runner/SSE-log
  flow (`/runs/:runId`) for watching a triggered run's progress — the same pattern `Schedules.tsx`'s
  "run now" button already established.
- **Path-traversal containment mirrors `config_io.config_path`'s exact pattern** (`resolve()` +
  a `path.parent != reports_dir` check), not a simpler "reject `/` in the filename" check — the
  same SonarCloud pythonsecurity:S2083 hardening this codebase already standardized on
  (devlog_162-adjacent). A corrupt/partial report file (e.g. from a run killed mid-write, like
  this session's own earlier incident) is skipped in the list endpoint rather than 500ing the
  whole page — one bad file shouldn't hide every other saved run.
- **Live-verified against real data, through the real dev-server proxy** (`localhost:5173/api/...`,
  not just the raw backend on `:8000`) — registered this actual repo as a workspace and confirmed
  all three of today's saved reports (including the one with real cache/RSS/CPU metrics from RFC
  0138 Phase 2) round-trip correctly end-to-end. **No Chrome browser tool was available in this
  environment** to take an actual screenshot or click through the UI — verification instead
  combined a real running backend + frontend, curl against the exact proxy path a browser would
  hit, the full component-level React Testing Library suite (which renders the real component tree
  and asserts on real DOM output, not a mock), and a clean `tsc -b`/`vite build`. This is a real,
  named gap versus the ideal verification standard, not silently glossed over.

## Live verification

`uv run ruff check/format --check` clean, `pytest` 75 passed/36 skipped (10 new `test_evals.py`
cases: empty state, summary shape including the new Phase 2 metric fields, corrupt-file skip
behavior, full detail fetch, 404, three path-traversal attempts, role gating, and the `eval-run`
command's registration/params). `tsc -b --noEmit` clean, `vite build` clean with `Evals`/
`EvalDetail` as their own lazy chunks (1.59 KB / 3.16 KB gzipped), 50/50 `vitest` tests (6 new).
**Live-verified**: built a fresh release `ekos` binary (the one on disk predated the `eval`
subcommand entirely), started the real FastAPI backend and Vite dev server, registered this repo
itself as a workspace (its MCP server came up automatically via the existing supervisor), and
fetched both the summary list and one full report through `localhost:5173/api/...` — the exact
path a browser hits — getting back the three real reports already on disk, correctly shaped.

---

## Knowledge Captured

- **The console's command-driven `Run` page generalizes over any allowlisted command for free** —
  adding a browser trigger for a new CLI subcommand is a `commands.py` entry, not new frontend
  code, as long as the command's shape (string/bool params, is_write, summary) fits the existing
  `Command`/`Param` dataclasses. Worth remembering before building a bespoke trigger UI for
  anything that's really just "run this CLI command with these flags."
- **A stale release binary is easy to miss** — `ekos/target/release/ekos` existed but predated this
  session's own `eval` subcommand by two weeks; the web console's `EKOS_BIN` env var pointed at it
  silently until checked. Worth a habit: confirm the release binary's mtime/version before wiring
  it into anything that assumes a specific command exists.

---

## Files Changed

| File | Change summary |
|---|---|
| `web/api/app/routes/evals.py` | New — report list/detail routes |
| `web/api/app/commands.py` | `eval-run` allowlist entry |
| `web/api/app/main.py` | Register the new router |
| `web/api/tests/test_evals.py` | New — 10 tests |
| `web/ui/src/pages/{Evals,EvalDetail}.tsx` | New pages |
| `web/ui/src/pages/Evals.test.tsx` | New — 6 tests |
| `web/ui/src/{WorkspaceShell,main}.tsx` | New tab + routes |
| `web/ui/src/api/types.ts` | `EvalReportSummary`/`EvalReport`/`EvalScenarioReport`/`EvalGateThresholds` |
| `README.md` | SonarCloud badges |
