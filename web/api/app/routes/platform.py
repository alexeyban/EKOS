"""Console settings and keychain, for platform owners (RFC 0184).

`console.toml` holds what the environment does not set; secrets go to the OS keychain, never to
the file or a response. Every setting is read at start, so a saved change applies after a
restart, which this router can trigger when a launcher restarts the console.
"""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Request
from pydantic import BaseModel, ValidationError

from .. import authz, console_file, envs, keychain, lifecycle
from ..auth import Principal
from ..settings import Settings
from ._responses import BAD_REQUEST

log = logging.getLogger("ekos.console")

router = APIRouter(prefix="/platform", tags=["platform"])

CONFLICT = {"description": "Not possible here (no keychain, no home, no supervisor)"}


class SettingsChange(BaseModel):
    values: dict[str, Any] = {}
    secrets: dict[str, str | None] = {}


class SecretValue(BaseModel):
    value: str


def _jsonable(value: Any) -> Any:
    return str(value) if isinstance(value, Path) else value


def _fresh() -> tuple[Settings | None, str]:
    """Settings as the next start would build them, or why they cannot be built."""
    try:
        return Settings(), ""
    except (ValidationError, ValueError) as exc:
        return None, str(exc)


@router.get("/settings")
async def get_settings_view(request: Request, _: Principal = Depends(authz.allow_platform)) -> dict:
    file_values = console_file.read()
    fresh, error = _fresh()
    shown = fresh or request.app.state.settings
    fields = []
    for name in Settings.model_fields:
        meta = console_file.FIELDS[name]
        source = console_file.source_of(Settings, name, file_values)
        fields.append(
            {
                "name": name,
                "group": meta.group,
                "help": meta.help,
                "secret": meta.secret,
                "editable": meta.editable and source != "environment",
                "source": source,
                "set": source != "default",
                "value": None if meta.secret else _jsonable(getattr(shown, name)),
                "type": type(getattr(shown, name)).__name__,
            }
        )
    usable, backend = keychain.available()
    path = console_file.path()
    return {
        "fields": fields,
        "file": str(path) if path else None,
        "error": error,
        "keychain": {"available": usable, "backend": backend},
        "supervised": request.app.state.settings.supervised,
    }


def _check_change(body: SettingsChange) -> None:
    fields = Settings.model_fields
    for name in list(body.values) + list(body.secrets):
        if name not in fields:
            raise HTTPException(status_code=400, detail=f"no setting {name!r}")
        meta = console_file.FIELDS[name]
        if not meta.editable:
            raise HTTPException(status_code=400, detail=f"{name} is not changed from the console")
        if console_file.set_by_environment(Settings, name):
            raise HTTPException(
                status_code=400, detail=f"{name} is set by the environment; change it there"
            )
    for name in body.values:
        if console_file.FIELDS[name].secret:
            raise HTTPException(
                status_code=400, detail=f"{name} is a secret: send it under `secrets`"
            )
    for name in body.secrets:
        if not console_file.FIELDS[name].secret:
            raise HTTPException(status_code=400, detail=f"{name} is not a secret")


@router.put("/settings", responses={400: BAD_REQUEST, 409: CONFLICT})
async def put_settings(body: SettingsChange, _: Principal = Depends(authz.allow_platform)) -> dict:
    _check_change(body)
    path = console_file.path()
    if body.values and path is None:
        raise HTTPException(
            status_code=409,
            detail="this console has no EKOS home, so no settings file; set environment variables",
        )
    if body.secrets and not keychain.available()[0]:
        raise HTTPException(
            status_code=409,
            detail="no keychain here; set the secret as an environment variable instead",
        )

    content = console_file.read()
    for name, value in body.values.items():
        if value is None:
            content.pop(name, None)
        else:
            content[name] = value
    with console_file.reading(content):
        try:
            built = Settings()
        except (ValidationError, ValueError) as exc:
            raise HTTPException(status_code=400, detail=str(exc)) from exc

    for name, value in body.secrets.items():
        if value is None:
            keychain.delete_console(name)
        else:
            keychain.put_console(name, value)
    if body.values and path is not None:
        # The validated value, so `"300"` sent for a number is written as 300.
        typed = {
            n: (None if v is None else _jsonable(getattr(built, n))) for n, v in body.values.items()
        }
        console_file.write(path, typed)
    return {"ok": True, "restart_required": True, "file": str(path) if path else None}


@router.post("/restart", status_code=202, responses={409: CONFLICT})
async def restart(request: Request, _: Principal = Depends(authz.allow_platform)) -> dict:
    """Ask the launcher for a restart (RFC 0185 §2): the console's watcher sees the file, shuts
    down cleanly, and the launcher starts it again."""
    settings: Settings = request.app.state.settings
    if not settings.supervised:
        raise HTTPException(
            status_code=409,
            detail="nothing would start the console again; restart it yourself "
            "(it was not started by `ekos console` or a container with a restart policy)",
        )
    marker = settings.control_dir / lifecycle.RESTART
    marker.parent.mkdir(parents=True, exist_ok=True)
    marker.touch()
    return {"ok": True}


# ── values for environment variables ─────────────────────────────────────────


@router.get("/keychain")
async def get_keychain(_: Principal = Depends(authz.allow_platform)) -> dict:
    usable, backend = keychain.available()
    return {"available": usable, "backend": backend, "names": keychain.env_names()}


def _variable(name: str) -> str:
    if not envs.is_valid_name(name):
        raise HTTPException(
            status_code=400,
            detail=f"{name!r} is not a variable a project may be given "
            "(letters, digits and _, not starting with a digit or EKOS_CONSOLE_)",
        )
    return name


@router.put("/keychain/{name}", responses={400: BAD_REQUEST, 409: CONFLICT})
async def put_keychain(
    name: str, body: SecretValue, _: Principal = Depends(authz.allow_platform)
) -> dict:
    _variable(name)
    if not body.value:
        raise HTTPException(status_code=400, detail="an empty value; remove the entry instead")
    try:
        keychain.put_env(name, body.value)
    except keychain.Unavailable as exc:
        raise HTTPException(status_code=409, detail=str(exc)) from exc
    return {"ok": True}


@router.delete("/keychain/{name}", responses={400: BAD_REQUEST, 409: CONFLICT})
async def delete_keychain(name: str, _: Principal = Depends(authz.allow_platform)) -> dict:
    _variable(name)
    try:
        found = keychain.delete_env(name)
    except keychain.Unavailable as exc:
        raise HTTPException(status_code=409, detail=str(exc)) from exc
    return {"ok": True, "found": found}
