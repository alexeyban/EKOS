"""Run history, detail, SSE log tail, cancellation (RFC 0131 §4)."""

from __future__ import annotations

import asyncio
from pathlib import Path
from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Query, Request
from fastapi.responses import StreamingResponse

from .. import models
from ..auth import require_role
from ..models import TERMINAL

router = APIRouter(prefix="/runs", tags=["runs"])


def _dump(run: models.Run, *, tail: int = 0) -> dict[str, Any]:
    body: dict[str, Any] = {
        "id": run.id,
        "workspace_id": run.workspace_id,
        "command": run.command,
        "params": run.params,
        "status": run.status,
        "stages": run.stages,
        "exit_code": run.exit_code,
        "created_at": run.created_at.isoformat(),
        "started_at": run.started_at.isoformat() if run.started_at else None,
        "ended_at": run.ended_at.isoformat() if run.ended_at else None,
    }
    if tail:
        log = Path(run.log_path)
        body["log_tail"] = (
            log.read_text(errors="replace").splitlines()[-tail:] if log.is_file() else []
        )
    return body


@router.get("", dependencies=[Depends(require_role("read"))])
async def list_runs(
    workspace: str | None = Query(None),
    status: str | None = Query(None),
    limit: int = Query(100, ge=1, le=500),
) -> list[dict]:
    return [_dump(r) for r in models.list_runs(workspace, status, limit)]


@router.get("/{run_id}", dependencies=[Depends(require_role("read"))])
async def get_run(run_id: str) -> dict:
    run = models.get_run(run_id)
    if run is None:
        raise HTTPException(status_code=404, detail="no such run")
    return _dump(run, tail=500)


@router.post("/{run_id}/cancel", dependencies=[Depends(require_role("write"))])
async def cancel_run(run_id: str, request: Request) -> dict:
    run = models.get_run(run_id)
    if run is None:
        raise HTTPException(status_code=404, detail="no such run")
    ok = await request.app.state.runner.cancel(run_id)
    return {"cancelled": ok}


@router.get("/{run_id}/logs", dependencies=[Depends(require_role("read"))])
async def stream_logs(run_id: str) -> StreamingResponse:
    run = models.get_run(run_id)
    if run is None:
        raise HTTPException(status_code=404, detail="no such run")
    log_path = Path(run.log_path)

    async def events():
        sent = 0
        while True:
            if log_path.is_file():
                lines = log_path.read_text(errors="replace").splitlines()
                for line in lines[sent:]:
                    yield f"data: {line}\n\n"
                sent = len(lines)
            fresh = models.get_run(run_id)
            if fresh is None or fresh.status in TERMINAL:
                yield f"event: end\ndata: {fresh.status if fresh else 'gone'}\n\n"
                return
            await asyncio.sleep(0.25)

    return StreamingResponse(events(), media_type="text/event-stream")
