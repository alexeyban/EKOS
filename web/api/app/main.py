"""FastAPI app factory for the EKOS web console (RFC 0127 §8).

uvicorn --factory app.main:create_app
"""

from __future__ import annotations

import logging
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles
from starlette.exceptions import HTTPException as StarletteHTTPException
from starlette.middleware.sessions import SessionMiddleware

from . import home, models
from .routes import (
    auth,
    commands,
    config,
    conflicts,
    evals,
    graph,
    mcp_gateway,
    meta,
    runs,
    schedules,
    semantics,
    stats,
    tenants,
    workspaces,
)
from .runner import JobRunner
from .scheduler import ConsoleScheduler
from .settings import get_settings
from .supervisor import McpSupervisor

# Every API router. `tests/test_authz_routes.py` walks this list, so a router mounted any other
# way would escape the check that each route declares the action it enforces (RFC 0180).
ROUTERS = (
    meta,
    auth,
    commands,
    runs,
    schedules,
    workspaces,
    stats,
    config,
    graph,
    evals,
    semantics,
    conflicts,
    tenants,
)
API_PREFIX = "/api"
# Every mount of routers. The MCP gateway (RFC 0182) is at `/mcp`, outside the API prefix,
# because that is the URL an MCP client is configured with.
MOUNTS = ((API_PREFIX, ROUTERS), ("", (mcp_gateway,)))

_UI_DIST = Path(__file__).resolve().parents[2] / "ui" / "dist"


class _SpaFiles(StaticFiles):
    """The built UI, with client-side routes falling back to `index.html`.

    Plain `StaticFiles(html=True)` answers a reload of `/w/<id>/semantics` with 404: there is no
    such file, the React router owns that path. An unknown non-API path gets the app instead; a
    missing asset (anything with a file extension) and `/api/...` still 404.
    """

    async def get_response(self, path: str, scope):  # type: ignore[override]
        try:
            return await super().get_response(path, scope)
        except StarletteHTTPException as exc:
            last = path.rsplit("/", 1)[-1]
            if exc.status_code != 404 or path.startswith(("api", "mcp")) or "." in last:
                raise
            return await super().get_response("index.html", scope)


log = logging.getLogger("ekos.console")


def _seed_registry_if_empty() -> None:
    """Populate an empty registry from EKOS_CONSOLE_WORKSPACES_JSON — a migration aid for
    Phase 0 Compose setups. Once a row exists this is a no-op."""
    if models.list_workspaces():
        return
    for seed in get_settings().workspace_seeds():
        root = Path(seed.path).expanduser().resolve()
        if (root / "ekos.toml").is_file():
            models.add_workspace(models.Workspace(id=seed.id, name=seed.name, path=str(root)))
        else:
            log.warning("seed workspace %r skipped: %s has no ekos.toml", seed.id, root)


def _register_home_projects() -> None:
    """Register every project of a known tenant that the registry does not hold (RFC 0179
    §2): a project exists because its directory does, so one created with the CLI in the same
    home appears here, and an emptied registry is rebuilt."""
    root = home.configured_home()
    if root is None:
        return
    known = {ws.path for ws in models.list_workspaces()}
    # Only tenants the console has a row for: a directory under data/ is not a tenant until a
    # platform owner creates it (RFC 0180).
    for tenant in models.list_tenants():
        for project in home.projects_in(root, tenant.id):
            if str(project.dir) in known:
                continue
            workspace_id = workspaces.workspace_id_for(tenant.id, project.name)
            if models.get_workspace(workspace_id) is not None:
                log.warning("project %r not registered: the id is taken", workspace_id)
                continue
            models.add_workspace(
                models.Workspace(
                    id=workspace_id, name=project.name, path=str(project.dir), tenant_id=tenant.id
                )
            )


@asynccontextmanager
async def _lifespan(app: FastAPI):
    settings = get_settings()
    models.init_engine(settings.console_db)
    _seed_registry_if_empty()
    _register_home_projects()

    app.state.supervisor = McpSupervisor(settings)
    # RFC 0182: no server is started here. Each starts when a page or a gateway call first needs
    # it, and stops again when idle.
    await app.state.supervisor.start([])

    app.state.runner = JobRunner(settings)
    app.state.runner.start()

    app.state.scheduler = ConsoleScheduler(app.state.runner)
    app.state.scheduler.start()
    try:
        yield
    finally:
        await app.state.scheduler.aclose()
        await app.state.runner.aclose()
        await app.state.supervisor.aclose()


def create_app() -> FastAPI:
    settings = get_settings()
    app = FastAPI(title="EKOS Console API", version="0.1.0", lifespan=_lifespan)
    app.state.settings = settings

    app.add_middleware(
        SessionMiddleware,
        secret_key=settings.session_secret,
        same_site="lax",
        https_only=False,
    )
    app.add_middleware(
        CORSMiddleware,
        allow_origins=[settings.dev_origin],
        allow_credentials=True,
        allow_methods=["*"],
        allow_headers=["*"],
    )

    for prefix, routers in MOUNTS:
        for r in routers:
            app.include_router(r.router, prefix=prefix)

    # Serve the built UI when it exists (Compose / production); the Vite dev server handles it
    # otherwise.
    if _UI_DIST.is_dir():
        app.mount("/", _SpaFiles(directory=_UI_DIST, html=True), name="ui")

    return app
