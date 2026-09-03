"""Public, unauthenticated meta endpoints."""

from __future__ import annotations

from importlib.metadata import PackageNotFoundError, version

from fastapi import APIRouter

from ..schemas import Health

router = APIRouter(tags=["meta"])


def _version() -> str:
    try:
        return version("ekos-console-api")
    except PackageNotFoundError:
        return "0.0.0+dev"


@router.get("/health", response_model=Health)
async def health() -> Health:
    return Health(version=_version())
