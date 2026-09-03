# RFC 0132 — Web Console Phase 4: scheduled runs

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-09-03
**Phase 4 of:** RFC 0127 (§8, §10) · **builds on:** RFC 0131 (Phase 3 job runner + auth,
`devlog_154`)
**Defers:** RFC 0127 Phases 5–7 (graph, hardening) → RFC 0133+

---

## Motivation

Phase 3 runs commands on demand. Phase 4 runs them on a schedule — a nightly `pipeline`, an hourly
`build`, a weekly `docs generate` — so a workspace stays current without someone clicking Run.

**Decisions locked before this RFC** (from the maintainer):

- **A `Schedule` SQLite row is the source of truth**, not APScheduler's own job store. APScheduler
  runs in-memory (`MemoryJobStore`) and is **rebuilt from the table on every console start**. This
  sidesteps APScheduler's pickle-based `SQLAlchemyJobStore` — schedules stay plain, inspectable,
  editable rows, and a library upgrade can't corrupt them.
- **Every schedule has a required `notify_url`.** On any non-`succeeded` terminal run the console
  POSTs a small JSON body to it. Plus the UI always shows each schedule's last-run status. No mail
  server, no default external dependency beyond the operator's own webhook.

**Not in this RFC:** ret/backoff on a failing webhook (best-effort, logged), calendar/RRULE
triggers beyond cron + fixed interval, per-schedule concurrency policy (a schedule that fires
while its previous run is still queued just adds another queue entry, subject to the Phase 3
depth-16 limit).

---

## 1. `models.Schedule`

```python
class Schedule(SQLModel, table=True):
    id: str            # slug
    workspace_id: str
    command: str        # a COMMAND_ALLOWLIST name
    params: dict        # JSON, validated against the command on create
    trigger_kind: str   # "cron" | "interval"
    trigger_expr: str   # cron: a 5-field crontab string; interval: integer seconds as text
    notify_url: str     # required — POSTed on a failed run
    enabled: bool = True
    created_at: datetime
    last_run_at: datetime | None
    last_run_id: str | None
    last_status: str | None   # mirrors the Run's terminal status
```

CRUD helpers next to the existing `Run`/`Workspace` ones. No APScheduler tables.

---

## 2. `web/api/app/scheduler.py` — `ConsoleScheduler`

Wraps **APScheduler 3.x** `AsyncIOScheduler` (runs on the FastAPI event loop, so it calls the
async `JobRunner.submit` directly) with a `MemoryJobStore`.

```python
class ConsoleScheduler:
    def __init__(self, runner: JobRunner) -> None: ...
    def start(self) -> None            # load every enabled Schedule row, add a job each
    async def aclose(self) -> None
    def add(self, s: Schedule) -> None
    def update(self, s: Schedule) -> None   # remove + re-add
    def remove(self, schedule_id: str) -> None
    async def fire(self, schedule_id: str) -> str   # run-now / the job callback; returns run_id
```

- **Trigger construction:** `trigger_kind == "cron"` → `CronTrigger.from_crontab(trigger_expr)`
  (UTC); `"interval"` → `IntervalTrigger(seconds=int(trigger_expr))`. Both validated at create
  time — a bad expression is a `422`, never a runtime crash.
- **`fire`** loads the row, calls `runner.submit(ws_id, ws_path, command, params, on_done=…)`,
  writes `last_run_at` / `last_run_id`, and returns the run id. It does **not** block on the run.
- **`on_done`** (new `JobRunner` hook, §3) fires when the run reaches a terminal status: the
  scheduler writes `last_status`, and if it isn't `succeeded` it POSTs the webhook.
- A `QueueFull` from `submit` is recorded as a synthetic failed run + a webhook, so a scheduler
  that outpaces the queue is visible rather than silent.

---

## 3. `JobRunner` — an `on_done` callback

Phase 3's `submit` gains one optional parameter:

```python
async def submit(self, ..., on_done: Callable[[Run], Awaitable[None]] | None = None) -> str
```

Stored as `self._on_done[run_id]`; invoked (via `asyncio.create_task`, exceptions logged) from
`_execute`'s `finally` after the terminal status is written. Manual runs pass `None`. This is the
only Phase 3 change — everything else in `runner.py` is untouched.

---

## 4. HTTP surface (RFC 0127 §8.3)

```
GET    /api/schedules?workspace=                 # list, each with last-run status (read)
POST   /api/schedules   {workspace_id, command, params, trigger_kind, trigger_expr, notify_url}
                                                 # validate everything, create, register (write)
PATCH  /api/schedules/{id}  {enabled?, trigger_*?, params?, notify_url?}   # re-register (write)
DELETE /api/schedules/{id}                       # (write)
POST   /api/schedules/{id}/run-now               # fire immediately → {run_id}  (write)
```

`POST`/`PATCH` validation: the command must be in `COMMAND_ALLOWLIST`, `params` must pass
`Command.render_argv`, `notify_url` must be `http(s)://`, and the trigger must construct. An
`is_write` command scheduled by a non-write principal is a `403` (same rule as Phase 3's
`POST /commands/{name}`).

---

## 5. Frontend

A **Schedules** route (`/schedules`, linked from the header):

- A table: name · workspace · command · trigger (human-readable) · last run (status chip + time) ·
  an enable/disable switch · **Run now** · delete.
- A create form: workspace select, command select (from `/api/commands`), a params sub-form
  (reused from the Run page), a trigger toggle — a crontab string field or an interval-seconds
  field — and the `notify_url` field.
- The whole page is hidden / read-only for the `read` role (schedules only run write commands).

`EventSource`/xterm not involved; `run-now` links to the Phase 3 `/runs/:id` detail.

---

## 6. Testing

**`web/api`** (pytest)
- `Schedule` CRUD + validation: a bad crontab / negative interval / non-allowlisted command /
  non-`http` `notify_url` → `422`; a write command + read principal → `403`.
- `ConsoleScheduler`: `start()` registers one APScheduler job per enabled row and none for a
  disabled row; `update` re-registers; `remove` deregisters.
- `fire` (live, `EKOS_BIN`): a schedule for `doctor` fires → a real run is submitted, `last_run_id`
  / `last_status` are written; a schedule for a command that fails → the webhook is POSTed
  (asserted against a local `aiohttp`/`httpx` capture server) with the documented body; a
  `succeeded` run POSTs nothing.
- An `IntervalTrigger(seconds=1)` schedule actually fires twice within ~2.5 s (fast-path timing
  test, not `EKOS_BIN`-gated — `runner.submit` monkeypatched).

**`web/ui`**: `tsc` + `vite build`; a render test of the schedules table.

---

## 7. Verification

- Rust: untouched.
- `web/api`: `ruff` clean, `pytest` green (unit + `EKOS_BIN`-gated fire/webhook).
- `web/ui`: `tsc` clean, `vite build` succeeds.
- End to end (token mode, recorded in the phase devlog): create an interval schedule for `status`
  on a scratch workspace with `notify_url` pointing at a local capture server; watch two runs land
  in `/runs`; break the command (schedule `pipeline` on a workspace with no `ekos.toml`) and see
  the webhook POST arrive.

---

## 8. Files changed (projected)

| File | Change |
|---|---|
| `web/api/app/models.py` | `Schedule` table + CRUD |
| `web/api/app/scheduler.py` | Stub → `ConsoleScheduler` |
| `web/api/app/runner.py` | `on_done` callback on `submit` |
| `web/api/app/routes/schedules.py` | New — CRUD + run-now |
| `web/api/app/main.py` | `ConsoleScheduler` on lifespan (after the runner), schedules router |
| `web/api/pyproject.toml` | `apscheduler>=3.10,<4` |
| `web/ui/src/pages/Schedules.tsx` | New |
| `web/ui/src/{main,Layout}.tsx` | Route + nav link |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Phase 4 note |

---

## 9. Open questions

1. **Webhook auth.** The POST is unauthenticated (the operator's `notify_url` can carry a secret
   in a query param or path). A signed header (HMAC over the body with a per-schedule secret) is a
   small add if anyone wants it.
2. **Missed runs while the console was down.** APScheduler's `misfire_grace_time` default (1 s)
   means a schedule that should have fired during a restart is skipped, not caught up. That's the
   right default for a `pipeline` (you don't want a thundering herd on boot); documented, not
   configurable in this RFC.
3. **Timezone.** All cron expressions are UTC. A per-schedule tz is a later refinement.
