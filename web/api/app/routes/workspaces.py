"""Workspace registry + per-workspace MCP-server status (RFC 0129 §4)."""

from __future__ import annotations

import asyncio
from pathlib import Path

from fastapi import APIRouter, Depends, HTTPException

from .. import _proc, home, models
from ..auth import require_role
from ..deps import get_supervisor
from ..schemas import ServerStatus, WorkspaceCreate, WorkspaceOut
from ..settings import Settings, get_settings
from ..supervisor import McpSupervisor
from ._responses import BAD_REQUEST, CONFLICT, NOT_FOUND

router = APIRouter(
    prefix="/workspaces", tags=["workspaces"], dependencies=[Depends(require_role("read"))]
)


def _out(ws: models.Workspace, supervisor: McpSupervisor) -> WorkspaceOut:
    handle = supervisor.handle(ws.id)
    server = ServerStatus(**handle.status()) if handle is not None else None
    return WorkspaceOut(id=ws.id, name=ws.name, path=ws.path, server=server)


# `project add --detect` walks the source directory once; a large tree takes a while.
_MANAGE_TIMEOUT = 120.0


def _inside(path: Path, allowed: str) -> bool:
    limit = Path(allowed).expanduser().resolve()
    return path == limit or limit in path.parents


def _existing_workspace(raw: str, settings: Settings) -> Path:
    """Validate a directory that already holds a workspace, in place or in the home."""
    root = Path(raw).expanduser().resolve()
    project = home.project_at(str(root))
    # The workspaces root limits which sources may be read. A project directory is the console's
    # own storage, and its sources were checked when it was created.
    if project is None and settings.workspaces_root and not _inside(root, settings.workspaces_root):
        allowed_root = Path(settings.workspaces_root).expanduser().resolve()
        raise HTTPException(
            status_code=400,
            detail=f"{root} is outside the configured workspaces root {allowed_root}",
        )
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


async def _create_project(name: str, source: str, settings: Settings) -> Path:
    """`ekos project add <name> --source <dir> --detect` in the configured home (RFC 0179 §2)."""
    root = home.configured_home()
    if root is None:
        raise HTTPException(
            status_code=400, detail="this console has no EKOS home (EKOS_HOME is not set)"
        )
    if not home.is_valid_name(name):
        raise HTTPException(
            status_code=400,
            detail="a project id is 1-63 lowercase letters, digits, `-` or `_`, "
            "starting with a letter or digit",
        )
    src = Path(source).expanduser().resolve()
    if settings.workspaces_root and not _inside(src, settings.workspaces_root):
        allowed_root = Path(settings.workspaces_root).expanduser().resolve()
        raise HTTPException(
            status_code=400,
            detail=f"{src} is outside the configured workspaces root {allowed_root}",
        )
    if not src.is_dir():
        raise HTTPException(status_code=400, detail=f"{src} is not a directory")

    project = home.Project(home=root, tenant=home.DEFAULT_TENANT, name=name)
    if project.config_path.exists():
        raise HTTPException(status_code=409, detail=f"project {name!r} already exists")
    await _ekos(
        settings,
        ["--home", str(root), "project", "add", name, "--source", str(src), "--detect"],
    )
    return project.dir


@router.get("", response_model=list[WorkspaceOut])
async def list_workspaces(
    supervisor: McpSupervisor = Depends(get_supervisor),
) -> list[WorkspaceOut]:
    return [_out(ws, supervisor) for ws in models.list_workspaces()]


@router.post(
    "",
    response_model=WorkspaceOut,
    status_code=201,
    dependencies=[Depends(require_role("write"))],
    responses={400: BAD_REQUEST, 409: CONFLICT},
)
async def register_workspace(
    body: WorkspaceCreate,
    supervisor: McpSupervisor = Depends(get_supervisor),
    settings: Settings = Depends(get_settings),
) -> WorkspaceOut:
    if models.get_workspace(body.id) is not None:
        raise HTTPException(status_code=409, detail=f"workspace {body.id!r} already registered")

    if body.source is not None:
        root = await _create_project(body.id, body.source, settings)
    else:
        root = _existing_workspace(body.path or "", settings)

    ws = models.Workspace(id=body.id, name=body.name, path=str(root))
    models.add_workspace(ws)
    await supervisor.ensure(ws)
    return _out(ws, supervisor)


@router.delete(
    "/{workspace_id}",
    status_code=204,
    dependencies=[Depends(require_role("write"))],
    responses={404: NOT_FOUND},
)
async def deregister_workspace(
    workspace_id: str,
    supervisor: McpSupervisor = Depends(get_supervisor),
    settings: Settings = Depends(get_settings),
) -> None:
    if models.get_workspace(workspace_id) is None:
        raise HTTPException(status_code=404, detail=f"unknown workspace {workspace_id!r}")
    ws = models.get_workspace(workspace_id)
    await supervisor.stop(workspace_id)
    project = home.project_at(ws.path) if ws is not None else None
    if project is not None and project.dir.is_dir():
        # A project lives only in the home: deregistering it alone would bring it back at the
        # next start. `project remove` keeps the data, in the home's trash.
        selector = ["--home", str(project.home), "--tenant", project.tenant]
        await _ekos(settings, [*selector, "project", "remove", project.name])
    models.delete_workspace(workspace_id)
