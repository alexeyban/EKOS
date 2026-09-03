"""Console persistence — SQLite via SQLModel (RFC 0127 §8.1, RFC 0129 §3).

Phase 1 defines exactly one table: the workspace registry. `Run`, `Schedule`, and any `User`
table are added by the phases that need them (3, 4, and whichever introduces the role split).
The rows are low-volume and every query is by primary key, so the sync SQLModel `Session` is
used directly from async routes — a threadpool hop would cost more than the query.
"""

from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path

from sqlmodel import Field, Session, SQLModel, create_engine, select


class Workspace(SQLModel, table=True):
    """One registered workspace. `path` is an absolute directory containing `ekos.toml` and
    `.ekos/` — validated before the row is written."""

    id: str = Field(primary_key=True)
    name: str
    path: str
    created_at: datetime = Field(default_factory=lambda: datetime.now(UTC))


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
