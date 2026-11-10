"""Open questions (RFC 0254): what EKOS needs a person to tell it, ranked, and who to ask.

* `GET  …/questions`                the list (`ekos questions list --json`, `read`), filtered by
  status, type, table and addressee.
* `GET  …/questions/{qid}`          one, with its context and answers.
* `GET  …/questions/{qid}/reveal`   the same with the unexplained values read live
  (`questions show --reveal`): counts only for a personal column, small cells hidden, the result
  shown and never stored (RFC 0254 §3.2).
* `POST …/questions/{qid}/answer`   an answer heard elsewhere (a meeting), recorded as testimony
  **by the operator** with the person named in `--on-behalf-of` (`review`; human-only).
* `POST …/questions/{qid}/reassign` hand it to someone (`operate`).
* `POST …/questions/{qid}/decline`  "Not me": only the person it is with, as themselves.
* `POST …/questions/mute`           no questions about tables matching a pattern, for yourself.
"""

from __future__ import annotations

from typing import Literal

from fastapi import APIRouter, Depends, HTTPException, Query
from pydantic import BaseModel, Field

from .. import authz, models, people_write
from ..auth import Principal
from ..deps import require_workspace
from ..settings import Settings, get_settings
from . import _people
from ._responses import BAD_GATEWAY, BAD_REQUEST, CONFLICT

router = APIRouter(prefix="/workspaces/{workspace_id}/questions", tags=["questions"])

_READ = authz.allow("read")
_REVIEW = authz.allow("review")
_OPERATE = authz.allow("operate")
# "Not me" and muting need only `read` (they are about the caller themselves), so a read-only
# showcase refuses them here; every other write needs a right a showcase never grants.
_ACTS = [Depends(authz.not_read_only)]

NOT_THEIRS = {"description": "The question is with someone else"}

Status = Literal["open", "answered", "closed", "obsolete", "all"]
QType = Literal[
    "meaning_of_value",
    "unexplained_values",
    "dead_code",
    "which_is_right",
    "logic_vs_data",
    "is_this_a_reference",
    "what_is_a_row",
    "still_true",
]


def _qid(qid: str) -> str:
    try:
        return people_write.object_id(qid)
    except people_write.ReviewError as exc:
        raise _people.bad_request(exc) from exc


@router.get("", dependencies=[Depends(authz.allow("read"))], responses={502: BAD_GATEWAY})
async def questions(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    status: Status = "open",
    type: QType | None = None,
    table: str | None = Query(default=None, max_length=200),
    addressee: str | None = Query(default=None, max_length=200),
    limit: int = 0,
) -> list[dict]:
    argv = ["questions", "list", "--json", "--status", status]
    if type:
        argv += ["--type", type]
    for flag, value in (("--table", table), ("--addressee", addressee)):
        if isinstance(value, str) and value.strip():
            try:
                argv += [flag, people_write._short(flag[2:], value) or ""]
            except people_write.ReviewError as exc:
                raise _people.bad_request(exc) from exc
    if limit > 0:
        argv += ["--limit", str(min(limit, 10_000))]
    return await _people.read(settings, ws, argv)


@router.get("/{qid}", dependencies=[Depends(authz.allow("read"))], responses={502: BAD_GATEWAY})
async def question(
    qid: str,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
) -> dict:
    return await _people.read(settings, ws, ["questions", "show", "--json", _qid(qid)])


@router.get(
    "/{qid}/reveal", dependencies=[Depends(authz.allow("read"))], responses={502: BAD_GATEWAY}
)
async def reveal(
    qid: str,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(authz.authenticated),
) -> dict:
    """The live values; the request is audited (who looked), the values are not."""
    authz.audit(
        principal,
        "questions: reveal",
        "allowed",
        tenant_id=ws.tenant_id,
        workspace_id=ws.id,
        detail=f"question={qid[:36]}",
    )
    return await _people.read(settings, ws, ["questions", "show", "--json", "--reveal", _qid(qid)])


class AnswerIn(BaseModel):
    # What the person said, as the operator heard it.
    text: str = Field(max_length=4000)
    # Who said it (a name or e-mail): recorded as `on_behalf_of`, never as the author.
    said_by: str = Field(max_length=200)
    about: list[str] = Field(default_factory=list, max_length=20)
    meaning: str | None = Field(default=None, max_length=300)
    reviewer: str | None = Field(default=None, max_length=80)


@router.post("/{qid}/answer", responses={400: BAD_REQUEST, 409: CONFLICT})
async def answer(
    qid: str,
    body: AnswerIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_REVIEW),
) -> dict:
    who = _people.person(principal, settings, body.reviewer)
    if not body.said_by.strip():
        raise HTTPException(status_code=400, detail="say who gave the answer")
    try:
        argv = people_write.testimony_add_argv(
            body.text,
            who,
            about=body.about,
            answers=_qid(qid),
            meaning=body.meaning,
            on_behalf_of=body.said_by,
        )
    except people_write.ReviewError as exc:
        raise _people.bad_request(exc) from exc
    return {"ok": True, "by": who, **(await _people.write(settings, ws, argv))}


class ReassignIn(BaseModel):
    to: str = Field(max_length=200)


@router.post("/{qid}/reassign", responses={400: BAD_REQUEST, 409: CONFLICT})
async def reassign(
    qid: str,
    body: ReassignIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    _: Principal = Depends(_OPERATE),
) -> dict:
    try:
        argv = people_write.reassign_argv(_qid(qid), body.to)
    except people_write.ReviewError as exc:
        raise _people.bad_request(exc) from exc
    return {"ok": True, **(await _people.write(settings, ws, argv))}


class SelfIn(BaseModel):
    reviewer: str | None = Field(default=None, max_length=80)


@router.post(
    "/{qid}/decline",
    dependencies=_ACTS,
    responses={400: BAD_REQUEST, 403: NOT_THEIRS, 409: CONFLICT},
)
async def decline(
    qid: str,
    body: SelfIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_READ),
) -> dict:
    """ "Not me", said by the person the question is with — nobody declines for someone else."""
    who = _people.person(principal, settings, body.reviewer)
    q = await _people.read(settings, ws, ["questions", "show", "--json", _qid(qid)])
    addressee = str(q.get("addressee") or "")
    if addressee.lower() != who.lower():
        raise HTTPException(
            status_code=403,
            detail="only the person a question is with can say it is not for them; "
            "an operator reassigns it instead",
        )
    argv = people_write.decline_argv(str(q.get("id") or qid), who)
    return {"ok": True, "by": who, **(await _people.write(settings, ws, argv))}


class MuteIn(BaseModel):
    pattern: str = Field(max_length=120)
    reviewer: str | None = Field(default=None, max_length=80)


@router.post("/mute", dependencies=_ACTS, responses={400: BAD_REQUEST, 409: CONFLICT})
async def mute(
    body: MuteIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_READ),
) -> dict:
    who = _people.person(principal, settings, body.reviewer)
    try:
        argv = people_write.mute_argv(body.pattern, who)
    except people_write.ReviewError as exc:
        raise _people.bad_request(exc) from exc
    return {"ok": True, "by": who, **(await _people.write(settings, ws, argv))}
