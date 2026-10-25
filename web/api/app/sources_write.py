"""Sources and databases from the console (RFC 0223) — argv builders and the credential seam.

A database password is taken once, stored in the keychain under a variable name the console
derives (`EKOS_DB_<TENANT>_<PROJECT>_<DATABASE>`), and granted to the tenant as a source
credential (`Tenant.source_env`, always in its environment). `ekos.toml` receives the variable's
name only. Nothing here logs or returns a password.
"""

from __future__ import annotations

import re

from . import envs, keychain, models
from .semantics_write import ReviewError

_NAME = re.compile(r"^[a-z0-9][a-z0-9_-]{0,62}$")
_OWNER = re.compile(r"^[A-Za-z][A-Za-z0-9_$#]{0,127}$")
_CONNECT = re.compile(r"^[A-Za-z0-9._-]+(:[0-9]{1,5})?/[A-Za-z0-9._$#-]+$")
_USER = re.compile(r"^[A-Za-z][A-Za-z0-9_$#]{0,127}$")
PROFILES = ("p0", "p1", "p2")


def check_name(name: str, what: str) -> str:
    if not _NAME.match(name or ""):
        raise ReviewError(
            f"{what} name: lowercase letters, digits, `-` and `_`, starting with a letter or digit"
        )
    return name


def secret_name(tenant: str, project: str, database: str) -> str:
    """The variable a database's password is kept under."""
    raw = f"EKOS_DB_{tenant}_{project}_{database}"
    return re.sub(r"[^A-Za-z0-9_]", "_", raw).upper()


def source_add_argv(name: str, path: str, kind: str | None) -> list[str]:
    check_name(name, "source")
    if not path or "\x00" in path:
        raise ReviewError("a source needs a directory")
    argv = ["source", "add", name, f"--path={path}"]
    if kind is not None:
        if kind not in ("git", "dir"):
            raise ReviewError("a source's kind is git or dir")
        argv.append(f"--kind={kind}")
    return argv


def database_add_argv(
    name: str,
    connect: str,
    user: str,
    password_env: str,
    owners: list[str],
    profile: str,
    dictionary: str | None,
) -> list[str]:
    check_name(name, "database")
    if not _CONNECT.match(connect or ""):
        raise ReviewError("connect is host:port/service (Easy Connect)")
    if not _USER.match(user or ""):
        raise ReviewError("user: an Oracle user name")
    if not owners or any(not _OWNER.match(o) for o in owners) or len(owners) > 50:
        raise ReviewError("owners: one or more schema names")
    if profile not in PROFILES:
        raise ReviewError("profile is p0, p1 or p2")
    if dictionary not in (None, "idempiere"):
        raise ReviewError("dictionary is idempiere or none")
    if not envs.is_valid_name(password_env):
        raise ReviewError("not a usable variable name")
    argv = [
        "database",
        "add",
        name,
        f"--connect={connect}",
        f"--user={user}",
        f"--password-env={password_env}",
        f"--profile={profile}",
    ]
    argv += [f"--owner={o}" for o in owners]
    if dictionary:
        argv.append(f"--dictionary={dictionary}")
    return argv


def store_password(tenant: models.Tenant, variable: str, password: str) -> None:
    """Keep the password in the keychain and grant its variable to the tenant."""
    if not password or "\x00" in password or len(password) > 1024:
        raise ReviewError("a password of 1 to 1024 characters is needed")
    keychain.put_env(variable, password)
    names = list(tenant.source_env or [])
    if variable not in names:
        models.update_tenant(tenant.id, source_env=[*names, variable])


def forget_password(tenant: models.Tenant, variable: str) -> None:
    """Delete a console-made credential and its grant. A variable the console did not make (an
    operator's own `password-env`) is left alone."""
    if not variable.startswith("EKOS_DB_"):
        return
    keychain.delete_env(variable)
    names = [n for n in tenant.source_env or [] if n != variable]
    models.update_tenant(tenant.id, source_env=names)
