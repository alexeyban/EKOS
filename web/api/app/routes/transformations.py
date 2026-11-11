"""Transformations drawn as a chain of steps (RFC 0257).

* `GET /workspaces/{id}/transformations` — the views, routines and queries with a recovered
  Transformation IR graph (`ekos transformation list --json`, `read`).
* `GET /workspaces/{id}/transformations/graph?object=<id>` — one of them as a chain
  (`ekos transformation graph --json`, `read`): steps, edges, the template sentence per step,
  columns, evidence, annotations and the step ↔ source-text map.

Both are reads. Annotating a step is the existing annotation route (`POST
/workspaces/{id}/annotations` with the step's node id as the target, RFC 0247), which is the
human-only `ekos annotate add`. Neither route serves a reader who sees only part of a workspace:
the CLI is not run under a read scope, so they are not marked `scoped` (agents get the same chain
from the scoped MCP tool `ekos_transformation_graph`).
"""

from __future__ import annotations

import re

from fastapi import APIRouter, Depends, HTTPException, Query

from .. import models, readproc
from ..authz import allow
from ..deps import require_workspace
from ..settings import Settings, get_settings
from ._responses import BAD_GATEWAY, BAD_REQUEST

router = APIRouter(prefix="/workspaces/{workspace_id}/transformations", tags=["transformations"])

_UUID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
# `all`, or 1-based statement numbers: `3` or `1,4,9`.
_EXPAND = re.compile(r"^(all|[1-9][0-9]{0,4}(,[1-9][0-9]{0,4}){0,49})$")
_FILTER = re.compile(r"^[A-Za-z0-9_.$ -]{1,80}$")
_BIG = 32 << 20


async def _read(settings: Settings, ws: models.Workspace, argv: list[str]) -> dict:
    try:
        out = await readproc.read_json(
            settings.ekos_bin, ws.path, argv, max_output=_BIG, time_limit=60.0
        )
    except readproc.ReadProcError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc
    if not isinstance(out, dict):
        raise HTTPException(status_code=502, detail="unexpected output from `ekos transformation`")
    return out


@router.get(
    "", dependencies=[Depends(allow("read"))], responses={400: BAD_REQUEST, 502: BAD_GATEWAY}
)
async def transformations(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    filter: str | None = Query(default=None, max_length=80),
    all: bool = False,
    limit: int = Query(default=500, ge=1, le=5000),
) -> dict:
    """`{total, hidden, transformations: [{key, name, kind, object, steps, unmapped}]}`."""
    argv = ["transformation", "list", "--json"] + (["--all"] if all else [])
    if filter:
        if not _FILTER.match(filter):
            raise HTTPException(status_code=400, detail="filter: letters, digits, `_ . $ -`")
        argv += ["--filter", filter]
    argv += ["--limit", str(limit)]
    return await _read(settings, ws, argv)


@router.get(
    "/graph", dependencies=[Depends(allow("read"))], responses={400: BAD_REQUEST, 502: BAD_GATEWAY}
)
async def transformation_graph(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    object: str = Query(max_length=36),
    expand: str | None = Query(default=None, max_length=300),
) -> dict:
    """The `ekos.transformation-graph/1` document for a TransformNode, View or Procedure id."""
    if not _UUID.match(object):
        raise HTTPException(status_code=400, detail="object must be an id")
    argv = ["transformation", "graph", "--json", "--object", object]
    if expand:
        if not _EXPAND.match(expand):
            raise HTTPException(status_code=400, detail="expand: `all` or statement numbers")
        argv += ["--expand", expand]
    return await _read(settings, ws, argv)
