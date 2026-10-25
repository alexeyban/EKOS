"""A project's sources: directories and live databases (RFC 0223, step 1 of the workflow).

Reads need `read`. Adding a source or a database needs `operate`; a route that takes a password
also refuses an API token — credentials come from a signed-in person only. Removing a database
needs `admin`. The password goes to the keychain (`sources_write`), never to `ekos.toml`, the
console database, an argv or a log.
"""

from __future__ import annotations

import json
import tomllib
from pathlib import Path
from typing import Literal

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel, Field

from .. import keychain, models, readproc, roots, semantics_write, sources_write
from ..auth import Principal
from ..authz import allow
from ..deps import require_workspace
from ..settings import Settings, get_settings
from ._responses import BAD_GATEWAY, BAD_REQUEST, CONFLICT

router = APIRouter(prefix="/workspaces/{workspace_id}", tags=["sources"])

_SLOW = 300.0
_OPERATOR = allow("operate")
_ADMIN = allow("admin")
_WRITE = {400: BAD_REQUEST, 409: CONFLICT, 502: BAD_GATEWAY}


def _tenant(ws: models.Workspace) -> models.Tenant:
    tenant = models.get_tenant(ws.tenant_id)
    if tenant is None:
        raise HTTPException(status_code=400, detail=f"tenant {ws.tenant_id!r} does not exist")
    return tenant


def _project_name(ws: models.Workspace) -> str:
    return Path(ws.path).name


def _no_token(principal: Principal) -> None:
    if principal.kind == "token":
        raise HTTPException(
            status_code=403, detail="credentials are entered by a signed-in person, not a token"
        )


async def _run(settings: Settings, ws: models.Workspace, argv: list[str]) -> str:
    try:
        return await semantics_write.review(settings.ekos_bin, ws.path, argv, time_limit=_SLOW)
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    except semantics_write.LedgerBusy as exc:
        raise HTTPException(status_code=409, detail=f"the workspace is busy: {exc}") from exc
    except semantics_write.CliFailed as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc


async def _read(settings: Settings, ws: models.Workspace, argv: list[str]):
    try:
        return await readproc.read_json(settings.ekos_bin, ws.path, argv)
    except readproc.ReadProcError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc


@router.get("/sources", dependencies=[Depends(allow("read"))], responses={502: BAD_GATEWAY})
async def sources(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
) -> dict:
    """The project's directory sources and databases (a database shows its variable's name and
    whether the console holds a password for it — never the password)."""
    dirs = await _read(settings, ws, ["source", "list", "--json"])
    dbs = (await _read(settings, ws, ["database", "list", "--json"]))["databases"]
    granted = set(_tenant(ws).source_env or [])
    for d in dbs:
        d["password_stored"] = d.get("password_env") in granted
        # The console's own process environment says nothing about the tenant's.
        d.pop("password_set", None)
    return {"sources": dirs, "databases": dbs}


class SourceIn(BaseModel):
    name: str = Field(max_length=63)
    path: str = Field(max_length=4096)
    kind: Literal["git", "dir"] | None = None


def _slug(name: str) -> str:
    """A source name from a directory name: `iDempiere Docs` → `idempiere-docs`."""
    out = "".join(c if c.isalnum() and c.isascii() else "-" for c in name.lower())
    out = "-".join(p for p in out.split("-") if p)[:63].strip("-_")
    return out or "main"


def _workspace_root(ws: models.Workspace) -> Path | None:
    """`[workspace] root` of the project's ekos.toml, when it names one."""
    try:
        cfg = tomllib.loads((Path(ws.path) / "ekos.toml").read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return None
    root = cfg.get("workspace", {}).get("root")
    return Path(root) if isinstance(root, str) and root else None


@router.post("/sources", status_code=201, responses=_WRITE)
async def add_source(
    body: SourceIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    _: Principal = Depends(_OPERATOR),
) -> dict:
    """Add a directory under a name. A project created with one unnamed directory gets that
    directory named first (after itself), or EKOS would stop reading it once sources are named."""
    tenant = _tenant(ws)
    path = Path(body.path).expanduser().resolve()
    limits = roots.allowed_roots(tenant, settings)
    if not roots.is_allowed(path, limits):
        raise HTTPException(status_code=400, detail=f"{path} is outside the allowed source roots")
    current = await _read(settings, ws, ["source", "list", "--json"])
    messages = []
    if not current:
        root = _workspace_root(ws)
        if root is not None and root.resolve() != path:
            first = _slug(root.name)
            messages.append(
                await _run(settings, ws, sources_write.source_add_argv(first, str(root), None))
            )
    try:
        argv = sources_write.source_add_argv(body.name, str(path), body.kind)
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    messages.append(await _run(settings, ws, argv))
    return {"ok": True, "message": "\n".join(messages)}


class DatabaseIn(BaseModel):
    name: str = Field(max_length=63)
    connect: str = Field(max_length=300)
    user: str = Field(max_length=128)
    # Sent once; kept in the keychain; never echoed.
    password: str = Field(min_length=1, max_length=1024)
    owners: list[str] = Field(min_length=1, max_length=50)
    profile: Literal["p0", "p1", "p2"] = "p0"
    dictionary: Literal["idempiere"] | None = None


@router.post("/databases", status_code=201, responses=_WRITE)
async def add_database(
    body: DatabaseIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_OPERATOR),
) -> dict:
    """Add an Oracle database: the password to the keychain, its variable's name to ekos.toml."""
    _no_token(principal)
    tenant = _tenant(ws)
    variable = sources_write.secret_name(tenant.id, _project_name(ws), body.name)
    try:
        argv = sources_write.database_add_argv(
            body.name, body.connect, body.user, variable, body.owners, body.profile, body.dictionary
        )
        sources_write.store_password(tenant, variable, body.password)
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    except keychain.Unavailable as exc:
        raise HTTPException(
            status_code=400,
            detail="this server has no keychain to keep the password in: export a variable in the "
            "console's environment and add the database with `ekos database add --password-env`",
        ) from exc
    try:
        message = await _run(settings, ws, argv)
    except HTTPException:
        sources_write.forget_password(_tenant(ws), variable)
        raise
    return {"ok": True, "password_env": variable, "message": message}


class PasswordIn(BaseModel):
    password: str = Field(min_length=1, max_length=1024)


@router.put("/databases/{name}/password", responses=_WRITE)
async def replace_password(
    name: str,
    body: PasswordIn,
    ws: models.Workspace = Depends(require_workspace),
    principal: Principal = Depends(_OPERATOR),
) -> dict:
    """Replace a console-kept password (a rotated credential)."""
    _no_token(principal)
    tenant = _tenant(ws)
    variable = sources_write.secret_name(tenant.id, _project_name(ws), name)
    if variable not in (tenant.source_env or []):
        raise HTTPException(status_code=400, detail=f"the console keeps no password for {name!r}")
    try:
        sources_write.store_password(tenant, variable, body.password)
    except (semantics_write.ReviewError, keychain.Unavailable) as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    return {"ok": True}


@router.post(
    "/databases/{name}/test",
    dependencies=[Depends(_OPERATOR)],
    responses=_WRITE,
)
async def test_database(
    name: str,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
) -> dict:
    """`ekos database test <name> --json`: connect read-only, census, census vs compiled."""
    try:
        sources_write.check_name(name, "database")
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    out = await _run(settings, ws, ["database", "test", name, "--json"])
    try:
        return json.loads(out)
    except json.JSONDecodeError as exc:
        raise HTTPException(status_code=502, detail="the test printed no result") from exc


@router.delete("/databases/{name}", responses=_WRITE)
async def remove_database(
    name: str,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    _: Principal = Depends(_ADMIN),
) -> dict:
    """Remove the entry; a password the console kept for it is deleted too."""
    try:
        sources_write.check_name(name, "database")
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    message = await _run(settings, ws, ["database", "remove", name])
    tenant = _tenant(ws)
    sources_write.forget_password(
        tenant, sources_write.secret_name(tenant.id, _project_name(ws), name)
    )
    return {"ok": True, "message": message}
