"""Console settings and keychain, for platform owners (RFC 0184).

`console.toml` holds what the environment does not set; secrets go to the OS keychain, never to
the file or a response. Every setting is read at start, so a saved change applies after a
restart, which this router can trigger when a launcher restarts the console.
"""

from __future__ import annotations

import asyncio
import contextlib
import logging
from pathlib import Path
from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Request
from pydantic import BaseModel, ValidationError

from .. import _proc, authz, console_file, envs, keychain, lifecycle
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


# ── uninstall (RFC 0188) ─────────────────────────────────────────────────────

BAD_GATEWAY = {"description": "The uninstall page did not start"}
# What `ekos uninstall --ui --detach` prints before the page's link.
_UNINSTALL_PREFIX = "EKOS uninstall: "


def _uninstall_refusal(settings: Settings) -> str | None:
    """Why the console cannot offer the uninstall page here, or None."""
    if settings.mode != "local":
        return (
            "only in local mode: the uninstall page opens on the machine the console runs on. "
            "On a server, run `ekos uninstall` there"
        )
    if not settings.ekos_home:
        return "this console has no EKOS home"
    if not (Path(settings.ekos_home) / "control" / "install.json").is_file():
        return (
            "EKOS was not installed by `ekos setup` here (no control/install.json); "
            "remove it the way it was installed"
        )
    return None


@router.get("/uninstall")
async def get_uninstall(request: Request, _: Principal = Depends(authz.allow_platform)) -> dict:
    reason = _uninstall_refusal(request.app.state.settings)
    return {"available": reason is None, "reason": reason}


@router.post("/uninstall", responses={409: CONFLICT, 502: BAD_GATEWAY})
async def start_uninstall(request: Request, _: Principal = Depends(authz.allow_platform)) -> dict:
    """Start the uninstall page in its own process and return its link. Nothing is removed
    until the person confirms on that page."""
    settings: Settings = request.app.state.settings
    reason = _uninstall_refusal(settings)
    if reason:
        raise HTTPException(status_code=409, detail=reason)
    argv = [settings.ekos_bin, "--home", settings.ekos_home, "uninstall", "--ui", "--detach"]
    proc = await _proc.spawn(argv, env=envs.for_tenant(None))
    # Read lines until the link, never to end of file: the page runs on in its own process and
    # may hold the pipe (uvloop does not mark it close-on-exec), so EOF would come only when the
    # page stops.
    url, seen = "", []
    try:
        async with asyncio.timeout(40):
            while line := await proc.stdout.readline():
                text = _proc.strip_ansi(line.decode(errors="replace")).strip()
                if text.startswith(_UNINSTALL_PREFIX):
                    url = text[len(_UNINSTALL_PREFIX) :].strip()
                    break
                seen.append(text)
            else:
                # End of output without a link: the command failed; let it finish. (Not after
                # a link: asyncio's wait() also waits for the pipes the page may hold.)
                await proc.wait()
    except TimeoutError:
        await _proc.terminate(proc)
    # The link must be the page on this machine; anything else is not followed.
    if not url.startswith("http://127.0.0.1:"):
        err = b""
        if proc.returncode is not None:
            with contextlib.suppress(TimeoutError):
                err = await asyncio.wait_for(proc.stderr.read(4096), 2)
        detail = (_proc.strip_ansi(err.decode(errors="replace")).strip() or " ".join(seen))[-500:]
        log.warning("uninstall page did not start: %s", detail)
        raise HTTPException(
            status_code=502, detail=f"the uninstall page did not start: {detail or 'no link'}"
        )
    return {"url": url}


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
