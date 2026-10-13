"""The command catalogue + submitting a run (RFC 0131 §2, §4)."""

from __future__ import annotations

from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Request

from .. import authz, models
from .. import commands as cmds
from ..auth import Principal
from ..deps import require_workspace
from ..runner import JobRunner, QueueFull
from ._responses import NOT_FOUND, TOO_MANY, UNPROCESSABLE

router = APIRouter(tags=["commands"])


@router.get("/commands", dependencies=[Depends(authz.authenticated)])
async def list_commands() -> list[dict]:
    return cmds.catalogue()


def _runner(request: Request) -> JobRunner:
    return request.app.state.runner


@router.post(
    "/workspaces/{workspace_id}/commands/{name}",
    responses={404: NOT_FOUND, 422: UNPROCESSABLE, 429: TOO_MANY},
)
async def run_command(
    name: str,
    request: Request,
    body: dict[str, Any] | None = None,
    ws: models.Workspace = Depends(require_workspace),
    principal: Principal = Depends(authz.authenticated),
) -> dict:
    command = cmds.BY_NAME.get(name)
    if command is None:
        raise HTTPException(status_code=404, detail=f"no such command {name!r}")

    # The action depends on the command: one that writes needs `operate`, the rest `read`.
    authz.require(
        principal,
        "operate" if command.is_write else "read",
        ws.tenant_id,
        ws.id,
        request=request,
    )
    # Any command prints what it read; none of them takes a read scope (RFC 0181).
    authz.require_all_of(principal, ws)

    try:
        run_id = await _runner(request).submit(ws.id, ws.path, command, body or {})
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    except QueueFull as exc:
        raise HTTPException(status_code=429, detail=str(exc)) from exc
    return {"run_id": run_id}
