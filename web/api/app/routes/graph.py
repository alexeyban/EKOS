"""Graph + search + object-state reads, proxied to the MCP tools (RFC 0128 §3.2, RFC 0133 §1)."""

from __future__ import annotations

from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Query

from ..auth import require_role
from ..deps import mcp_for_workspace
from ..layout import compute_layout
from ..mcp_client import EkosMcpClient, McpToolError
from ..schemas import GraphOut, LayoutIn, LayoutOut

router = APIRouter(
    prefix="/workspaces", tags=["graph"], dependencies=[Depends(require_role("read"))]
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
    include_properties: bool = Query(False),
    as_of: str | None = Query(None, description="RFC 3339 — reconstruct the graph as of then"),
    include_first_seen: bool = Query(False),
) -> GraphOut:
    args: dict[str, Any] = {
        "level": level,
        "group_by": group_by,
        "min_degree": min_degree,
        "max_nodes": max_nodes,
        "max_edges": max_edges,
        "include_properties": include_properties,
        "include_first_seen": include_first_seen,
    }
    if kind:
        args["kinds"] = kind
    if exclude_rel_kind:
        args["exclude_rel_kinds"] = exclude_rel_kind
    if as_of:
        args["as_of"] = as_of
    return GraphOut.model_validate(await mcp.call_tool("ekos_graph_export", args))


@router.get("/{workspace_id}/objects/{object_id}")
async def object_state(
    object_id: str,
    mcp: EkosMcpClient = Depends(mcp_for_workspace),
    as_of: str | None = Query(None, description="RFC 3339 — reconstruct state as of then"),
) -> Any:
    """`ekos_state` — the object, its relationships, and the resolved evidence behind every
    claim (RFC 0133 §2.4). `as_of` reconstructs historical state (RFC 0134 §3.7)."""
    args: dict[str, Any] = {"id": object_id}
    if as_of:
        args["at"] = as_of
    try:
        return await mcp.call_tool("ekos_state", args)
    except McpToolError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc


@router.get("/{workspace_id}/search")
async def search(
    mcp: EkosMcpClient = Depends(mcp_for_workspace),
    q: str = Query(min_length=1),
    limit: int = Query(20, ge=1, le=100),
) -> Any:
    return await mcp.call_tool("ekos_search", {"query": q, "limit": limit})


@router.get("/{workspace_id}/neighborhood/{object_id}")
async def neighborhood(
    object_id: str,
    mcp: EkosMcpClient = Depends(mcp_for_workspace),
    depth: int = Query(1, ge=1, le=3),
) -> Any:
    """`ekos_neighborhood` (RFC 0136 §2) — a real sub-graph (objects *and* relationships), not
    just an id list, so the console draws the returned edges directly rather than reconstructing
    them client-side."""
    return await mcp.call_tool("ekos_neighborhood", {"id": object_id, "depth": depth})


@router.get("/{workspace_id}/impact/{object_id}")
async def impact(
    object_id: str,
    mcp: EkosMcpClient = Depends(mcp_for_workspace),
    direction: str = Query("dependents", pattern="^(dependents|dependencies)$"),
    max_hops: int = Query(5, ge=1, le=50),
    kind: list[str] = Query(default=[]),
) -> Any:
    """`ekos_impact` (RFC 0136 §3) — a hop-depth node list (`hops: [{hop, id, name, kind, via}]`),
    not an edge-path trace; the console highlights nodes by hop and edges already present in the
    loaded graph between two impacted nodes, rather than reconstructing a precise path."""
    args: dict[str, Any] = {"id": object_id, "direction": direction, "max_hops": max_hops}
    if kind:
        args["kinds"] = kind
    try:
        return await mcp.call_tool("ekos_impact", args)
    except McpToolError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc


@router.post("/{workspace_id}/graph/layout", response_model=LayoutOut)
async def graph_layout(workspace_id: str, body: LayoutIn) -> LayoutOut:
    """Server-side ForceAtlas2 (RFC 0136 §4) — for graphs past the client-side simulation
    threshold. No MCP call: pure graph-structure input already held by the caller from its own
    prior `/graph` fetch. `workspace_id` isn't used (layout has no per-workspace state), but kept
    in the path for URL symmetry with every other route here and to leave room for a future
    per-workspace cache namespace without a breaking route change."""
    positions = compute_layout(body.nodes, body.edges)
    return LayoutOut(positions=positions)
