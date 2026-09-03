"""Workspace registry + per-workspace status (RFC 0128 §3.2)."""

from __future__ import annotations

from fastapi import APIRouter, Depends

from ..deps import mcp_for_workspace, require_console_token
from ..mcp_client import EkosMcpClient
from ..schemas import StatusOut, WorkspaceOut
from ..settings import Settings, get_settings

router = APIRouter(
    prefix="/workspaces", tags=["workspaces"], dependencies=[Depends(require_console_token)]
)


@router.get("", response_model=list[WorkspaceOut])
async def list_workspaces(settings: Settings = Depends(get_settings)) -> list[WorkspaceOut]:
    return [WorkspaceOut(id=w.id, name=w.name, path=w.path) for w in settings.workspaces()]


@router.get("/{workspace_id}/stats", response_model=StatusOut)
async def workspace_stats(mcp: EkosMcpClient = Depends(mcp_for_workspace)) -> StatusOut:
    # The `ekos_status` MCP tool. The richer `ekos status --json` (RFC 0127 R2, with storage
    # breakdown + evidence count + last_write) needs a subprocess and arrives with the Phase 1
    # job runner.
    result = await mcp.call_tool("ekos_status")
    return StatusOut(
        entries=result.get("entries", 0),
        objects=result.get("objects", 0),
        relationships=result.get("relationships"),
    )
