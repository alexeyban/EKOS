"""Scheduled runs (RFC 0132).

A `models.Schedule` SQLite row is the source of truth. APScheduler (`AsyncIOScheduler`,
`MemoryJobStore`) is rebuilt from the enabled rows on every console start — no pickle job store.
On a non-`succeeded` terminal run the schedule's required `notify_url` is POSTed.
"""

from __future__ import annotations

import logging

import httpx
from apscheduler.schedulers.asyncio import AsyncIOScheduler
from apscheduler.triggers.cron import CronTrigger
from apscheduler.triggers.interval import IntervalTrigger

from . import models
from .commands import BY_NAME
from .models import Run, Schedule
from .runner import JobRunner

log = logging.getLogger("ekos.console.scheduler")

_WEBHOOK_TIMEOUT = 10.0


class ScheduleError(ValueError):
    """A trigger expression or command that can't be scheduled."""


def build_trigger(kind: str, expr: str):
    """Construct an APScheduler trigger, raising `ScheduleError` on anything malformed."""
    if kind == "cron":
        try:
            return CronTrigger.from_crontab(expr, timezone="UTC")
        except Exception as exc:
            raise ScheduleError(f"invalid crontab {expr!r}: {exc}") from exc
    if kind == "interval":
        try:
            seconds = int(expr)
        except ValueError as exc:
            raise ScheduleError(f"interval must be integer seconds, got {expr!r}") from exc
        if seconds < 1:
            raise ScheduleError("interval must be >= 1 second")
        return IntervalTrigger(seconds=seconds)
    raise ScheduleError(f"unknown trigger kind {kind!r}")


class ConsoleScheduler:
    def __init__(self, runner: JobRunner) -> None:
        self._runner = runner
        self._sched = AsyncIOScheduler(timezone="UTC")

    def start(self) -> None:
        self._sched.start()
        for row in models.list_schedules():
            if row.enabled:
                self._register(row)

    async def aclose(self) -> None:
        self._sched.shutdown(wait=False)

    # ── registration ─────────────────────────────────────────────────────────

    def _register(self, s: Schedule) -> None:
        self._sched.add_job(
            self.fire,
            trigger=build_trigger(s.trigger_kind, s.trigger_expr),
            args=[s.id],
            id=s.id,
            replace_existing=True,
            misfire_grace_time=1,
            coalesce=True,
        )

    def add(self, s: Schedule) -> None:
        if s.enabled:
            self._register(s)

    def update(self, s: Schedule) -> None:
        self.remove(s.id)
        if s.enabled:
            self._register(s)

    def remove(self, schedule_id: str) -> None:
        job = self._sched.get_job(schedule_id)
        if job is not None:
            job.remove()

    def job_ids(self) -> list[str]:
        return [j.id for j in self._sched.get_jobs()]

    # ── firing ───────────────────────────────────────────────────────────────

    async def fire(self, schedule_id: str) -> str | None:
        """Submit the schedule's command. Returns the run id, or None if the schedule/workspace
        is gone. Does not block on the run — completion is handled by the `on_done` callback."""
        s = models.get_schedule(schedule_id)
        if s is None:
            self.remove(schedule_id)
            return None
        ws = models.get_workspace(s.workspace_id)
        if ws is None:
            log.warning("schedule %s: workspace %s is gone", s.id, s.workspace_id)
            return None
        command = BY_NAME.get(s.command)
        if command is None:  # pragma: no cover - guarded on create
            return None

        async def _on_done(run: Run) -> None:
            models.update_schedule(s.id, last_status=run.status)
            if run.status != "succeeded":
                await self._notify(s, run)

        try:
            run_id = await self._runner.submit(
                s.workspace_id, ws.path, command, s.params, on_done=_on_done
            )
        except Exception as exc:  # QueueFull, etc.
            log.warning("schedule %s: submit failed: %s", s.id, exc)
            models.update_schedule(s.id, last_status="failed", last_run_at=models._now())
            await self._notify(s, None, detail=str(exc))
            return None

        models.update_schedule(s.id, last_run_at=models._now(), last_run_id=run_id)
        return run_id

    async def _notify(self, s: Schedule, run: Run | None, detail: str = "") -> None:
        body = {
            "schedule_id": s.id,
            "workspace_id": s.workspace_id,
            "command": s.command,
            "run_id": run.id if run else None,
            "status": run.status if run else "failed",
            "detail": detail,
        }
        await self.post_webhook(s.notify_url, body)

    async def post_webhook(self, url: str, body: dict) -> None:
        """Best-effort POST — logged on failure, no retry in Phase 4. Its own method so tests can
        capture it without patching all of httpx."""
        try:
            async with httpx.AsyncClient(timeout=_WEBHOOK_TIMEOUT) as client:
                await client.post(url, json=body)
        except Exception as exc:
            log.warning("notify_url POST to %s failed: %s", url, exc)
