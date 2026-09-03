"""FastAPI app factory for the EKOS web console (RFC 0128 §3).

uvicorn app.main:create_app --factory
"""

from __future__ import annotations

from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles

from .mcp_client import ClientPool
from .routes import graph, meta, workspaces
from .settings import get_settings

_UI_DIST = Path(__file__).resolve().parents[2] / "ui" / "dist"


@asynccontextmanager
async def _lifespan(app: FastAPI):
    app.state.pool = ClientPool()
    try:
        yield
    finally:
        await app.state.pool.aclose()


def create_app() -> FastAPI:
    settings = get_settings()
    app = FastAPI(title="EKOS Console API", version="0.1.0", lifespan=_lifespan)

    app.add_middleware(
        CORSMiddleware,
        allow_origins=[settings.dev_origin],
        allow_methods=["*"],
        allow_headers=["*"],
    )

    app.include_router(meta.router, prefix="/api")
    app.include_router(workspaces.router, prefix="/api")
    app.include_router(graph.router, prefix="/api")

    # Serve the built UI when it exists (Compose / production); the Vite dev server handles it
    # otherwise.
    if _UI_DIST.is_dir():
        app.mount("/", StaticFiles(directory=_UI_DIST, html=True), name="ui")

    return app
