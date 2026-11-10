"""Channels (RFC 0253 §3.2, §3.8): account links, channel → project bindings, the digest, and the
inbound endpoints Slack and Teams post to.

* `POST   /channels/link`             a signed-in person enters the code a bot gave them.
* `GET    /channels/me`               their linked accounts; `DELETE /channels/me/{id}` unlinks.
* `GET    /channels`                  (owner) which channels run, every link and binding.
* `POST   /channels/bindings`         (owner) bind a Slack/Teams channel or Telegram group.
* `DELETE /channels/bindings/{id}`    (owner).
* `POST   /channels/digest`           (owner) send the questions digest now (RFC 0254 §3.3).
* `POST   /channels/slack/events`, `POST /channels/slack/interactivity` — Slack's signed
  requests; `POST /channels/teams/messages` — the Bot Framework's, with its JWT. These carry no
  console credential: the platform's signature is the credential, checked before anything else.
"""

from __future__ import annotations

import json

from fastapi import APIRouter, Depends, Header, HTTPException, Request
from fastapi.responses import JSONResponse, Response
from pydantic import BaseModel, Field

from .. import authz, models
from ..auth import Principal
from ..channels import actions, slack, teams
from ..channels.core import CHANNELS, ButtonClick
from ..channels.service import configured
from ..settings import Settings, get_settings
from ._responses import BAD_REQUEST, NOT_FOUND

router = APIRouter(prefix="/channels", tags=["channels"])

UNAUTHORIZED = {"description": "Not the channel platform's signed request"}


def _service(request: Request):
    return getattr(request.app.state, "channels", None)


class LinkIn(BaseModel):
    code: str = Field(min_length=4, max_length=20)


@router.post("/link", responses={400: BAD_REQUEST})
async def link(body: LinkIn, principal: Principal = Depends(authz.authenticated)) -> dict:
    """RFC 0253 §3.2: the code `/ekos link` gave, entered by the person while signed in."""
    try:
        account = actions.complete_link(principal, body.code)
    except actions.ActionRefused as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    return {"ok": True, "id": account.id, "channel": account.channel, "label": account.label}


def _account_row(a: models.ChannelAccount) -> dict:
    return {
        "id": a.id,
        "channel": a.channel,
        "account": a.account,
        "label": a.label,
        "default_workspace": a.default_workspace,
        "linked_at": a.linked_at.isoformat(),
    }


@router.get("/me")
async def my_accounts(principal: Principal = Depends(authz.authenticated)) -> list[dict]:
    return [_account_row(a) for a in models.channel_accounts_of(principal.kind, principal.id)]


@router.delete("/me/{account_id}", responses={404: NOT_FOUND})
async def unlink(account_id: str, principal: Principal = Depends(authz.authenticated)) -> dict:
    mine = {a.id for a in models.channel_accounts_of(principal.kind, principal.id)}
    if account_id not in mine:
        raise HTTPException(status_code=404, detail="no such linked account of yours")
    models.delete_channel_account(account_id)
    authz.audit(principal, "channels: unlink", "ok", detail=account_id)
    return {"ok": True}


@router.get("")
async def overview(
    _: Principal = Depends(authz.allow_platform), settings: Settings = Depends(get_settings)
) -> dict:
    return {
        "configured": configured(settings),
        "public": settings.public,
        "digest_cron": settings.channels_digest_cron,
        "accounts": [_account_row(a) for a in models.list_channel_accounts()],
        "bindings": [
            {
                "id": b.id,
                "channel": b.channel,
                "conversation": b.conversation,
                "workspace_id": b.workspace_id,
                "created_by": b.created_by,
            }
            for b in models.list_channel_bindings()
        ],
    }


class BindingIn(BaseModel):
    channel: str
    conversation: str = Field(min_length=1, max_length=200)
    workspace_id: str = Field(min_length=1, max_length=200)


@router.post("/bindings", responses={400: BAD_REQUEST})
async def bind(body: BindingIn, principal: Principal = Depends(authz.allow_platform)) -> dict:
    if body.channel not in CHANNELS:
        raise HTTPException(status_code=400, detail=f"channel is one of {', '.join(CHANNELS)}")
    if models.get_workspace(body.workspace_id) is None:
        raise HTTPException(status_code=400, detail=f"no workspace {body.workspace_id!r}")
    row = models.ChannelBinding(
        id=f"{body.channel}:{body.conversation}",
        channel=body.channel,
        conversation=body.conversation,
        workspace_id=body.workspace_id,
        created_by=principal.label,
    )
    with models.session() as s:
        s.merge(row)
        s.commit()
    return {"ok": True, "id": row.id}


@router.delete("/bindings/{binding_id}", responses={404: NOT_FOUND})
async def unbind(binding_id: str, _: Principal = Depends(authz.allow_platform)) -> dict:
    if not models.delete_channel_binding(binding_id):
        raise HTTPException(status_code=404, detail="no such binding")
    return {"ok": True}


@router.post("/digest", responses={409: {"description": "No channel runs"}})
async def digest_now(request: Request, _: Principal = Depends(authz.allow_platform)) -> dict:
    service = _service(request)
    if service is None:
        raise HTTPException(status_code=409, detail="no channel is configured")
    return await service.digest()


# ── inbound: Slack ───────────────────────────────────────────────────────────


def _slack(request: Request):
    service = _service(request)
    adapter = service.adapters.get("slack") if service else None
    if adapter is None or not adapter.signing_secret:
        raise HTTPException(status_code=404, detail="the Slack Events API is not configured")
    return service, adapter


async def _signed_body(request: Request, adapter) -> bytes:
    body = await request.body()
    ok = slack.verify(
        adapter.signing_secret,
        request.headers.get("x-slack-request-timestamp", ""),
        body,
        request.headers.get("x-slack-signature", ""),
    )
    if not ok:
        raise HTTPException(status_code=401, detail="bad signature")
    return body


@router.post("/slack/events", responses={401: UNAUTHORIZED, 404: NOT_FOUND})
async def slack_events(request: Request) -> Response:
    service, adapter = _slack(request)
    body = await _signed_body(request, adapter)
    try:
        payload = json.loads(body)
    except json.JSONDecodeError as exc:
        raise HTTPException(status_code=400, detail="not JSON") from exc
    if payload.get("type") == "url_verification":
        return JSONResponse({"challenge": str(payload.get("challenge", ""))})
    msg = slack.parse_event(payload, bot_user=adapter.bot_user)
    # Slack resends an event it thinks was not received; the first delivery is being handled.
    if msg is not None and not request.headers.get("x-slack-retry-num"):
        service.spawn(service.on_message(adapter, msg))
    return Response(status_code=200)


@router.post("/slack/interactivity", responses={401: UNAUTHORIZED, 404: NOT_FOUND})
async def slack_interactivity(request: Request) -> Response:
    service, adapter = _slack(request)
    body = await _signed_body(request, adapter)
    click = slack.parse_action(slack.form_payload(body))
    if click is not None:
        service.spawn(service.on_click(adapter, click))
    return Response(status_code=200)


# ── inbound: Teams ───────────────────────────────────────────────────────────


@router.post("/teams/messages", responses={401: UNAUTHORIZED, 404: NOT_FOUND})
async def teams_messages(request: Request, authorization: str = Header(default="")) -> Response:
    service = _service(request)
    adapter = service.adapters.get("teams") if service else None
    if adapter is None:
        raise HTTPException(status_code=404, detail="Teams is not configured")
    try:
        activity = json.loads(await request.body())
    except json.JSONDecodeError as exc:
        raise HTTPException(status_code=400, detail="not JSON") from exc
    try:
        await adapter.verify_token(authorization, activity)
    except teams.TeamsAuthError as exc:
        raise HTTPException(status_code=401, detail=str(exc)) from exc
    event = teams.parse_activity(activity)
    if isinstance(event, ButtonClick):
        text = await service.on_click(adapter, event)
        return JSONResponse(teams.invoke_response(text))
    if event is not None:
        service.spawn(service.on_message(adapter, event))
    return Response(status_code=200)
