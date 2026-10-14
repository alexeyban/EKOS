"""Secrets in the operating system's keychain (RFC 0184 §2).

Windows Credential Manager, macOS Keychain or Secret Service on Linux, through `keyring`. One
service, `ekos`, two kinds of entry:

* `env:<NAME>` — the value of an environment variable a project names for its model key;
* `console:<field>` — a secret console setting (OIDC client secret, session secret).

`keyring` cannot list what it holds, so the names are kept in one more entry, `index`, as JSON.
A value is never returned to a browser, logged or audited; only names are.

A container or a headless server usually has no keychain. `available()` says so and every write
raises `Unavailable`; reads then find nothing. `EKOS_CONSOLE_KEYCHAIN=off` turns it off outright.
"""

from __future__ import annotations

import json
import logging
import os

import keyring
from keyring.errors import KeyringError, PasswordDeleteError

log = logging.getLogger("ekos.console")

SERVICE = "ekos"
_INDEX = "index"
ENV = "env:"
CONSOLE = "console:"


class Unavailable(RuntimeError):
    """No usable keychain here."""


def _disabled() -> bool:
    return os.environ.get("EKOS_CONSOLE_KEYCHAIN", "").strip().lower() in ("off", "0", "false")


def _backend_usable(backend) -> bool:
    from keyring.backends import chainer, fail

    if isinstance(backend, fail.Keyring):
        return False
    if isinstance(backend, chainer.ChainerBackend):
        return bool(backend.backends)
    try:
        return backend.priority > 0
    except Exception:  # a backend that cannot rate itself is not usable
        return False


def available() -> tuple[bool, str]:
    """Whether a keychain can be used, and a name for it."""
    if _disabled():
        return False, "off (EKOS_CONSOLE_KEYCHAIN)"
    backend = keyring.get_keyring()
    name = type(backend).__module__.rsplit(".", 1)[-1] + "." + type(backend).__name__
    return _backend_usable(backend), name


def _get(entry: str) -> str | None:
    if not available()[0]:
        return None
    try:
        return keyring.get_password(SERVICE, entry)
    except KeyringError as exc:
        log.warning("keychain read of %s failed: %s", entry, type(exc).__name__)
        return None


def _require() -> None:
    usable, name = available()
    if not usable:
        raise Unavailable(
            f"no usable keychain here ({name}); set the value as an environment variable instead"
        )


def _index() -> dict[str, list[str]]:
    raw = _get(_INDEX)
    try:
        data = json.loads(raw) if raw else {}
    except ValueError:
        data = {}
    return {kind: sorted(set(data.get(kind, []))) for kind in ("env", "console")}


def _write_index(index: dict[str, list[str]]) -> None:
    keyring.set_password(SERVICE, _INDEX, json.dumps(index, sort_keys=True))


def _put(kind: str, name: str, value: str) -> None:
    _require()
    if not value:
        raise ValueError("an empty value; remove the entry instead")
    keyring.set_password(SERVICE, f"{kind}:{name}", value)
    index = _index()
    index[kind] = sorted(set(index[kind]) | {name})
    _write_index(index)


def _delete(kind: str, name: str) -> bool:
    _require()
    try:
        keyring.delete_password(SERVICE, f"{kind}:{name}")
        found = True
    except PasswordDeleteError:
        found = False
    index = _index()
    index[kind] = [n for n in index[kind] if n != name]
    _write_index(index)
    return found


# ── values for environment variables ─────────────────────────────────────────


def put_env(name: str, value: str) -> None:
    _put("env", name, value)


def get_env(name: str) -> str | None:
    return _get(ENV + name)


def delete_env(name: str) -> bool:
    return _delete("env", name)


def env_names() -> list[str]:
    return _index()["env"] if available()[0] else []


# ── console secrets ──────────────────────────────────────────────────────────


def put_console(field: str, value: str) -> None:
    _put("console", field, value)


def get_console(field: str) -> str | None:
    return _get(CONSOLE + field)


def delete_console(field: str) -> bool:
    return _delete("console", field)
