"""The interaction map (RFC 0256): who reads and writes each table, declared and observed.

* `GET …/interaction`           the matrix (`ekos interaction matrix --json`, `read`), optionally
  for one table or one actor.
* `GET …/interaction/findings`  unexplained writers, unattributed writes, several writing systems,
  declared writes never observed (`ekos interaction findings --json`, `read`).

Both are reads. Accounting for a database role is a review decision (`ekos review interaction`), on
the Review page like every other.
"""

from __future__ import annotations

from typing import Any

from fastapi import APIRouter, Depends, Query

from .. import authz, models, people_write
from ..deps import require_workspace
from ..settings import Settings, get_settings
from . import _people
from ._responses import BAD_GATEWAY, BAD_REQUEST

router = APIRouter(prefix="/workspaces/{workspace_id}/interaction", tags=["interaction"])


@router.get(
    "",
    dependencies=[Depends(authz.allow("read"))],
    responses={400: BAD_REQUEST, 502: BAD_GATEWAY},
)
async def matrix(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    table: str | None = Query(default=None, max_length=200),
    actor: str | None = Query(default=None, max_length=200),
) -> Any:
    argv = ["interaction", "matrix", "--json"]
    for flag, value in (("--table", table), ("--actor", actor)):
        if isinstance(value, str) and value.strip():
            try:
                argv += [flag, people_write._short(flag[2:], value) or ""]
            except people_write.ReviewError as exc:
                raise _people.bad_request(exc) from exc
    return await _people.read(settings, ws, argv)


@router.get("/findings", dependencies=[Depends(authz.allow("read"))], responses={502: BAD_GATEWAY})
async def findings(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
) -> Any:
    return await _people.read(settings, ws, ["interaction", "findings", "--json"])
