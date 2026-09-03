"""Graph + search reads, proxied to the MCP tools (RFC 0128 §3.2)."""

from __future__ import annotations

from typing import Any

from fastapi import APIRouter, Depends, Query

from ..deps import mcp_for_workspace, require_console_token
from ..mcp_client import EkosMcpClient
from ..schemas import GraphOut

router = APIRouter(
    prefix="/workspaces", tags=["graph"], dependencies=[Depends(require_console_token)]
)


@router.get("/{workspace_id}/graph", response_model=GraphOut)
async def graph_export(
    mcp: EkosMcpClient = Depends(mcp_for_workspace),
    level: str = Query("object", pattern="^(object|aggregate)$"),
    group_by: str = Query("kind", pattern="^(kind|path_prefix)$"),
    kind: list[str] = Query(default=[]),
    exclude_rel_kind: list[str] = Query(default=[]),
    min_degree: int = Query(0, ge=0),
    max_nodes: int = Query(5000, ge=1),
    max_edges: int = Query(20000, ge=1),
) -> GraphOut:
    args: dict[str, Any] = {
        "level": level,
        "group_by": group_by,
        "min_degree": min_degree,
        "max_nodes": max_nodes,
        "max_edges": max_edges,
    }
    if kind:
        args["kinds"] = kind
    if exclude_rel_kind:
        args["exclude_rel_kinds"] = exclude_rel_kind
    return GraphOut.model_validate(await mcp.call_tool("ekos_graph_export", args))


@router.get("/{workspace_id}/search")
async def search(
    mcp: EkosMcpClient = Depends(mcp_for_workspace),
    q: str = Query(min_length=1),
    limit: int = Query(20, ge=1, le=100),
) -> Any:
    return await mcp.call_tool("ekos_search", {"query": q, "limit": limit})
