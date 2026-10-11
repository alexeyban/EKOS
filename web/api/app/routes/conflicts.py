"""Conflicting evidence: where sources disagree, and a person's decision (RFC 0172/0174).

The list is a read (`ekos conflicts list --json`, `read` role). A decision needs the `write`
role, runs the human-only `ekos conflicts resolve`, and is attributed to the signed-in user.
Nothing here goes through MCP, which has no tool that can settle a conflict.
"""

from __future__ import annotations

from typing import Literal

from fastapi import APIRouter, Depends, HTTPException, Query
from pydantic import BaseModel, Field

from .. import conflicts_write, models, readproc, semantics_write
from ..auth import Principal, require_role
from ..deps import require_workspace
from ..settings import Settings, get_settings
from ._responses import BAD_GATEWAY, BAD_REQUEST, CONFLICT

router = APIRouter(prefix="/workspaces/{workspace_id}/conflicts", tags=["conflicts"])

_WRITER = require_role("write")
_BIG = 32 << 20

Status = Literal["open", "resolved", "dismissed"]
Type = Literal["duplicate_definition", "merge_loss", "label_mismatch", "doc_vs_data"]


@router.get("", dependencies=[Depends(require_role("read"))], responses={502: BAD_GATEWAY})
async def conflicts(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    status: Status | None = Query(default=None),
    type: Type | None = Query(default=None),
) -> list[dict]:
    """The current conflicts, each with every claim's file and line."""
    argv = ["conflicts", "list", "--json"]
    if status:
        argv += ["--status", status]
    if type:
        argv += ["--type", type]
    try:
        return await readproc.read_json(settings.ekos_bin, ws.path, argv, max_output=_BIG)
    except readproc.ReadProcError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc


class ResolveIn(BaseModel):
    # The 1-based number of the right claim, or `both_valid` — exactly one.
    pick: int | None = Field(default=None, ge=1, le=100)
    both_valid: bool = False
    note: str | None = Field(default=None, max_length=2000)
    # Token mode only: who is deciding (recorded as `token:<name>`). Ignored under OIDC.
    reviewer: str | None = Field(default=None, max_length=80)


@router.post(
    "/{conflict_id}/resolve",
    responses={400: BAD_REQUEST, 409: CONFLICT, 502: BAD_GATEWAY},
)
async def resolve(
    conflict_id: str,
    body: ResolveIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_WRITER),
) -> dict:
    """Decide one conflict — `ekos conflicts resolve <id> --pick=<n> | --both-valid --as <you>`."""
    try:
        who = semantics_write.reviewer_for(principal, settings.oidc_enabled, body.reviewer)
        argv = conflicts_write.resolve_argv(
            conflict_id, who, pick=body.pick, both_valid=body.both_valid, note=body.note
        )
        message = await semantics_write.review(settings.ekos_bin, ws.path, argv)
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    except semantics_write.LedgerBusy as exc:
        raise HTTPException(
            status_code=409, detail=f"the workspace is busy (a run holds the ledger): {exc}"
        ) from exc
    except semantics_write.CliFailed as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    return {"ok": True, "reviewer": who, "message": message}
