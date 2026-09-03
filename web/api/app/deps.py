"""Shared FastAPI dependencies: workspace lookup + per-workspace MCP client resolution.

Auth (`require_role`) lives in `app.auth`.
"""

from __future__ import annotations

from fastapi import Depends, HTTPException, Request

from . import models
from .mcp_client import EkosMcpClient
from .supervisor import McpSupervisor, _NotReady


def get_supervisor(request: Request) -> McpSupervisor:
    return request.app.state.supervisor


def require_workspace(workspace_id: str) -> models.Workspace:
    ws = models.get_workspace(workspace_id)
    if ws is None:
        raise HTTPException(status_code=404, detail=f"unknown workspace {workspace_id!r}")
    return ws


async def mcp_for_workspace(
    ws: models.Workspace = Depends(require_workspace),
    supervisor: McpSupervisor = Depends(get_supervisor),
) -> EkosMcpClient:
    try:
        return supervisor.client_for(ws.id)
    except _NotReady as exc:
        raise HTTPException(status_code=503, detail=f"mcp server not ready: {exc}") from exc
