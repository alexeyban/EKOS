"""Shared FastAPI dependencies: workspace lookup + per-workspace MCP client resolution.

Authorization lives in `app.authz`; `require_workspace` is re-exported from there because it
answers 404 for a workspace the caller may not see.
"""

from __future__ import annotations

from fastapi import Depends, HTTPException, Request

from . import authz, models
from .auth import Principal
from .authz import require_workspace
from .mcp_client import ScopedMcp
from .supervisor import McpSupervisor, _NotReady

__all__ = ["get_supervisor", "mcp_for_workspace", "require_workspace"]


def get_supervisor(request: Request) -> McpSupervisor:
    return request.app.state.supervisor


# Async on purpose: FastAPI runs an async dependency on the event loop, which owns the supervisor's
# handle table; a sync one would read it from a thread-pool thread.
async def mcp_for_workspace(  # NOSONAR(python:S7503)
    ws: models.Workspace = Depends(require_workspace),
    principal: Principal = Depends(authz.authenticated),
    supervisor: McpSupervisor = Depends(get_supervisor),
) -> ScopedMcp:
    """The workspace's MCP server, as this caller may read it (RFC 0181): the client it returns
    attaches the caller's read scope to every call. This is the only way a route gets an MCP
    client, so no route reaches the ledger without the scope."""
    try:
        client = supervisor.client_for(ws.id)
    except _NotReady as exc:
        raise HTTPException(status_code=503, detail=f"mcp server not ready: {exc}") from exc
    return ScopedMcp(client, authz.scope_for(principal, ws))
