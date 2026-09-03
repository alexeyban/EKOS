"""Scheduler (RFC 0127 §8.1) — STUB, Phase 4.

APScheduler 3.x with a `SQLAlchemyJobStore` on SQLite. Cron/interval schedules trigger the same
pipeline chains the job runner executes; a failed stage raises a failure notification.
"""

from __future__ import annotations


class ConsoleScheduler:
    def __init__(self) -> None:  # pragma: no cover - stub
        raise NotImplementedError("scheduler arrives with RFC 0127 Phase 4")
