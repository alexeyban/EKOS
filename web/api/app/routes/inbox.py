"""The inbox (RFC 0253 §3.3): what people sent — files and links — held for the next build.

* `GET`    the items, pending and accepted (`ekos inbox list --json`, `read`).
* `POST`   send a file (base64) or a link, as the signed-in person (`ekos inbox add --as`). Anyone
  who may read the project may send; what a person without the `review` right sends waits as
  `pending` until a reviewer accepts it. Never from an API token, never in a read-only showcase
  or a public demo.
* `POST …/{item}/accept` the human-only `ekos inbox accept --as <you>` (`review`).
* `POST …/{item}/purge`  the human-only `ekos inbox purge --as <you>` (`admin`, the item's id
  typed as a confirmation): deletes the original; the redacted text already compiled stays.
"""

from __future__ import annotations

import asyncio
import base64
import binascii
import contextlib
import re
import shutil
import tempfile
from pathlib import Path
from typing import Literal

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel, Field

from .. import authz, models, people_write
from ..auth import Principal
from ..deps import require_workspace
from ..settings import Settings, get_settings
from . import _people
from ._responses import BAD_GATEWAY, BAD_REQUEST, CONFLICT

router = APIRouter(prefix="/workspaces/{workspace_id}/inbox", tags=["inbox"])

_READ = authz.allow("read")
_REVIEW = authz.allow("review")
_ADMIN = authz.allow("admin")

FORBIDDEN = {"description": "An API token, a read-only showcase or a public demo"}
_SAFE = re.compile(r"[^A-Za-z0-9._-]")


@router.get("", dependencies=[Depends(authz.allow("read"))], responses={502: BAD_GATEWAY})
async def inbox(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    status: Literal["pending", "accepted"] | None = None,
) -> list[dict]:
    argv = ["inbox", "list", "--json"]
    if status:
        argv += ["--status", status]
    return await _people.read(settings, ws, argv)


class SendIn(BaseModel):
    # A file: its name and its bytes, base64. Or a link (http/https).
    name: str | None = Field(default=None, max_length=200)
    content_base64: str | None = None
    url: str | None = Field(default=None, max_length=2000)
    # Token mode only: who is sending (recorded as `token:<name>`). Ignored when people sign in.
    reviewer: str | None = Field(default=None, max_length=80)


def _decode(body: SendIn, limit_mb: int) -> bytes:
    try:
        data = base64.b64decode(body.content_base64 or "", validate=True)
    except (binascii.Error, ValueError) as exc:
        raise HTTPException(status_code=400, detail="the file is not valid base64") from exc
    if not data:
        raise HTTPException(status_code=400, detail="the file is empty")
    if len(data) > limit_mb * 1024 * 1024:
        raise HTTPException(status_code=400, detail=f"the file is larger than {limit_mb} MB")
    return data


def may_send(settings: Settings) -> None:
    """A read-only showcase acts on nothing; a public demo takes nothing from strangers."""
    if settings.public:
        raise HTTPException(
            status_code=403, detail="this console does not take material (a public deployment)"
        )


def pending_for(principal: Principal, ws: models.Workspace) -> bool:
    """RFC 0253 §3.2: material from someone who may not accept it waits for someone who may."""
    return not authz.authorize(principal, "review", ws.tenant_id, ws.id)


async def store_bytes(
    settings: Settings,
    ws: models.Workspace,
    name: str,
    data: bytes,
    by: str,
    *,
    channel: str,
    permalink: str | None,
    pending: bool,
) -> dict:
    """Write `data` to a private temporary directory under its (cleaned) name, hand the path to
    `ekos inbox add`, and remove it: the inbox keeps its own copy."""
    clean = _SAFE.sub("_", Path(name).name).lstrip(".") or "file"
    tmp = Path(tempfile.mkdtemp(prefix="ekos-inbox-"))
    try:
        path = tmp / clean[:120]
        await asyncio.to_thread(path.write_bytes, data)
        argv = people_write.inbox_add_argv(
            str(path), by, channel=channel, permalink=permalink, pending=pending
        )
        return await _people.write(settings, ws, argv)
    finally:
        with contextlib.suppress(OSError):
            shutil.rmtree(tmp)


async def store_link(
    settings: Settings,
    ws: models.Workspace,
    url: str,
    by: str,
    *,
    channel: str,
    permalink: str | None,
    pending: bool,
) -> dict:
    if not people_write.is_link(url):
        raise HTTPException(status_code=400, detail="a link is an http(s) address")
    argv = people_write.inbox_add_argv(
        url.strip(), by, channel=channel, permalink=permalink, pending=pending
    )
    return await _people.write(settings, ws, argv)


@router.post(
    "",
    dependencies=[Depends(authz.not_read_only)],
    responses={400: BAD_REQUEST, 403: FORBIDDEN, 409: CONFLICT},
)
async def send(
    body: SendIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_READ),
) -> dict:
    may_send(settings)
    who = _people.person(principal, settings, body.reviewer)
    pending = pending_for(principal, ws)
    if body.url and not body.content_base64:
        return await store_link(
            settings, ws, body.url, who, channel="console", permalink=None, pending=pending
        )
    if not body.name or not body.content_base64:
        raise HTTPException(status_code=400, detail="send a file (name and content) or a link")
    data = _decode(body, settings.inbox_upload_max_mb)
    return await store_bytes(
        settings, ws, body.name, data, who, channel="console", permalink=None, pending=pending
    )


class DecideIn(BaseModel):
    reviewer: str | None = Field(default=None, max_length=80)
    # Purge only: the item's id, typed.
    confirm: str | None = Field(default=None, max_length=80)


@router.post("/{item}/accept", responses={400: BAD_REQUEST, 409: CONFLICT})
async def accept(
    item: str,
    body: DecideIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_REVIEW),
) -> dict:
    who = _people.person(principal, settings, body.reviewer)
    try:
        argv = people_write.inbox_accept_argv(item, who)
    except people_write.ReviewError as exc:
        raise _people.bad_request(exc) from exc
    out = await _people.write(settings, ws, argv)
    return {"ok": True, "by": who, **out}


@router.post("/{item}/purge", responses={400: BAD_REQUEST, 409: CONFLICT})
async def purge(
    item: str,
    body: DecideIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_ADMIN),
) -> dict:
    if (body.confirm or "").strip() != item:
        raise HTTPException(status_code=409, detail=f"type the item's id ({item}) to purge it")
    who = _people.person(principal, settings, body.reviewer)
    try:
        argv = people_write.inbox_purge_argv(item, who)
    except people_write.ReviewError as exc:
        raise _people.bad_request(exc) from exc
    out = await _people.write(settings, ws, argv)
    return {"ok": True, "by": who, **out}
