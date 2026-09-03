"""Console persistence — SQLite via SQLModel (RFC 0127 §8.1, RFC 0129 §3).

Phase 1 defines exactly one table: the workspace registry. `Run`, `Schedule`, and any `User`
table are added by the phases that need them (3, 4, and whichever introduces the role split).
The rows are low-volume and every query is by primary key, so the sync SQLModel `Session` is
used directly from async routes — a threadpool hop would cost more than the query.
"""

from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from sqlalchemy import Column
from sqlalchemy.types import JSON
from sqlmodel import Field, Session, SQLModel, create_engine, select


def _now() -> datetime:
    return datetime.now(UTC)


class Workspace(SQLModel, table=True):
    """One registered workspace. `path` is an absolute directory containing `ekos.toml` and
    `.ekos/` — validated before the row is written."""

    id: str = Field(primary_key=True)
    name: str
    path: str
    created_at: datetime = Field(default_factory=_now)


TERMINAL = {"succeeded", "failed", "cancelled", "timed_out", "interrupted"}


class Run(SQLModel, table=True):
    """One command execution (RFC 0131 §3)."""

    id: str = Field(primary_key=True)
    workspace_id: str = Field(index=True)
    command: str
    params: dict[str, Any] = Field(default_factory=dict, sa_column=Column(JSON))
    status: str = Field(default="queued", index=True)
    stages: list[dict[str, Any]] = Field(default_factory=list, sa_column=Column(JSON))
    exit_code: int | None = None
    log_path: str = ""
    created_at: datetime = Field(default_factory=_now)
    started_at: datetime | None = None
    ended_at: datetime | None = None


class Schedule(SQLModel, table=True):
    """A recurring command run (RFC 0132). This row is the source of truth; APScheduler is
    rebuilt from the enabled rows on every console start."""

    id: str = Field(primary_key=True)
    workspace_id: str = Field(index=True)
    command: str
    params: dict[str, Any] = Field(default_factory=dict, sa_column=Column(JSON))
    trigger_kind: str  # "cron" | "interval"
    trigger_expr: str  # cron: 5-field crontab string; interval: integer seconds as text
    notify_url: str  # required — POSTed on a non-succeeded terminal run
    enabled: bool = True
    created_at: datetime = Field(default_factory=_now)
    last_run_at: datetime | None = None
    last_run_id: str | None = None
    last_status: str | None = None


_engine = None


def init_engine(db_path: str) -> None:
    """Create the SQLite file (and its parent dir) and every table. Idempotent."""
    global _engine
    p = Path(db_path)
    p.parent.mkdir(parents=True, exist_ok=True)
    _engine = create_engine(f"sqlite:///{p}", connect_args={"check_same_thread": False})
    SQLModel.metadata.create_all(_engine)


def session() -> Session:
    if _engine is None:  # pragma: no cover - guarded by lifespan
        raise RuntimeError("models.init_engine() has not been called")
    # expire_on_commit=False: callers read attributes off the returned rows after the `with`
    # block closes. `Workspace` has no relationships, so every column is already loaded — the
    # detached instances stay fully usable.
    return Session(_engine, expire_on_commit=False)


def list_workspaces() -> list[Workspace]:
    with session() as s:
        return list(s.exec(select(Workspace).order_by(Workspace.created_at)))


def get_workspace(workspace_id: str) -> Workspace | None:
    with session() as s:
        return s.get(Workspace, workspace_id)


def add_workspace(ws: Workspace) -> None:
    with session() as s:
        s.add(ws)
        s.commit()


def delete_workspace(workspace_id: str) -> bool:
    with session() as s:
        row = s.get(Workspace, workspace_id)
        if row is None:
            return False
        s.delete(row)
        s.commit()
        return True


# ── Run ──────────────────────────────────────────────────────────────────────


def add_run(run: Run) -> None:
    with session() as s:
        s.add(run)
        s.commit()


def get_run(run_id: str) -> Run | None:
    with session() as s:
        return s.get(Run, run_id)


def list_runs(
    workspace_id: str | None = None, status: str | None = None, limit: int = 100
) -> list[Run]:
    with session() as s:
        q = select(Run).order_by(Run.created_at.desc()).limit(limit)
        if workspace_id:
            q = q.where(Run.workspace_id == workspace_id)
        if status:
            q = q.where(Run.status == status)
        return list(s.exec(q))


def update_run(run_id: str, **fields: Any) -> None:
    with session() as s:
        row = s.get(Run, run_id)
        if row is None:  # pragma: no cover - the runner always creates the row first
            return
        for k, v in fields.items():
            setattr(row, k, v)
        s.add(row)
        s.commit()


def sweep_stale_runs() -> int:
    """On startup, any run left `queued`/`running` (the console died mid-run) is `interrupted`."""
    with session() as s:
        rows = list(s.exec(select(Run).where(Run.status.in_(["queued", "running"]))))
        for row in rows:
            row.status = "interrupted"
            row.ended_at = _now()
            s.add(row)
        s.commit()
        return len(rows)


# ── Schedule ─────────────────────────────────────────────────────────────────


def add_schedule(sched: Schedule) -> None:
    with session() as s:
        s.add(sched)
        s.commit()


def get_schedule(schedule_id: str) -> Schedule | None:
    with session() as s:
        return s.get(Schedule, schedule_id)


def list_schedules(workspace_id: str | None = None) -> list[Schedule]:
    with session() as s:
        q = select(Schedule).order_by(Schedule.created_at)
        if workspace_id:
            q = q.where(Schedule.workspace_id == workspace_id)
        return list(s.exec(q))


def update_schedule(schedule_id: str, **fields: Any) -> Schedule | None:
    with session() as s:
        row = s.get(Schedule, schedule_id)
        if row is None:
            return None
        for k, v in fields.items():
            setattr(row, k, v)
        s.add(row)
        s.commit()
        s.refresh(row)
        return row


def delete_schedule(schedule_id: str) -> bool:
    with session() as s:
        row = s.get(Schedule, schedule_id)
        if row is None:
            return False
        s.delete(row)
        s.commit()
        return True
