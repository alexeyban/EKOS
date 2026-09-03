"""Shared FastAPI dependencies: console auth + per-workspace MCP client resolution."""

from __future__ import annotations

import secrets

from fastapi import Depends, Header, HTTPException, Request

from . import models
from .mcp_client import EkosMcpClient
from .settings import Settings, get_settings
from .supervisor import McpSupervisor, _NotReady


def require_console_token(
    authorization: str = Header(default=""),
    settings: Settings = Depends(get_settings),
) -> None:
    """Bearer-token gate for every /api route except /api/health (RFC 0128 §3.3).

    RFC 0129 keeps this a single static token; real users and a read/write role split arrive with
    the first browser write path (Phase 3).
    """
    prefix = "Bearer "
    supplied = authorization[len(prefix) :] if authorization.startswith(prefix) else ""
    if not secrets.compare_digest(supplied, settings.console_token):
        raise HTTPException(status_code=401, detail="invalid or missing console token")


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
