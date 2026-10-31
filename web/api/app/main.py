"""FastAPI app factory for the EKOS web console (RFC 0127 §8).

uvicorn --factory app.main:create_app
"""

from __future__ import annotations

import asyncio
import contextlib
import ipaddress
import logging
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles
from starlette.exceptions import HTTPException as StarletteHTTPException
from starlette.middleware.sessions import SessionMiddleware
from starlette.responses import JSONResponse

from . import cloud, home_projects, lifecycle, models
from .routes import (
    auth,
    chat,
    cli,
    commands,
    config,
    conflicts,
    evals,
    extensions,
    graph,
    integrity,
    mcp_gateway,
    meta,
    platform,
    review,
    runs,
    schedules,
    semantics,
    sources,
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
    integrity,
    review,
    sources,
    tenants,
    platform,
    cli,
    chat,
    extensions,
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
    missing file and `/api/...` still 404.

    A file of the UI is under `assets/` or directly at the root (`logo.png`). A dot elsewhere
    says nothing: the page of a workspace in a tenant is `/w/<tenant>.<project>`.
    """

    async def get_response(self, path: str, scope):  # type: ignore[override]
        try:
            return await super().get_response(path, scope)
        except StarletteHTTPException as exc:
            # Starlette builds `path` with the OS separator: `w\acme.shop` on Windows.
            first, _, rest = path.replace("\\", "/").partition("/")
            is_file = first == "assets" or (not rest and "." in first)
            if exc.status_code != 404 or first in ("api", "mcp") or is_file:
                raise
            return await super().get_response("index.html", scope)


log = logging.getLogger("ekos.console")


def _is_loopback(host: str) -> bool:
    try:
        return ipaddress.ip_address(host.strip("[]")).is_loopback
    except ValueError:
        return host.lower() == "localhost"


def _host_name(header: str) -> str:
    """The name in a `Host` header, without its port: `[::1]:8000` → `::1`."""
    if header.startswith("["):
        return header[1:].split("]", 1)[0]
    return header.rsplit(":", 1)[0] if header.count(":") == 1 else header


class LoopbackOnly:
    """RFC 0184 §3, local mode: only this machine may talk to the console. The client must be a
    loopback address, and the `Host` header must name one, so a page on another site that has
    pointed its own name at 127.0.0.1 (DNS rebinding) is refused."""

    def __init__(self, app) -> None:
        self.app = app

    async def __call__(self, scope, receive, send) -> None:
        if scope["type"] in ("http", "websocket"):
            client = (scope.get("client") or ("", 0))[0]
            headers = dict(scope.get("headers") or [])
            host = _host_name(headers.get(b"host", b"").decode("latin-1"))
            if not (_is_loopback(client) and _is_loopback(host)):
                if scope["type"] == "http":
                    response = JSONResponse(
                        {"detail": "this console is in local mode: this machine only"},
                        status_code=403,
                    )
                    await response(scope, receive, send)
                return
        await self.app(scope, receive, send)


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


@asynccontextmanager
async def _lifespan(app: FastAPI):
    settings = get_settings()
    # RFC 0232: before anything writes the home, become its only writer.
    lease = (
        cloud.InstanceLease(settings.control_dir, interval=settings.instance_lease_seconds)
        if settings.instance_lease
        else None
    )
    if lease is not None:
        await lease.take_over()
    if settings.backup_url and settings.backup_restore_on_empty and settings.ekos_home:
        restored = await cloud.restore_if_empty(
            settings.ekos_bin,
            settings.backup_url,
            Path(settings.ekos_home),
            Path(settings.console_db),
        )
        # The restore replaced control/, lease file included: write it back at once, so a
        # console starting now still waits for this one.
        if restored == "restored" and lease is not None:
            lease.beat()
    models.init_engine(settings.console_db)
    _seed_registry_if_empty()
    home_projects.register_new()

    app.state.supervisor = McpSupervisor(settings)
    # RFC 0182: no server is started here. Each starts when a page or a gateway call first needs
    # it, and stops again when idle.
    await app.state.supervisor.start([])

    app.state.runner = JobRunner(settings)
    app.state.runner.start()

    app.state.scheduler = ConsoleScheduler(app.state.runner)
    app.state.scheduler.start()

    # RFC 0185: under `ekos console`, stop and restart arrive as files in the control directory.
    watcher = (
        asyncio.create_task(lifecycle.watch(settings.control_dir)) if settings.supervised else None
    )
    holder = asyncio.create_task(lease.hold(lifecycle.shut_down)) if lease is not None else None
    if settings.backup_url:
        job = cloud.BackupJob(settings.ekos_bin, settings.backup_url)
        app.state.backup = job
        app.state.scheduler.add_platform_job(
            "ekos-home-backup", job.run, cloud.backup_trigger(settings.backup_cron)
        )
    try:
        yield
    finally:
        if holder is not None:
            holder.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await holder
        if watcher is not None:
            watcher.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await watcher
        await app.state.scheduler.aclose()
        await app.state.runner.aclose()
        await app.state.supervisor.aclose()
        if lease is not None:
            lease.release()


def cookie_is_https_only(settings) -> bool:
    """Whether the session cookie is marked `Secure`. It is when sign-in comes back to an
    `https://` address, which is what a console behind a TLS proxy is configured with. On plain
    HTTP (development, a first try on loopback) a `Secure` cookie would never be sent back and
    nobody could sign in."""
    return settings.oidc_enabled and settings.oidc_redirect_uri.lower().startswith("https://")


def create_app() -> FastAPI:
    settings = get_settings()
    app = FastAPI(title="EKOS Console API", version="0.1.0", lifespan=_lifespan)
    app.state.settings = settings

    app.add_middleware(
        SessionMiddleware,
        secret_key=settings.session_secret,
        same_site="lax",
        https_only=cookie_is_https_only(settings),
    )
    app.add_middleware(
        CORSMiddleware,
        allow_origins=[settings.dev_origin],
        allow_credentials=True,
        allow_methods=["*"],
        allow_headers=["*"],
    )

    # Outermost, so nothing answers a request from elsewhere, not even a 404.
    if settings.local:
        app.add_middleware(LoopbackOnly)

    for prefix, routers in MOUNTS:
        for r in routers:
            app.include_router(r.router, prefix=prefix)

    # Serve the built UI when it exists (Compose / production / a package); the Vite dev server
    # handles it otherwise.
    ui = Path(settings.ui_dir) if settings.ui_dir else _UI_DIST
    if ui.is_dir():
        app.mount("/", _SpaFiles(directory=ui, html=True), name="ui")

    return app
