"""Shared FastAPI dependencies: console auth + per-workspace MCP client resolution."""

from __future__ import annotations

import secrets

from fastapi import Depends, Header, HTTPException, Request

from .mcp_client import EkosMcpClient
from .settings import Settings, WorkspaceConfig, get_settings


def require_console_token(
    authorization: str = Header(default=""),
    settings: Settings = Depends(get_settings),
) -> None:
    """Bearer-token gate for every /api route except /api/health (RFC 0128 §3.3)."""
    prefix = "Bearer "
    supplied = authorization[len(prefix) :] if authorization.startswith(prefix) else ""
    if not secrets.compare_digest(supplied, settings.console_token):
        raise HTTPException(status_code=401, detail="invalid or missing console token")


def _workspace(workspace_id: str, settings: Settings) -> WorkspaceConfig:
    for ws in settings.workspaces():
        if ws.id == workspace_id:
            return ws
    raise HTTPException(status_code=404, detail=f"unknown workspace {workspace_id!r}")


async def mcp_for_workspace(
    workspace_id: str,
    request: Request,
    settings: Settings = Depends(get_settings),
) -> EkosMcpClient:
    ws = _workspace(workspace_id, settings)
    pool = request.app.state.pool
    return await pool.get(ws.id, ws.mcp_host, ws.mcp_port, settings.mcp_token or None)
