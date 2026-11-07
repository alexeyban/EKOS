"""Public-demo safety (EKOS RFC 0249): screening a question, a person's remaining allowance, and
the owner's log and unban.

Off unless `moderation_enabled`: every route then answers 404, so a console that is not a public
demo has no such surface. Screening needs a signed-in principal; the log and the unban are the
platform owner's.
"""

from __future__ import annotations

from fastapi import APIRouter, Depends, HTTPException, Query, Request
from pydantic import BaseModel, Field

from .. import authz, models
from ..auth import Principal
from ..authz import allow_platform, authenticated
from ..moderation import Moderator, client_ip
from ._responses import BAD_REQUEST

router = APIRouter(prefix="/moderation", tags=["moderation"])


def _moderator(request: Request) -> Moderator:
    m = getattr(request.app.state, "moderator", None)
    if m is None:
        raise HTTPException(status_code=404, detail="the public-demo checks are not enabled")
    return m


def _subject(p: Principal) -> str:
    """Who is being moderated. A signed-in user is their identity provider's subject; any other
    principal (a token, the development tokens) is itself, so two of them never share a standing."""
    if p.kind == "user":
        return p.subject or p.email or p.id
    return f"{p.kind}:{p.id}"


class ScreenIn(BaseModel):
    text: str = Field(min_length=1, max_length=4000)


@router.post("/screen", responses={404: {"description": "not enabled"}})
async def screen(
    body: ScreenIn,
    request: Request,
    principal: Principal = Depends(authenticated),
    moderator: Moderator = Depends(_moderator),
) -> dict:
    """Decide one question before it is answered. Only `allowed: true` grants anything; every
    refusal carries the reason the person is shown."""
    hops = request.app.state.settings.moderation_trusted_hops
    ip = client_ip(
        request.headers.get("x-forwarded-for", ""),
        request.client.host if request.client else "",
        hops,
    )
    out = await moderator.screen(_subject(principal), ip, body.text)
    return out.as_json()


@router.get("/limits")
async def limits(
    principal: Principal = Depends(authenticated),
    moderator: Moderator = Depends(_moderator),
) -> dict:
    """What the signed-in person has left today."""
    return {"remaining": moderator.remaining(_subject(principal))}


@router.get("/events", dependencies=[Depends(allow_platform)])
async def events(
    moderator: Moderator = Depends(_moderator),
    subject: str | None = Query(default=None, max_length=200),
    limit: int = Query(default=100, ge=1, le=500),
) -> list[dict]:
    """The owner's log: refusals, blocks, bans and lifted bans, newest first."""
    return moderator.events(limit, subject)


@router.get("/restricted", dependencies=[Depends(allow_platform)])
async def restricted(moderator: Moderator = Depends(_moderator)) -> list[dict]:
    """Everyone banned or currently blocked."""
    return moderator.restricted()


class UnbanIn(BaseModel):
    subject: str = Field(min_length=1, max_length=200)


@router.post("/unban", responses={400: BAD_REQUEST})
async def unban(
    body: UnbanIn,
    principal: Principal = Depends(allow_platform),
    moderator: Moderator = Depends(_moderator),
) -> dict:
    """Lift a ban or block and reset the person's counters (the log keeps the history)."""
    if not moderator.unban(body.subject, principal.label):
        raise HTTPException(status_code=400, detail="no such person")
    authz.audit(principal, "moderation: unban", "ok", detail=body.subject)
    return {"ok": True, "subject": body.subject, "standing": moderator.standing_of(body.subject)}


__all__ = ["models", "router"]
