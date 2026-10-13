"""Shared FastAPI dependencies: workspace lookup + per-workspace MCP client resolution.

Authorization lives in `app.authz`; `require_workspace` is re-exported from there because it
answers 404 for a workspace the caller may not see.
"""

from __future__ import annotations

from fastapi import Depends, HTTPException, Request

from . import models
from .authz import require_workspace
from .mcp_client import EkosMcpClient
from .supervisor import McpSupervisor, _NotReady

__all__ = ["get_supervisor", "mcp_for_workspace", "require_workspace"]


def get_supervisor(request: Request) -> McpSupervisor:
    return request.app.state.supervisor


# Async on purpose: FastAPI runs an async dependency on the event loop, which owns the supervisor's
# handle table; a sync one would read it from a thread-pool thread.
async def mcp_for_workspace(  # NOSONAR(python:S7503)
    ws: models.Workspace = Depends(require_workspace),
    supervisor: McpSupervisor = Depends(get_supervisor),
) -> EkosMcpClient:
    try:
        return supervisor.client_for(ws.id)
    except _NotReady as exc:
        raise HTTPException(status_code=503, detail=f"mcp server not ready: {exc}") from exc
