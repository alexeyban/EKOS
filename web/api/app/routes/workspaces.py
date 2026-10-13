"""Workspace registry + per-workspace MCP-server status (RFC 0129 §4, RFC 0179, RFC 0180)."""

from __future__ import annotations

import asyncio
from pathlib import Path

from fastapi import APIRouter, Depends, HTTPException, Request

from .. import _proc, authz, home, models, roots
from ..auth import Principal
from ..deps import get_supervisor
from ..schemas import ServerStatus, WorkspaceCreate, WorkspaceOut
from ..settings import Settings, get_settings
from ..supervisor import McpSupervisor
from ._responses import BAD_REQUEST, CONFLICT, NOT_FOUND

router = APIRouter(prefix="/workspaces", tags=["workspaces"])

# `project add --detect` walks the source directory once; a large tree takes a while.
_MANAGE_TIMEOUT = 120.0


def workspace_id_for(tenant_id: str, name: str) -> str:
    """A name cannot contain a dot, so `<tenant>.<name>` never collides with a `default` id."""
    return name if tenant_id == models.DEFAULT_TENANT else f"{tenant_id}.{name}"


def _out(ws: models.Workspace, supervisor: McpSupervisor, principal: Principal) -> WorkspaceOut:
    handle = supervisor.handle(ws.id)
    server = ServerStatus(**handle.status()) if handle is not None else None
    return WorkspaceOut(
        id=ws.id,
        name=ws.name,
        path=ws.path,
        tenant_id=ws.tenant_id,
        roles=sorted(authz.roles_in(principal, ws.tenant_id, ws.id)),
        restricted=authz.is_restricted(principal, ws),
        server=server,
    )


def _refuse_outside(path: Path, limits: list[Path] | None) -> None:
    if not roots.is_allowed(path, limits):
        if limits and len(limits) == 1:
            detail = f"{path} is outside the configured workspaces root {limits[0]}"
        else:
            detail = f"{path} is outside the allowed source roots: {roots.describe(limits)}"
        raise HTTPException(status_code=400, detail=detail)


def _existing_workspace(raw: str, tenant: models.Tenant, settings: Settings) -> Path:
    """Validate a directory that already holds a workspace, in place or in the home."""
    root = Path(raw).expanduser().resolve()
    project = home.project_at(str(root))
    if project is not None:
        # A project directory is the console's own storage; its sources were checked when it
        # was created. It must be this tenant's.
        if project.tenant != tenant.id:
            raise HTTPException(status_code=400, detail=f"{root} is not a project of this tenant")
    else:
        _refuse_outside(root, roots.allowed_roots(tenant, settings))
    if not (root / "ekos.toml").is_file():
        raise HTTPException(status_code=400, detail=f"{root} has no ekos.toml")
    if project is not None:
        if not project.state_dir.is_dir():
            raise HTTPException(status_code=400, detail=f"{root} has no state/")
    elif not (root / ".ekos").is_dir():
        raise HTTPException(
            status_code=400, detail=f"{root} has no .ekos/ — run the pipeline there first"
        )
    return root


async def _ekos(settings: Settings, argv: list[str]) -> None:
    """Run one `ekos` management command; a failure is the caller's 400 with the CLI's reason."""
    try:
        proc = await _proc.spawn([settings.ekos_bin, *argv])
    except OSError as exc:
        raise HTTPException(status_code=502, detail=f"could not run ekos: {exc}") from exc
    try:
        _out, err = await asyncio.wait_for(proc.communicate(), timeout=_MANAGE_TIMEOUT)
    except TimeoutError:
        await _proc.terminate(proc)
        raise HTTPException(status_code=502, detail="the ekos command timed out") from None
    if proc.returncode != 0:
        reason = _proc.strip_ansi(err.decode(errors="replace")).strip()[-500:]
        raise HTTPException(status_code=400, detail=reason or f"ekos exited {proc.returncode}")


async def _create_project(
    name: str, source: str, tenant: models.Tenant, settings: Settings
) -> Path:
    """`ekos project add <name> --source <dir> --detect` in the configured home (RFC 0179 §2)."""
    root = home.configured_home()
    if root is None:
        raise HTTPException(
            status_code=400, detail="this console has no EKOS home (EKOS_HOME is not set)"
        )
    src = Path(source).expanduser().resolve()
    _refuse_outside(src, roots.allowed_roots(tenant, settings))
    if not src.is_dir():
        raise HTTPException(status_code=400, detail=f"{src} is not a directory")

    project = home.Project(home=root, tenant=tenant.id, name=name)
    if project.config_path.exists():
        raise HTTPException(status_code=409, detail=f"project {name!r} already exists")
    selector = ["--home", str(root), "--tenant", tenant.id]
    await _ekos(settings, [*selector, "project", "add", name, "--source", str(src), "--detect"])
    return project.dir


@router.get("", response_model=list[WorkspaceOut])
async def list_workspaces(
    supervisor: McpSupervisor = Depends(get_supervisor),
    principal: Principal = Depends(authz.authenticated),
) -> list[WorkspaceOut]:
    return [
        _out(ws, supervisor, principal)
        for ws in models.list_workspaces()
        if authz.sees_workspace(principal, ws)
    ]


@router.post(
    "",
    response_model=WorkspaceOut,
    status_code=201,
    responses={400: BAD_REQUEST, 409: CONFLICT},
)
async def register_workspace(
    body: WorkspaceCreate,
    request: Request,
    supervisor: McpSupervisor = Depends(get_supervisor),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(authz.authenticated),
) -> WorkspaceOut:
    # Adding a workspace to a tenant is that tenant's `admin`.
    tenant = authz.tenant_for(principal, body.tenant)
    authz.require(principal, "admin", tenant.id, request=request)

    named = body.source is not None or tenant.id != models.DEFAULT_TENANT
    if named and not home.is_valid_name(body.id):
        raise HTTPException(
            status_code=400,
            detail="a project id is 1-63 lowercase letters, digits, `-` or `_`, "
            "starting with a letter or digit",
        )
    workspace_id = workspace_id_for(tenant.id, body.id)
    if models.get_workspace(workspace_id) is not None:
        raise HTTPException(status_code=409, detail=f"workspace {body.id!r} already registered")

    if body.source is not None:
        root = await _create_project(body.id, body.source, tenant, settings)
    else:
        root = _existing_workspace(body.path or "", tenant, settings)

    ws = models.Workspace(id=workspace_id, name=body.name, path=str(root), tenant_id=tenant.id)
    models.add_workspace(ws)
    await supervisor.ensure(ws)
    return _out(ws, supervisor, principal)


@router.delete(
    "/{workspace_id}",
    status_code=204,
    dependencies=[Depends(authz.allow("admin"))],
    responses={404: NOT_FOUND},
)
async def deregister_workspace(
    workspace_id: str,
    supervisor: McpSupervisor = Depends(get_supervisor),
    settings: Settings = Depends(get_settings),
) -> None:
    ws = models.get_workspace(workspace_id)
    if ws is None:
        raise HTTPException(status_code=404, detail=f"unknown workspace {workspace_id!r}")
    await supervisor.stop(workspace_id)
    project = home.project_at(ws.path)
    if project is not None and project.dir.is_dir():
        # A project lives only in the home: deregistering it alone would bring it back at the
        # next start. `project remove` keeps the data, in the home's trash.
        selector = ["--home", str(project.home), "--tenant", project.tenant]
        await _ekos(settings, [*selector, "project", "remove", project.name])
    models.delete_workspace(workspace_id)
