"""Statistics endpoints (RFC 0129 §4).

`stats` / `health` / `stats/timeline` shell out through the read-only subprocess seam (the MCP
tools only expose leaner payloads). `stats/kinds` goes through the already-running MCP server.
`stats/queries` reads the workspace's own query-usage log (RFC 0114) off disk.
"""

from __future__ import annotations

import json
from pathlib import Path

from fastapi import APIRouter, Depends, HTTPException, Query

from .. import models, readproc
from ..deps import mcp_for_workspace, require_console_token, require_workspace
from ..mcp_client import EkosMcpClient
from ..schemas import (
    DoctorOut,
    KindCount,
    QueryStats,
    StatusOut,
    TimelineOut,
)
from ..settings import Settings, get_settings

router = APIRouter(
    prefix="/workspaces", tags=["stats"], dependencies=[Depends(require_console_token)]
)


def _bin(settings: Settings) -> str:
    return settings.ekos_bin


@router.get("/{workspace_id}/stats", response_model=StatusOut)
async def stats(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
) -> StatusOut:
    try:
        payload = await readproc.read_json(_bin(settings), ws.path, ["status", "--json"])
    except readproc.ReadProcError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc
    return StatusOut.model_validate(payload)


@router.get("/{workspace_id}/health", response_model=DoctorOut)
async def health(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
) -> DoctorOut:
    try:
        payload = await readproc.read_json(_bin(settings), ws.path, ["doctor", "--json"])
    except readproc.ReadProcError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc
    return DoctorOut.model_validate(payload)


@router.get("/{workspace_id}/stats/timeline", response_model=TimelineOut)
async def timeline(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    bucket: str = Query("day", pattern="^(day|week|month)$"),
    since: str | None = Query(None),
) -> TimelineOut:
    argv = ["ledger", "timeline", "--json", "--bucket", bucket]
    if since:
        argv += ["--since", since]
    try:
        payload = await readproc.read_json(_bin(settings), ws.path, argv)
    except readproc.ReadProcError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc
    return TimelineOut.model_validate(payload)


@router.get("/{workspace_id}/stats/kinds", response_model=list[KindCount])
async def kinds(mcp: EkosMcpClient = Depends(mcp_for_workspace)) -> list[KindCount]:
    result = await mcp.call_tool("ekos_ekl", {"query": "FIND Object COUNT GROUP BY kind"})
    rows = result.get("rows", []) if isinstance(result, dict) else []
    out: list[KindCount] = []
    for row in rows:
        kind = row.get("kind")
        count = row.get("count")
        if kind is None or count is None:
            continue
        out.append(KindCount(kind=str(kind), count=int(count)))
    out.sort(key=lambda k: k.count, reverse=True)
    return out


@router.get("/{workspace_id}/stats/queries", response_model=QueryStats)
async def queries(
    ws: models.Workspace = Depends(require_workspace),
    limit: int = Query(2000, ge=1, le=50_000),
) -> QueryStats:
    log = Path(ws.path) / ".ekos" / "query-log.jsonl"
    if not log.is_file():
        return QueryStats(total=0, by_tool={}, cache_hit_rate=0.0, p50_ms=0.0, p95_ms=0.0)

    lines = log.read_text(errors="replace").splitlines()[-limit:]
    by_tool: dict[str, int] = {}
    durations: list[float] = []
    hits = 0
    total = 0
    for line in lines:
        try:
            rec = json.loads(line)
        except json.JSONDecodeError:
            continue
        total += 1
        tool = rec.get("tool", "unknown")
        by_tool[tool] = by_tool.get(tool, 0) + 1
        if rec.get("cache_hit"):
            hits += 1
        d = rec.get("duration_ms")
        if isinstance(d, (int, float)):
            durations.append(float(d))

    durations.sort()

    def pct(p: float) -> float:
        if not durations:
            return 0.0
        idx = min(len(durations) - 1, int(p * len(durations)))
        return round(durations[idx], 2)

    return QueryStats(
        total=total,
        by_tool=dict(sorted(by_tool.items(), key=lambda kv: kv[1], reverse=True)),
        cache_hit_rate=round(hits / total, 3) if total else 0.0,
        p50_ms=pct(0.50),
        p95_ms=pct(0.95),
    )
