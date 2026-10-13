"""The environment a tenant's `ekos` processes run with (RFC 0183 §2).

Every command the console starts used to inherit the console's whole environment. With several
tenants that hands each tenant's projects every other tenant's model key (a project names the
variable holding its key in its own `ekos.toml`), and the console's own secrets with them.

`for_tenant` builds the environment instead: a fixed base, plus the variables a platform owner
named for that tenant, and those only while the tenant may make metered calls.
"""

from __future__ import annotations

import os
import re
from pathlib import Path

from . import home, models
from .settings import Settings, get_settings

# What any `ekos` process needs to run and to reach the network, and nothing that is a secret.
# `OLLAMA_HOST` is here on purpose: a local model server needs no key, so a tenant with metered
# calls off can still use one.
BASE = frozenset(
    {
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "SHELL",
        "LANG",
        "LANGUAGE",
        "TZ",
        "TMPDIR",
        "TEMP",
        "TMP",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "no_proxy",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "OLLAMA_HOST",
        "EKOS_HOME",
        "RUST_BACKTRACE",
    }
)
_BASE_PREFIXES = ("LC_",)

# The console's own settings: session secret, OIDC client secret, static tokens.
CONSOLE_PREFIX = "EKOS_CONSOLE_"

_NAME = re.compile(r"[A-Za-z_][A-Za-z0-9_]{0,127}")


def is_valid_name(name: str) -> bool:
    """A variable a tenant may be given: an environment variable name that is not one of the
    console's own."""
    return _NAME.fullmatch(name) is not None and not name.upper().startswith(CONSOLE_PREFIX)


def _granted(tenant: models.Tenant) -> set[str]:
    if not tenant.allow_metered:
        return set()
    return {name for name in tenant.model_env or [] if is_valid_name(name)}


def for_tenant(
    tenant: models.Tenant | None,
    settings: Settings | None = None,
    environ: dict[str, str] | None = None,
) -> dict[str, str]:
    """The environment for a process run on behalf of `tenant`.

    * The base, and the tenant's named variables when it may make metered calls.
    * Outside production mode the `default` tenant keeps the rest of the environment as well
      (an existing single-tenant install keeps working), minus the console's own variables and
      every variable named for another tenant.
    * An unknown tenant gets the base only.
    """
    settings = settings or get_settings()
    environ = dict(os.environ) if environ is None else environ

    def in_base(name: str) -> bool:
        return name in BASE or name.startswith(_BASE_PREFIXES)

    if tenant is None:
        return {k: v for k, v in environ.items() if in_base(k)}

    compat = tenant.id == models.DEFAULT_TENANT and not settings.production
    if compat:
        others: set[str] = set()
        for other in models.list_tenants():
            if other.id != tenant.id:
                others |= set(other.model_env or [])
        return {
            k: v
            for k, v in environ.items()
            if not k.upper().startswith(CONSOLE_PREFIX) and (k not in others or in_base(k))
        }

    granted = _granted(tenant)
    return {k: v for k, v in environ.items() if in_base(k) or k in granted}


def for_path(path: str) -> dict[str, str]:
    """The environment for a command run against the workspace at `path`: its tenant's."""
    project = home.project_at(path)
    tenant_id = project.tenant if project is not None else None
    if tenant_id is None:
        resolved = str(Path(path).expanduser().resolve())
        match = next((w for w in models.list_workspaces() if w.path == resolved), None)
        tenant_id = match.tenant_id if match is not None else None
    return for_tenant(models.get_tenant(tenant_id) if tenant_id else None)
