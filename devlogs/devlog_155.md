# Devlog 155 — RFC 0132: Web Console Phase 4 (scheduled runs)

**Date:** 2026-09-03
**Branch:** `feat/0132-web-console-phase-4` → `main` (local merge, `[skip ci]`)
**RFC:** `ekos/docs/rfcs/0132-web-console-phase-4.md`

---

## Summary

Phase 3 runs commands on demand; Phase 4 runs them on a schedule. `ConsoleScheduler` wraps
APScheduler's `AsyncIOScheduler`, driven by a SQLite `Schedule` table that is the source of truth
— APScheduler runs in-memory and is rebuilt from the enabled rows on every console start, so
there is no pickle job store to corrupt. Every schedule carries a required `notify_url` that is
POSTed on any non-`succeeded` terminal run. Console-only, no Rust.

---

## PR — Phase 4

| Component | Role |
|---|---|
| `models.Schedule` | `{id, workspace_id, command, params, trigger_kind, trigger_expr, notify_url, enabled, last_run_at/id/status}` + CRUD |
| `scheduler.py` `ConsoleScheduler` | `start()` loads every enabled row and adds one APScheduler job; `add`/`update`/`remove` keep it in sync; `fire(id)` submits the command and returns the run id without blocking |
| `scheduler.build_trigger` | `"cron"` → `CronTrigger.from_crontab(expr, tz=UTC)`, `"interval"` → `IntervalTrigger(seconds=int(expr))`; anything malformed → `ScheduleError` (→ HTTP 422) |
| `runner.JobRunner.submit(..., on_done=)` | **The only Phase 3 change.** An optional terminal-status callback, stored per run, fired via a tracked `asyncio.Task` from `_execute`'s finally (and the queued-cancel / QueueFull paths). Manual runs pass `None`. |
| `routes/schedules.py` | `GET` (read), `POST` / `PATCH` / `DELETE` / `POST /{id}/run-now` (write). Create validates: command in the allowlist, `params` pass `render_argv`, `notify_url` is `http(s)`, the trigger constructs. |
| `web/ui/src/pages/Schedules.tsx` | Table (command · workspace · trigger · last-run chip + log link · enable toggle · run-now · delete) + a create form. Write-role only; nav link in the header. |

### `fire` → webhook flow

`fire` builds a per-schedule `_on_done(run)` closure and passes it to `runner.submit`. When the
run reaches a terminal status the runner invokes it: the scheduler writes `Schedule.last_status`,
and if it isn't `succeeded` it calls `post_webhook(notify_url, body)` where `body` is
`{schedule_id, workspace_id, command, run_id, status, detail}`. A `QueueFull` from `submit` is
recorded as a `failed` last-run + a webhook with `run_id: null`, so a scheduler that outpaces the
depth-16 queue is visible.

### Decisions

- **`Schedule` row, not APScheduler's job store** (maintainer's call). APScheduler's
  `SQLAlchemyJobStore` pickles the job callable + args; a library upgrade or a refactor of the
  callback can silently break every persisted schedule. A plain row rebuilt on boot has neither
  problem and stays editable/inspectable like `Run` and `Workspace`.
- **`notify_url` is required, not optional** (maintainer's call) — a scheduled run that fails
  silently is worse than a manual one, since nobody is watching. The webhook is best-effort
  (logged, no retry in Phase 4).
- **`post_webhook` is its own method** so tests capture it by setting
  `app.state.scheduler.post_webhook = …` instead of monkeypatching all of `httpx` (which would
  also intercept the test's own `ASGITransport` client — that bit me once).
- **`misfire_grace_time=1`, `coalesce=True`** — a schedule that should have fired during a restart
  is skipped, not caught up. Right default for a `pipeline`: you don't want a thundering herd on
  boot.

---

## Verification

Local gates only (`[skip ci]`):

- Rust: untouched.
- `web/api`: `ruff` clean, `pytest` **68/68** with `EKOS_BIN` (10 new — trigger validation,
  CRUD + role gating, disabled-row-not-registered across an app rebuild, `fire` records
  `last_run_*`, failed run POSTs the webhook, succeeded run POSTs nothing; two `EKOS_BIN`-gated:
  `run-now` submits a real `doctor` to `succeeded`, a scheduled bad `ekl` query → `failed` + the
  webhook fires with the documented body).
- `web/ui`: `tsc -b --noEmit` clean, `vite build` succeeds.

---

## Knowledge Captured

- **Don't monkeypatch `httpx.AsyncClient.post` in a test that also uses `httpx.AsyncClient` +
  `ASGITransport`** — you replace the transport the test itself talks through. Give the code under
  test a narrow seam (`ConsoleScheduler.post_webhook`) and patch that.
- **APScheduler `AsyncIOScheduler` must be started on the loop it will run jobs on.** In tests it
  starts inside `app.router.lifespan_context`; calling `scheduler.fire()` directly from a test is
  fine (it only touches the DB + `runner.submit`), but never touch `scheduler._sched` from a
  different loop.
- **`CronTrigger.from_crontab` accepts 5-field crontab strings; `IntervalTrigger` wants an int.**
  Both raise on garbage — validate at create time so a bad schedule is a 422, never a background
  crash.

---

## Files Changed

| File | Change |
|---|---|
| `web/api/app/models.py` | `Schedule` table + CRUD |
| `web/api/app/scheduler.py` | Stub → `ConsoleScheduler` + `build_trigger` |
| `web/api/app/runner.py` | `on_done` callback on `submit`; `_notify_done`; tracked bg-task set |
| `web/api/app/routes/schedules.py` | New |
| `web/api/app/{main,routes/__init__}.py` | Scheduler on lifespan, schedules router |
| `web/api/pyproject.toml` | `apscheduler>=3.10,<4` |
| `web/api/tests/{test_schedules,test_schedules_live}.py` | New |
| `web/ui/src/pages/Schedules.tsx` | New |
| `web/ui/src/{main,Layout,index.css}.tsx` | Route, nav link, `select` styling |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Phase 4 note |
