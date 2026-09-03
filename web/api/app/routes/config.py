"""`ekos.toml` config UX (RFC 0130 §3).

`GET` / `PUT` read and write the file (the write is a plain local `.bak`-then-write, not a
pipeline operation). `validate` and `preview-scan` shell out to `ekos config …` through the
read-only subprocess allowlist so there is one source of truth for the checks.
"""

from __future__ import annotations

import tempfile
from pathlib import Path
from typing import Any

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel

from .. import config_io, models, readproc
from ..deps import require_console_token, require_workspace
from ..settings import Settings, get_settings

router = APIRouter(
    prefix="/workspaces", tags=["config"], dependencies=[Depends(require_console_token)]
)


class ConfigOut(BaseModel):
    raw: str
    observe: dict[str, list[str]]


class RawIn(BaseModel):
    raw: str


class WriteOut(BaseModel):
    written: bool
    observe_delta: dict[str, list[str]]
    warnings: list[dict[str, str]]
    append_only_warning: str | None = None


def _bin(s: Settings) -> str:
    return s.ekos_bin


async def _validate_text(settings: Settings, ws_path: str, raw: str) -> dict[str, Any]:
    """Run `ekos config validate --json` against `raw` written to a temp file, with the walk
    still rooted at the real workspace (so `observe-path-missing` resolves correctly)."""
    with tempfile.NamedTemporaryFile("w", suffix=".toml", delete=False) as tf:
        tf.write(raw)
        tmp = tf.name
    try:
        return await readproc.read_json(
            _bin(settings), ws_path, ["config", "validate", "--json", "--file", tmp]
        )
    finally:
        Path(tmp).unlink(missing_ok=True)


@router.get("/{workspace_id}/config", response_model=ConfigOut)
async def get_config(ws: models.Workspace = Depends(require_workspace)) -> ConfigOut:
    try:
        raw, observe = config_io.read_config(ws.path)
    except config_io.ConfigError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc
    return ConfigOut(
        raw=raw,
        observe={"paths": observe.paths, "ignore_patterns": observe.ignore_patterns},
    )


@router.post("/{workspace_id}/config/validate")
async def validate_config(
    body: RawIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
) -> dict[str, Any]:
    try:
        config_io.parse(body.raw)
    except config_io.ConfigError as exc:
        return {
            "schema_version": 1,
            "ok": False,
            "errors": [{"code": "toml", "detail": str(exc)}],
            "warnings": [],
        }
    try:
        return await _validate_text(settings, ws.path, body.raw)
    except readproc.ReadProcError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc


@router.post("/{workspace_id}/config/preview-scan")
async def preview_scan(
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
) -> dict[str, Any]:
    try:
        return await readproc.read_json(
            _bin(settings), ws.path, ["config", "preview-scan", "--json"]
        )
    except readproc.ReadProcError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc


@router.put("/{workspace_id}/config", response_model=WriteOut)
async def put_config(
    body: RawIn,
    ws: models.Workspace = Depends(require_workspace),
    settings: Settings = Depends(get_settings),
) -> WriteOut:
    # 1. syntax
    try:
        config_io.parse(body.raw)
    except config_io.ConfigError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc

    # 2. semantics (via the CLI)
    try:
        report = await _validate_text(settings, ws.path, body.raw)
    except readproc.ReadProcError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc
    if not report.get("ok", False):
        raise HTTPException(status_code=422, detail={"errors": report.get("errors", [])})

    # 3. write (.bak, then the file)
    try:
        delta = config_io.write_config(ws.path, body.raw)
    except config_io.ConfigError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc

    return WriteOut(
        written=True,
        observe_delta={
            "added_paths": delta.added_paths,
            "removed_paths": delta.removed_paths,
            "added_patterns": delta.added_patterns,
            "removed_patterns": delta.removed_patterns,
        },
        warnings=report.get("warnings", []),
        append_only_warning=config_io.append_only_warning(delta),
    )
