"""Annotations: what people wrote about an entity (RFC 0247).

The list is a read (`ekos annotate list --json`, `read` role). Writing needs a signed-in person:
adding or withdrawing is the `operate` right, approving or confirming is `review` — the classes of
the same commands in the CLI catalogue. Nothing here goes through MCP, which has no tool that can
write an annotation.
"""

from __future__ import annotations

from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Query
from pydantic import BaseModel, Field

from .. import annotations_write, models, readproc, semantics_write
from ..auth import Principal
from ..authz import allow
from ..deps import require_workspace
from ..settings import Settings, get_settings
from ._responses import BAD_GATEWAY, BAD_REQUEST, CONFLICT

router = APIRouter(prefix="/workspaces/{workspace_id}/annotations", tags=["annotations"])

_OPERATOR = allow("operate")
_REVIEWER = allow("review")
_BIG = 32 << 20


@router.get(
    "", dependencies=[Depends(allow("read"))], responses={400: BAD_REQUEST, 502: BAD_GATEWAY}
)
async def list_annotations(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    target: str | None = Query(default=None, max_length=64),
) -> list[dict]:
    """The current annotations with their state (`active`, `stale`, `withdrawn`)."""
    argv = ["annotate", "list", "--json"]
    if target is not None:
        try:
            argv += ["--target", annotations_write.target_ok(target)]
        except semantics_write.ReviewError as exc:
            raise HTTPException(status_code=400, detail=str(exc)) from exc
    try:
        return await readproc.read_json(settings.ekos_bin, ws.path, argv, max_output=_BIG)
    except readproc.ReadProcError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc


class AddIn(BaseModel):
    target: str = Field(max_length=64)
    kind: str = Field(max_length=16)
    text: str = Field(max_length=2000)
    rule: dict[str, Any] | None = None
    term: str | None = Field(default=None, max_length=200)
    shared: bool = False
    # Token mode only: who is writing (recorded as `token:<name>`). Ignored under OIDC.
    reviewer: str | None = Field(default=None, max_length=80)


class ActIn(BaseModel):
    reviewer: str | None = Field(default=None, max_length=80)


async def _run(
    settings: Settings, ws: models.Workspace, build, principal: Principal, typed: str | None
) -> dict:
    try:
        who = semantics_write.reviewer_for(principal, settings.signs_in, typed)
        message = await semantics_write.review(settings.ekos_bin, ws.path, build(who))
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    except semantics_write.LedgerBusy as exc:
        raise HTTPException(
            status_code=409, detail=f"the workspace is busy (a run holds the ledger): {exc}"
        ) from exc
    except semantics_write.CliFailed as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    return {"ok": True, "author": who, "message": message}


@router.post("", responses={400: BAD_REQUEST, 409: CONFLICT, 502: BAD_GATEWAY})
async def add(
    body: AddIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_OPERATOR),
) -> dict:
    """Write an annotation — `ekos annotate add --as <you> …`. Shared ones wait for approval."""
    return await _run(
        settings,
        ws,
        lambda who: annotations_write.add_argv(
            body.target,
            body.kind,
            body.text,
            who,
            rule=body.rule,
            term=body.term,
            shared=body.shared,
        ),
        principal,
        body.reviewer,
    )


@router.post(
    "/{annotation_id}/withdraw", responses={400: BAD_REQUEST, 409: CONFLICT, 502: BAD_GATEWAY}
)
async def withdraw(
    annotation_id: str,
    body: ActIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_OPERATOR),
) -> dict:
    """Withdraw your own annotation (the CLI refuses anyone else's)."""
    return await _run(
        settings,
        ws,
        lambda who: annotations_write.act_argv("withdraw", annotation_id, who),
        principal,
        body.reviewer,
    )


@router.post(
    "/{annotation_id}/approve", responses={400: BAD_REQUEST, 409: CONFLICT, 502: BAD_GATEWAY}
)
async def approve(
    annotation_id: str,
    body: ActIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_REVIEWER),
) -> dict:
    """Approve someone else's shared instruction or synonym for everyone."""
    return await _run(
        settings,
        ws,
        lambda who: annotations_write.act_argv("approve", annotation_id, who),
        principal,
        body.reviewer,
    )


@router.post(
    "/{annotation_id}/confirm", responses={400: BAD_REQUEST, 409: CONFLICT, 502: BAD_GATEWAY}
)
async def confirm(
    annotation_id: str,
    body: ActIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_REVIEWER),
) -> dict:
    """Re-bind a stale annotation to the entity as it is now (clears an earlier approval)."""
    return await _run(
        settings,
        ws,
        lambda who: annotations_write.act_argv("confirm", annotation_id, who),
        principal,
        body.reviewer,
    )
