"""Business semantics review + LinkML schema viewer/editor (RFC 0170).

Reads (`items`, `item`, `gaps`, `linkml`, and an import *dry run*) go through the read-only
subprocess seam and need the `read` role. Decisions — confirm/reject/edit and an applied LinkML
import — are human review: they need the `write` role, run the human-only CLI commands
(`semantics_write`), and are attributed to the signed-in user. Nothing here goes through MCP:
the MCP surface cannot promote a hypothesis, by design and by test.
"""

from __future__ import annotations

from typing import Any, Literal

from fastapi import APIRouter, Depends, HTTPException, Query
from pydantic import BaseModel, Field

from .. import models, readproc, semantics_write
from ..auth import Principal, require_role
from ..deps import require_workspace
from ..settings import Settings, get_settings
from ._responses import BAD_GATEWAY, BAD_REQUEST, CONFLICT, NOT_FOUND

router = APIRouter(prefix="/workspaces/{workspace_id}/semantics", tags=["semantics"])

# Module-level so the dependency is built once (and ruff's B008 is satisfied).
_WRITER = require_role("write")

_BIG = 32 << 20  # a large workspace's item list or schema


async def _read(settings: Settings, ws: models.Workspace, argv: list[str]) -> Any:
    try:
        return await readproc.read_json(settings.ekos_bin, ws.path, argv, max_output=_BIG)
    except readproc.ReadProcError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc


Kind = Literal["concept", "enum", "constraint", "gap", "conflict", "rationale"]
Status = Literal["hypothesis", "confirmed", "rejected", "needs_review"]


@router.get("/items", dependencies=[Depends(require_role("read"))], responses={502: BAD_GATEWAY})
async def items(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    kind: Kind | None = Query(default=None),
    status: Status | None = Query(default=None),
) -> list[dict]:
    """The current hypotheses and decisions — `ekos semantics list --json`."""
    argv = ["semantics", "list", "--json"]
    if kind:
        argv += ["--kind", kind]
    if status:
        argv += ["--status", status]
    return await _read(settings, ws, argv)


@router.get(
    "/items/{item_id}",
    dependencies=[Depends(require_role("read"))],
    responses={400: BAD_REQUEST, 502: BAD_GATEWAY},
)
async def item(
    item_id: str,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
) -> dict:
    """One item with its evidence lines and links — `ekos semantics show --json <id>`."""
    if not semantics_write._UUID.match(item_id):
        raise HTTPException(status_code=400, detail="item id must be a UUID")
    return await _read(settings, ws, ["semantics", "show", "--json", item_id])


@router.get("/gaps", dependencies=[Depends(require_role("read"))], responses={502: BAD_GATEWAY})
async def gaps(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
) -> list[dict]:
    """Open questions, conflicts and the `needs_review` queue."""
    return await _read(settings, ws, ["semantics", "gaps", "--json"])


@router.get("/linkml", dependencies=[Depends(require_role("read"))], responses={502: BAD_GATEWAY})
async def linkml(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    status: Literal["confirmed", "hypothesis", "all"] = Query(default="all"),
) -> dict:
    """The LinkML schema as JSON (`ekos export linkml --json`). An empty selection is not an
    error here: the viewer shows an empty schema and says why."""
    try:
        schema = await readproc.read_json(
            settings.ekos_bin,
            ws.path,
            ["export", "linkml", "--json", "--status", status],
            max_output=_BIG,
        )
    except readproc.ReadProcError as exc:
        if "nothing to export" in str(exc):
            return {"empty": True, "reason": str(exc).split(": ", 1)[-1][:400]}
        raise HTTPException(status_code=502, detail=str(exc)) from exc
    return {"empty": False, "schema": schema}


@router.get(
    "/linkml/yaml", dependencies=[Depends(require_role("read"))], responses={502: BAD_GATEWAY}
)
async def linkml_yaml(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    status: Literal["confirmed", "hypothesis", "all"] = Query(default="all"),
) -> dict:
    """The schema as YAML text — what the editor edits and `ekos import linkml` reads back."""
    try:
        text = await readproc.read_text(
            settings.ekos_bin,
            ws.path,
            ["export", "linkml", "--yaml", "--status", status],
            max_output=_BIG,
        )
    except readproc.ReadProcError as exc:
        if "nothing to export" in str(exc):
            return {"empty": True, "reason": str(exc).split(": ", 1)[-1][:400], "yaml": ""}
        raise HTTPException(status_code=502, detail=str(exc)) from exc
    return {"empty": False, "yaml": text}


# ── human decisions ──────────────────────────────────────────────────────────────────────────


class ReviewIn(BaseModel):
    action: Literal["confirm", "reject", "edit"]
    note: str | None = Field(default=None, max_length=2000)
    name: str | None = Field(default=None, max_length=2000)
    description: str | None = Field(default=None, max_length=2000)
    label: str | None = Field(default=None, max_length=2000)
    # Token mode only: who is reviewing (recorded as `token:<name>`). Ignored under OIDC.
    reviewer: str | None = Field(default=None, max_length=80)


def _reviewer(principal: Principal, settings: Settings, typed: str | None) -> str:
    try:
        return semantics_write.reviewer_for(principal, settings.oidc_enabled, typed)
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc


@router.post(
    "/items/{item_id}/review",
    responses={400: BAD_REQUEST, 404: NOT_FOUND, 409: CONFLICT, 502: BAD_GATEWAY},
)
async def review(
    item_id: str,
    body: ReviewIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_WRITER),
) -> dict:
    """Confirm, reject or edit one hypothesis — `ekos semantics <action> <id> --as <you>`."""
    who = _reviewer(principal, settings, body.reviewer)
    try:
        argv = semantics_write.review_argv(
            body.action,
            item_id,
            who,
            note=body.note,
            name=body.name,
            description=body.description,
            label=body.label,
        )
        message = await semantics_write.review(settings.ekos_bin, ws.path, argv)
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    except semantics_write.LedgerBusy as exc:
        raise HTTPException(
            status_code=409, detail=f"the workspace is busy (a run holds the ledger): {exc}"
        ) from exc
    except semantics_write.CliFailed as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    return {"ok": True, "reviewer": who, "message": message}


class BulkIn(BaseModel):
    action: Literal["confirm", "reject"]
    ids: list[str] = Field(min_length=1, max_length=500)
    note: str | None = Field(default=None, max_length=2000)
    reviewer: str | None = Field(default=None, max_length=80)


@router.post(
    "/review-bulk",
    responses={400: BAD_REQUEST, 409: CONFLICT, 502: BAD_GATEWAY},
)
async def review_bulk(
    body: BulkIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_WRITER),
) -> dict:
    """Confirm or reject many items in one decision — all or nothing, one reviewer, one note."""
    who = _reviewer(principal, settings, body.reviewer)
    try:
        argv = semantics_write.bulk_argv(body.action, body.ids, who, note=body.note)
        message = await semantics_write.review(settings.ekos_bin, ws.path, argv)
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    except semantics_write.LedgerBusy as exc:
        raise HTTPException(status_code=409, detail=str(exc)) from exc
    except semantics_write.CliFailed as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    return {"ok": True, "reviewer": who, "count": len(body.ids), "message": message}


class ImportIn(BaseModel):
    yaml: str
    reviewer: str | None = Field(default=None, max_length=80)


async def _import(
    body: ImportIn, ws: models.Workspace, settings: Settings, *, dry_run: bool, who: str | None
) -> dict:
    try:
        return await semantics_write.import_linkml(
            settings.ekos_bin, ws.path, body.yaml, dry_run=dry_run, reviewer=who
        )
    except semantics_write.ReviewError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    except semantics_write.LedgerBusy as exc:
        raise HTTPException(status_code=409, detail=str(exc)) from exc
    except semantics_write.CliFailed as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc


@router.post(
    "/linkml/validate",
    dependencies=[Depends(require_role("read"))],
    responses={400: BAD_REQUEST, 502: BAD_GATEWAY},
)
async def validate_linkml(
    body: ImportIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
) -> dict:
    """Dry run: what an edited schema would decide, with errors and warnings. Writes nothing."""
    return await _import(body, ws, settings, dry_run=True, who=None)


@router.post(
    "/linkml/import",
    responses={400: BAD_REQUEST, 409: CONFLICT, 502: BAD_GATEWAY},
)
async def apply_linkml(
    body: ImportIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(_WRITER),
) -> dict:
    """Apply an edited schema's decisions — all or nothing — attributed to the signed-in user."""
    who = _reviewer(principal, settings, body.reviewer)
    return await _import(body, ws, settings, dry_run=False, who=who)
