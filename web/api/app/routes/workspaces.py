"""Workspace registry + per-workspace MCP-server status (RFC 0129 §4)."""

from __future__ import annotations

from pathlib import Path

from fastapi import APIRouter, Depends, HTTPException

from .. import models
from ..auth import require_role
from ..deps import get_supervisor
from ..schemas import ServerStatus, WorkspaceCreate, WorkspaceOut
from ..settings import Settings, get_settings
from ..supervisor import McpSupervisor

router = APIRouter(
    prefix="/workspaces", tags=["workspaces"], dependencies=[Depends(require_role("read"))]
)


def _out(ws: models.Workspace, supervisor: McpSupervisor) -> WorkspaceOut:
    handle = supervisor.handle(ws.id)
    server = ServerStatus(**handle.status()) if handle is not None else None
    return WorkspaceOut(id=ws.id, name=ws.name, path=ws.path, server=server)


@router.get("", response_model=list[WorkspaceOut])
async def list_workspaces(
    supervisor: McpSupervisor = Depends(get_supervisor),
) -> list[WorkspaceOut]:
    return [_out(ws, supervisor) for ws in models.list_workspaces()]


@router.post(
    "",
    response_model=WorkspaceOut,
    status_code=201,
    dependencies=[Depends(require_role("write"))],
)
async def register_workspace(
    body: WorkspaceCreate,
    supervisor: McpSupervisor = Depends(get_supervisor),
    settings: Settings = Depends(get_settings),
) -> WorkspaceOut:
    if models.get_workspace(body.id) is not None:
        raise HTTPException(status_code=409, detail=f"workspace {body.id!r} already registered")

    root = Path(body.path).expanduser().resolve()
    if settings.workspaces_root:
        allowed_root = Path(settings.workspaces_root).expanduser().resolve()
        if root != allowed_root and allowed_root not in root.parents:
            raise HTTPException(
                status_code=400,
                detail=f"{root} is outside the configured workspaces root {allowed_root}",
            )
    if not (root / "ekos.toml").is_file():
        raise HTTPException(status_code=400, detail=f"{root} has no ekos.toml")
    if not (root / ".ekos").is_dir():
        raise HTTPException(
            status_code=400, detail=f"{root} has no .ekos/ — run the pipeline there first"
        )

    ws = models.Workspace(id=body.id, name=body.name, path=str(root))
    models.add_workspace(ws)
    await supervisor.ensure(ws)
    return _out(ws, supervisor)


@router.delete("/{workspace_id}", status_code=204, dependencies=[Depends(require_role("write"))])
async def deregister_workspace(
    workspace_id: str,
    supervisor: McpSupervisor = Depends(get_supervisor),
) -> None:
    if models.get_workspace(workspace_id) is None:
        raise HTTPException(status_code=404, detail=f"unknown workspace {workspace_id!r}")
    await supervisor.stop(workspace_id)
    models.delete_workspace(workspace_id)
