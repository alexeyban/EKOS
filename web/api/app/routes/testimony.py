"""Testimony (RFC 0253 §3.4 to 3.6): what a person told EKOS, in their own words.

* `GET  /workspaces/{id}/testimony` — the testimonies (`ekos testimony list --json`, `read`),
  filtered by entity (`about`) or author; an entity page shows "What people said" from it.
* `POST /workspaces/{id}/testimony` — record your own statement (`review`): the human-only
  `ekos testimony add --as <you>`.
* `POST /workspaces/{id}/testimony/{tid}/withdraw` — withdraw your own (`review`).
* `POST /workspaces/{id}/proposals/{pid}/save` — the Save button of a chat proposal card: the
  statement's author only, with their own (or edited) text and the proposals they kept.
* `POST /workspaces/{id}/proposals/{pid}/discard` — the author drops a proposal card.

None of this is reachable from the chat agent or MCP: a model proposes, a person saves.
"""

from __future__ import annotations

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel, Field

from .. import authz, models, people_write, proposals
from ..auth import Principal
from ..deps import require_workspace
from ..settings import Settings, get_settings
from . import _people
from ._responses import BAD_GATEWAY, BAD_REQUEST, CONFLICT, NOT_FOUND

router = APIRouter(tags=["testimony"])

_READ = authz.allow("read")
_REVIEW = authz.allow("review")


@router.get(
    "/workspaces/{workspace_id}/testimony",
    dependencies=[Depends(authz.allow("read"))],
    responses={502: BAD_GATEWAY},
)
async def testimonies(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    about: str | None = None,
    author: str | None = None,
    all: bool = False,
) -> list[dict]:
    argv = ["testimony", "list", "--json"] + (["--all"] if all else [])
    for flag, value in (("--about", about), ("--author", author)):
        if value:
            try:
                value = people_write._short(flag[2:], value)
            except people_write.ReviewError as exc:
                raise _people.bad_request(exc) from exc
            if value:
                argv += [flag, value]
    return await _people.read(settings, ws, argv)


class StatementIn(BaseModel):
    text: str = Field(max_length=4000)
    about: list[str] = Field(default_factory=list, max_length=20)
    answers: str | None = Field(default=None, max_length=40)
    meaning: str | None = Field(default=None, max_length=300)
    reviewer: str | None = Field(default=None, max_length=80)


@router.post("/workspaces/{workspace_id}/testimony", responses={400: BAD_REQUEST, 409: CONFLICT})
async def add(
    body: StatementIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_REVIEW),
) -> dict:
    who = _people.person(principal, settings, body.reviewer)
    try:
        argv = people_write.testimony_add_argv(
            body.text, who, about=body.about, answers=body.answers, meaning=body.meaning
        )
    except people_write.ReviewError as exc:
        raise _people.bad_request(exc) from exc
    return {"ok": True, "by": who, **(await _people.write(settings, ws, argv))}


class WithdrawIn(BaseModel):
    reviewer: str | None = Field(default=None, max_length=80)


@router.post(
    "/workspaces/{workspace_id}/testimony/{tid}/withdraw",
    responses={400: BAD_REQUEST, 409: CONFLICT},
)
async def withdraw(
    tid: str,
    body: WithdrawIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_REVIEW),
) -> dict:
    who = _people.person(principal, settings, body.reviewer)
    try:
        argv = people_write.testimony_withdraw_argv(tid, who)
    except people_write.ReviewError as exc:
        raise _people.bad_request(exc) from exc
    return {"ok": True, "by": who, **(await _people.write(settings, ws, argv))}


class SaveIn(BaseModel):
    # The person's correction of their own words; empty keeps what they wrote.
    text: str | None = Field(default=None, max_length=4000)
    # Which proposals (by position on the card) to keep; omitted keeps them all.
    keep: list[int] | None = Field(default=None, max_length=50)
    reviewer: str | None = Field(default=None, max_length=80)


def _own_proposal(pid: str, ws: models.Workspace, principal: Principal) -> models.Proposal:
    p = models.get_proposal(pid)
    if p is None or p.workspace_id != ws.id or p.author != proposals.author_key(principal):
        # Somebody else's card is not there for this caller (RFC 0253 §3.6: only the author's
        # click counts).
        raise HTTPException(status_code=404, detail="no such proposal")
    if p.status != "open":
        raise HTTPException(status_code=409, detail=f"this proposal is already {p.status}")
    return p


@router.post(
    "/workspaces/{workspace_id}/proposals/{pid}/save",
    responses={400: BAD_REQUEST, 404: NOT_FOUND, 409: CONFLICT},
)
async def save(
    pid: str,
    body: SaveIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_REVIEW),
) -> dict:
    p = _own_proposal(pid, ws, principal)
    who = _people.person(principal, settings, body.reviewer)
    try:
        calls = proposals.save_argvs(p, who, edited=body.text, keep=body.keep)
    except people_write.ReviewError as exc:
        raise _people.bad_request(exc) from exc
    saved = [await _people.write(settings, ws, argv) for argv in calls]
    models.set_proposal_status(p.id, "saved", saved_by=who)
    authz.audit(
        principal,
        "testimony: save",
        "ok",
        tenant_id=ws.tenant_id,
        workspace_id=ws.id,
        detail=f"channel={p.channel} message={p.message_ref}",
    )
    return {"ok": True, "by": who, "saved": saved}


@router.post(
    "/workspaces/{workspace_id}/proposals/{pid}/discard",
    dependencies=[Depends(authz.not_read_only)],
    responses={404: NOT_FOUND, 409: CONFLICT},
)
async def discard(
    pid: str,
    ws: models.Workspace = Depends(require_workspace),
    principal: Principal = Depends(_READ),
) -> dict:
    p = _own_proposal(pid, ws, principal)
    models.set_proposal_status(p.id, "discarded")
    return {"ok": True}
