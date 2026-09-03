"""Schedule CRUD + run-now (RFC 0132 §4)."""

from __future__ import annotations

import uuid
from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Query, Request
from pydantic import BaseModel, field_validator

from .. import commands as cmds
from .. import models
from ..auth import check_role, require_role
from ..deps import require_workspace
from ..scheduler import ConsoleScheduler, ScheduleError, build_trigger
from ..settings import get_settings

router = APIRouter(prefix="/schedules", tags=["schedules"])


class ScheduleIn(BaseModel):
    workspace_id: str
    command: str
    params: dict[str, Any] = {}
    trigger_kind: str
    trigger_expr: str
    notify_url: str

    @field_validator("notify_url")
    @classmethod
    def _http_url(cls, v: str) -> str:
        if not v.startswith(("http://", "https://")):
            raise ValueError("notify_url must be an http(s) URL")
        return v


class SchedulePatch(BaseModel):
    enabled: bool | None = None
    params: dict[str, Any] | None = None
    trigger_kind: str | None = None
    trigger_expr: str | None = None
    notify_url: str | None = None


def _sched(request: Request) -> ConsoleScheduler:
    return request.app.state.scheduler


def _dump(s: models.Schedule) -> dict:
    return {
        "id": s.id,
        "workspace_id": s.workspace_id,
        "command": s.command,
        "params": s.params,
        "trigger_kind": s.trigger_kind,
        "trigger_expr": s.trigger_expr,
        "notify_url": s.notify_url,
        "enabled": s.enabled,
        "last_run_at": s.last_run_at.isoformat() if s.last_run_at else None,
        "last_run_id": s.last_run_id,
        "last_status": s.last_status,
    }


def _validate(command_name: str, params: dict, kind: str, expr: str) -> None:
    command = cmds.BY_NAME.get(command_name)
    if command is None:
        raise HTTPException(status_code=422, detail=f"unknown command {command_name!r}")
    try:
        command.render_argv(params)
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    try:
        build_trigger(kind, expr)
    except ScheduleError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc


@router.get("", dependencies=[Depends(require_role("read"))])
async def list_schedules(workspace: str | None = Query(None)) -> list[dict]:
    return [_dump(s) for s in models.list_schedules(workspace)]


@router.post("", status_code=201, dependencies=[Depends(require_role("write"))])
async def create_schedule(body: ScheduleIn, request: Request) -> dict:
    require_workspace(body.workspace_id)  # 404 if unknown
    _validate(body.command, body.params, body.trigger_kind, body.trigger_expr)
    command = cmds.BY_NAME[body.command]
    if command.is_write:
        check_role(request, request.headers.get("authorization", ""), get_settings(), "write")

    row = models.Schedule(
        id=uuid.uuid4().hex[:12],
        workspace_id=body.workspace_id,
        command=body.command,
        params=body.params,
        trigger_kind=body.trigger_kind,
        trigger_expr=body.trigger_expr,
        notify_url=body.notify_url,
    )
    models.add_schedule(row)
    _sched(request).add(row)
    return _dump(row)


@router.patch("/{schedule_id}", dependencies=[Depends(require_role("write"))])
async def patch_schedule(schedule_id: str, body: SchedulePatch, request: Request) -> dict:
    current = models.get_schedule(schedule_id)
    if current is None:
        raise HTTPException(status_code=404, detail="no such schedule")
    fields = body.model_dump(exclude_none=True)
    if "notify_url" in fields and not fields["notify_url"].startswith(("http://", "https://")):
        raise HTTPException(status_code=422, detail="notify_url must be an http(s) URL")
    merged = {**_dump(current), **fields}
    _validate(merged["command"], merged["params"], merged["trigger_kind"], merged["trigger_expr"])

    row = models.update_schedule(schedule_id, **fields)
    _sched(request).update(row)
    return _dump(row)


@router.delete("/{schedule_id}", status_code=204, dependencies=[Depends(require_role("write"))])
async def delete_schedule(schedule_id: str, request: Request) -> None:
    if not models.delete_schedule(schedule_id):
        raise HTTPException(status_code=404, detail="no such schedule")
    _sched(request).remove(schedule_id)


@router.post("/{schedule_id}/run-now", dependencies=[Depends(require_role("write"))])
async def run_now(schedule_id: str, request: Request) -> dict:
    if models.get_schedule(schedule_id) is None:
        raise HTTPException(status_code=404, detail="no such schedule")
    run_id = await _sched(request).fire(schedule_id)
    if run_id is None:
        raise HTTPException(status_code=409, detail="could not fire (workspace gone or queue full)")
    return {"run_id": run_id}
