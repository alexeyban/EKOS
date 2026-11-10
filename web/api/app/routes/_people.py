"""Shared by the inbox, testimony and questions routes (RFC 0253/0254): running one CLI read or
write and turning its failure into an HTTP answer."""

from __future__ import annotations

from typing import Any

from fastapi import HTTPException

from .. import models, people_write, readproc, semantics_write
from ..auth import Principal
from ..settings import Settings

_BIG = 16 << 20


async def read(settings: Settings, ws: models.Workspace, argv: list[str]) -> Any:
    try:
        return await readproc.read_json(
            settings.ekos_bin, ws.path, argv, max_output=_BIG, time_limit=300.0
        )
    except readproc.ReadProcError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc


async def write(settings: Settings, ws: models.Workspace, argv: list[str]) -> dict[str, Any]:
    try:
        return await people_write.run_json(settings.ekos_bin, ws.path, argv)
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    except semantics_write.LedgerBusy as exc:
        raise HTTPException(
            status_code=409, detail=f"the workspace is busy (a run holds the ledger): {exc}"
        ) from exc
    except semantics_write.CliFailed as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc


def person(principal: Principal, settings: Settings, typed: str | None) -> str:
    """Who a write is attributed to: the signed-in person; in development token mode a typed
    name recorded as `token:<name>` (the same rule as every review)."""
    if principal.kind == "token":
        raise HTTPException(status_code=403, detail="this is for a person, not an API token")
    try:
        return semantics_write.reviewer_for(principal, settings.signs_in, typed)
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc


def bad_request(exc: Exception) -> HTTPException:
    return HTTPException(status_code=400, detail=str(exc))
