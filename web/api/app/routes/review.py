"""The workflow bar and the source-model review (RFC 0213 Phase 3).

Reads (`ekos workflow status`, `ekos review list|show`) need the `read` right. Decisions and
sign-off need `review` — which no API token can hold (RFC 0180) — run the human-only CLI commands
(`review_write`), and are attributed to the signed-in person. Nothing here goes through MCP.

The first read after a ledger change computes every subject from the whole ledger (about 25 s on
iDempiere); later reads come from the CLI's cache. Hence the long timeouts.
"""

from __future__ import annotations

from typing import Literal

from fastapi import APIRouter, Depends, HTTPException, Query
from pydantic import BaseModel, Field

from .. import models, readproc, review_write, semantics_write
from ..auth import Principal
from ..authz import allow
from ..deps import require_workspace
from ..settings import Settings, get_settings
from ._responses import BAD_GATEWAY, BAD_REQUEST, CONFLICT

router = APIRouter(prefix="/workspaces/{workspace_id}", tags=["review"])

_REVIEWER = allow("review")
_BIG = 32 << 20
_SLOW = 300.0

Subject = Literal["scope", "pii", "conflicts", "identity", "write-behaviour", "glossary"]


async def _read(settings: Settings, ws: models.Workspace, argv: list[str]):
    try:
        return await readproc.read_json(
            settings.ekos_bin, ws.path, argv, max_output=_BIG, time_limit=_SLOW
        )
    except readproc.ReadProcError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc


@router.get("/workflow", dependencies=[Depends(allow("read"))], responses={502: BAD_GATEWAY})
async def workflow(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
) -> dict:
    """The seven steps with their states, step 3 with every subject's sign-off."""
    return await _read(settings, ws, ["workflow", "status", "--json"])


@router.get("/review", dependencies=[Depends(allow("read"))], responses={502: BAD_GATEWAY})
async def subjects(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
) -> dict:
    """Every subject: items, decided, outdated, blocking, digest, sign-off."""
    return await _read(settings, ws, ["review", "list", "--json"])


@router.get(
    "/review/{subject}", dependencies=[Depends(allow("read"))], responses={502: BAD_GATEWAY}
)
async def subject_items(
    subject: Subject,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    undecided: bool = Query(default=False),
    limit: int | None = Query(default=None, ge=1, le=100000),
) -> dict:
    """One subject's items with the facts shown, decisions and the digest a sign-off needs."""
    argv = ["review", "show", subject, "--json"]
    if undecided:
        argv.append("--undecided")
    if limit is not None:
        argv += ["--limit", str(limit)]
    return await _read(settings, ws, argv)


class DecideIn(BaseModel):
    targets: list[str] = Field(min_length=1, max_length=500)
    # scope: in|out · pii: confirm|reject · write-behaviour: confirm · identity: confirm|reject
    verdict: str = Field(max_length=16)
    mask: str | None = Field(default=None, max_length=16)
    reason: str | None = Field(default=None, max_length=2000)
    # write-behaviour overrides: update_class, delete_classes (comma list), change_log.
    overrides: dict[str, str] | None = None
    # Token mode only: who is deciding (recorded as `token:<name>`). Ignored under OIDC.
    reviewer: str | None = Field(default=None, max_length=80)


class SignoffIn(BaseModel):
    digest: str = Field(max_length=64)
    # Typed confirmation: the subject's name, as the person typed it.
    confirm: str = Field(max_length=32)
    note: str | None = Field(default=None, max_length=2000)
    reviewer: str | None = Field(default=None, max_length=80)


class RevokeIn(BaseModel):
    reason: str = Field(max_length=2000)
    reviewer: str | None = Field(default=None, max_length=80)


async def _write(settings: Settings, ws: models.Workspace, build) -> str:
    try:
        argv = build()
        return await semantics_write.review(settings.ekos_bin, ws.path, argv, time_limit=_SLOW)
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    except semantics_write.LedgerBusy as exc:
        raise HTTPException(
            status_code=409, detail=f"the workspace is busy (a run holds the ledger): {exc}"
        ) from exc
    except semantics_write.CliFailed as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc


_WRITE_RESPONSES = {400: BAD_REQUEST, 409: CONFLICT, 502: BAD_GATEWAY}


@router.post("/review/{subject}/decide", responses=_WRITE_RESPONSES)
async def decide(
    subject: Subject,
    body: DecideIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_REVIEWER),
) -> dict:
    """Decide items — `ekos review <subject> … --as <you> -- <items>`."""
    try:
        who = semantics_write.reviewer_for(principal, settings.oidc_enabled, body.reviewer)
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    message = await _write(
        settings,
        ws,
        lambda: review_write.decide_argv(
            subject,
            body.targets,
            who,
            verdict=body.verdict,
            mask=body.mask,
            reason=body.reason,
            overrides=body.overrides,
        ),
    )
    return {"ok": True, "reviewer": who, "message": message}


@router.post("/review/{subject}/signoff", responses=_WRITE_RESPONSES)
async def signoff(
    subject: Subject,
    body: SignoffIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_REVIEWER),
) -> dict:
    """Sign a subject off on the digest the person was shown, after they typed its name."""
    if body.confirm.strip() != subject:
        raise HTTPException(status_code=400, detail=f"type `{subject}` to confirm the sign-off")
    try:
        who = semantics_write.reviewer_for(principal, settings.oidc_enabled, body.reviewer)
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    message = await _write(
        settings, ws, lambda: review_write.approve_argv(subject, body.digest, who, note=body.note)
    )
    return {"ok": True, "reviewer": who, "message": message}


@router.post("/review/{subject}/revoke", responses=_WRITE_RESPONSES)
async def revoke(
    subject: Subject,
    body: RevokeIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_REVIEWER),
) -> dict:
    """Withdraw a subject's sign-off."""
    try:
        who = semantics_write.reviewer_for(principal, settings.oidc_enabled, body.reviewer)
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    message = await _write(
        settings, ws, lambda: review_write.revoke_argv(subject, body.reason, who)
    )
    return {"ok": True, "reviewer": who, "message": message}
