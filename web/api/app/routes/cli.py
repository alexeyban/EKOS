"""Every command from the web (RFC 0186 §1).

`GET  /workspaces/{id}/cli`  the catalogue, with what this person may run here.
`POST /workspaces/{id}/cli`  run one: `{"command": [...], "args": {...}, "confirm": "..."}`.

Who may run what follows the command's class (`catalogue.ACTIONS`): reading needs `read`,
changing the project `operate`, a review `review` (a person, never a token), and anything
destructive or reaching outside the home `admin` plus the command's name typed as a
confirmation. Commands of class `local` (servers, the installer, the console itself) are not
run from the web at all.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Request
from pydantic import BaseModel

from .. import authz, catalogue, models, roots
from ..auth import Principal
from ..commands import CLI
from ..deps import require_workspace
from ..runner import QueueFull
from ..settings import Settings, get_settings
from ._responses import NOT_FOUND, TOO_MANY, UNPROCESSABLE

router = APIRouter(tags=["commands"])

FORBIDDEN = {"description": "Not allowed for this person, or not run from the web"}
CONFLICT = {"description": "Needs the command's name typed as a confirmation"}


class CliRun(BaseModel):
    command: list[str]
    args: dict[str, Any] = {}
    confirm: str | None = None


async def _catalogue(settings: Settings) -> dict[str, Any]:
    try:
        return await catalogue.load(settings.ekos_bin)
    except (RuntimeError, OSError, TimeoutError, ValueError) as exc:
        raise HTTPException(status_code=502, detail=f"cannot read the command list: {exc}") from exc


@router.get("/config-schema")
async def config_schema(
    _: Principal = Depends(authz.authenticated), settings: Settings = Depends(get_settings)
) -> dict:
    """The JSON schema of `ekos.toml` (RFC 0186 §2), from the binary."""
    try:
        return await catalogue.load_schema(settings.ekos_bin)
    except (RuntimeError, OSError, TimeoutError, ValueError) as exc:
        raise HTTPException(
            status_code=502, detail=f"cannot read the config schema: {exc}"
        ) from exc


@router.get("/workspaces/{workspace_id}/cli", responses={404: NOT_FOUND})
async def list_cli(
    ws: models.Workspace = Depends(require_workspace),
    principal: Principal = Depends(authz.authenticated),
    settings: Settings = Depends(get_settings),
) -> dict:
    cat = await _catalogue(settings)
    whole = authz.sees_all_of(principal, ws)
    out = []
    for entry in cat["commands"]:
        if entry["class"] == "local":
            continue
        action = catalogue.ACTIONS[entry["class"]]
        allowed = whole and authz.authorize(principal, action, ws.tenant_id, ws.id)
        out.append({**entry, "allowed": allowed, "confirm": entry["class"] in catalogue.CONFIRMED})
    return {"version": cat.get("version"), "commands": out}


def _allowed_dirs(ws: models.Workspace, settings: Settings) -> list[Path]:
    base = Path(ws.path).resolve()
    tenant = models.get_tenant(ws.tenant_id)
    limits = roots.allowed_roots(tenant, settings) if tenant is not None else []
    # No limit (a single-user install) still keeps paths to the project and nothing else: a
    # command argument is not the place to reach the rest of the disk.
    return [base, *(limits or [])]


@router.post(
    "/workspaces/{workspace_id}/cli",
    responses={403: FORBIDDEN, 404: NOT_FOUND, 409: CONFLICT, 422: UNPROCESSABLE, 429: TOO_MANY},
)
async def run_cli(
    body: CliRun,
    request: Request,
    ws: models.Workspace = Depends(require_workspace),
    principal: Principal = Depends(authz.authenticated),
    settings: Settings = Depends(get_settings),
) -> dict:
    cat = await _catalogue(settings)
    entry = catalogue.find(cat, body.command)
    if entry is None:
        raise HTTPException(status_code=404, detail=f"no command `{' '.join(body.command)}`")
    name = catalogue.describe_command(entry)
    if entry["class"] == "local":
        raise HTTPException(
            status_code=403, detail=f"`{name}` is not run from the web; run it on the server"
        )
    authz.require(
        principal, catalogue.ACTIONS[entry["class"]], ws.tenant_id, ws.id, request=request
    )
    # A command prints what it read, and none takes the caller's read scope (RFC 0181).
    authz.require_all_of(principal, ws)
    if entry["class"] in catalogue.CONFIRMED and (body.confirm or "").strip() != name:
        raise HTTPException(
            status_code=409, detail=f"type `{name}` to confirm: this command is {entry['class']}"
        )
    try:
        argv = catalogue.build_argv(
            entry,
            body.args,
            reviewer=principal.label,
            base=Path(ws.path).resolve(),
            allowed=_allowed_dirs(ws, settings),
        )
    except catalogue.ArgvError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    try:
        run_id = await request.app.state.runner.submit(
            ws.id, ws.path, CLI, {"argv": argv, "label": name}
        )
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    except QueueFull as exc:
        raise HTTPException(status_code=429, detail=str(exc)) from exc
    return {"run_id": run_id, "argv": argv}
