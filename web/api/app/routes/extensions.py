"""Workspace pages declared by extensions (RFC 0198).

`GET /extensions` the pages the console's binary declares (any signed-in person).
`GET /workspaces/{id}/extensions/{ext}/{page}/{panel}` one panel's data: its read command, run in
the workspace, with the page selections it declares as query parameters (RFC 0207). Actions are not here: a page's buttons go through `POST /workspaces/{id}/cli`, with
the command's own class, role and confirmation (RFC 0186).
"""

from __future__ import annotations

from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Request

from .. import authz, catalogue, extensions, models, readproc
from ..authz import allow
from ..deps import require_workspace
from ..settings import Settings, get_settings
from ._responses import BAD_GATEWAY, NOT_FOUND, UNPROCESSABLE

router = APIRouter(tags=["extensions"])

_BIG = 16 << 20


@router.get("/extensions", dependencies=[Depends(authz.authenticated)])
async def list_pages(settings: Settings = Depends(get_settings)) -> dict[str, Any]:
    """Every extension page, with its panels and actions."""
    spec = await extensions.load(settings.ekos_bin)
    return {"pages": extensions.pages(spec)}


@router.get(
    "/workspaces/{workspace_id}/extensions/{ext}/{page_id}/{panel_id}",
    dependencies=[Depends(allow("read"))],
    responses={404: NOT_FOUND, 422: UNPROCESSABLE, 502: BAD_GATEWAY},
)
async def panel_data(
    ext: str,
    page_id: str,
    panel_id: str,
    request: Request,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
) -> Any:
    """The JSON the panel's command prints in this workspace."""
    spec = await extensions.load(settings.ekos_bin)
    panel = extensions.find_panel(spec, ext, page_id, panel_id)
    if panel is None:
        raise HTTPException(status_code=404, detail=f"no panel {ext}/{page_id}/{panel_id}")
    cat = await catalogue.load(settings.ekos_bin)
    selections = {k: v for k, v in request.query_params.items()}
    try:
        argv = extensions.panel_argv(cat, panel, selections)
    except extensions.SelectionError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    except extensions.PanelError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc
    try:
        return await readproc.read_checked_json(settings.ekos_bin, ws.path, argv, max_output=_BIG)
    except readproc.ReadProcError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc
