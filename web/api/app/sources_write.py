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
_OWNER = re.compile(r"^[A-Za-z_][A-Za-z0-9_$#]{0,127}$")
_USER = re.compile(r"^[A-Za-z_][A-Za-z0-9_$#.-]{0,127}$")
PROFILES = ("p0", "p1", "p2")
# RFC 0235: each kind's `connect` form, and what the form calls it.
KINDS = ("oracle", "postgres", "clickhouse", "sqlserver", "informix")
_CONNECT = {
    "oracle": (
        re.compile(r"^[A-Za-z0-9._-]+(:[0-9]{1,5})?/[A-Za-z0-9._$#-]+$"),
        "host:port/service (Easy Connect)",
    ),
    "postgres": (
        re.compile(r"^[A-Za-z0-9._-]+(:[0-9]{1,5})?/[A-Za-z0-9._$-]+$"),
        "host[:port]/database",
    ),
    "clickhouse": (
        re.compile(r"^https?://[A-Za-z0-9._-]+(:[0-9]{1,5})?/?$"),
        "http[s]://host[:port]",
    ),
    # RFC 0252 §3.5: as PostgreSQL's, plus the one option `ekos` accepts for a self-signed
    # certificate.
    "sqlserver": (
        re.compile(
            r"^[A-Za-z0-9._-]+(:[0-9]{1,5})?/[A-Za-z0-9._$-]+(\?trust-server-certificate=true)?$"
        ),
        "host[:port]/database[?trust-server-certificate=true]",
    ),
    # RFC 0252 §3.5: the Informix Client SDK (`…@server`) or the Data Server Driver over DRDA.
    # `?driver=` (a shared library to load) is for `ekos.toml` only, never the web form.
    "informix": (
        re.compile(
            r"^(?:[A-Za-z0-9._-]+(:[0-9]{1,5})?/[A-Za-z0-9._$-]+@[A-Za-z0-9._$-]+"
            r"|drda://[A-Za-z0-9._-]+(:[0-9]{1,5})?/[A-Za-z0-9._$-]+)"
            r"(\?db-locale=[A-Za-z0-9._-]+)?$"
        ),
        "host[:port]/database@server or drda://host[:port]/database",
    ),
}


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
    kind: str = "oracle",
) -> list[str]:
    check_name(name, "database")
    if kind not in KINDS:
        raise ReviewError("kind is oracle, postgres, clickhouse, sqlserver or informix")
    pattern, form = _CONNECT[kind]
    if not pattern.match(connect or ""):
        raise ReviewError(f"connect is {form}")
    if not _USER.match(user or ""):
        raise ReviewError("user: a database user name")
    what = {"clickhouse": "database names", "informix": "table owners"}.get(kind, "schema names")
    if not owners or any(not _OWNER.match(o) for o in owners) or len(owners) > 50:
        raise ReviewError(f"owners: one or more {what}")
    if profile not in PROFILES:
        raise ReviewError("profile is p0, p1 or p2")
    if dictionary not in (None, "idempiere"):
        raise ReviewError("dictionary is idempiere or none")
    # RFC 0256: iDempiere runs on Oracle and on PostgreSQL; its dictionary is read on both.
    if kind not in ("oracle", "postgres") and dictionary:
        raise ReviewError("an application dictionary is read for Oracle and PostgreSQL only")
    if not envs.is_valid_name(password_env):
        raise ReviewError("not a usable variable name")
    argv = [
        "database",
        "add",
        name,
        f"--kind={kind}",
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
