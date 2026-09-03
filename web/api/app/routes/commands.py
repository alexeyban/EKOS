"""The command catalogue + submitting a run (RFC 0131 §2, §4)."""

from __future__ import annotations

from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Request

from .. import commands as cmds
from .. import models
from ..auth import check_role, require_role
from ..deps import require_workspace
from ..runner import JobRunner, QueueFull
from ..settings import get_settings

router = APIRouter(tags=["commands"])


@router.get("/commands", dependencies=[Depends(require_role("read"))])
async def list_commands() -> list[dict]:
    return cmds.catalogue()


def _runner(request: Request) -> JobRunner:
    return request.app.state.runner


@router.post("/workspaces/{workspace_id}/commands/{name}")
async def run_command(
    name: str,
    request: Request,
    body: dict[str, Any] | None = None,
    ws: models.Workspace = Depends(require_workspace),
) -> dict:
    command = cmds.BY_NAME.get(name)
    if command is None:
        raise HTTPException(status_code=404, detail=f"no such command {name!r}")

    # role gate: is_write commands need the write role, the rest need read.
    check_role(
        request,
        request.headers.get("authorization", ""),
        get_settings(),
        "write" if command.is_write else "read",
    )

    try:
        run_id = await _runner(request).submit(ws.id, ws.path, command, body or {})
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    except QueueFull as exc:
        raise HTTPException(status_code=429, detail=str(exc)) from exc
    return {"run_id": run_id}
