"""Which host directories a tenant's projects may read (RFC 0179 §2, RFC 0180).

A project's sources are directories on the host. Without a limit, anyone who can create a project
or edit its config could point it at any path the console's user can read.
"""

from __future__ import annotations

from pathlib import Path

from . import models
from .settings import Settings


def inside(path: Path, root: Path) -> bool:
    return path == root or root in path.parents


def _resolved(raw: str) -> Path:
    return Path(raw).expanduser().resolve()


def allowed_roots(tenant: models.Tenant, settings: Settings) -> list[Path] | None:
    """The directories a project of `tenant` may read, or `None` for no limit.

    * A tenant with source roots: those, minus any that fall outside the console-wide
      `EKOS_CONSOLE_WORKSPACES_ROOT`.
    * `default` without roots: the console-wide root, or no limit when there is none (a
      single-user install).
    * Any other tenant without roots: nothing. Only a platform owner sets roots, so a new
      tenant's admin cannot reach a host path nobody granted.
    """
    console_root = _resolved(settings.workspaces_root) if settings.workspaces_root else None
    if tenant.source_roots:
        roots = [_resolved(r) for r in tenant.source_roots]
        return [r for r in roots if console_root is None or inside(r, console_root)]
    if tenant.id == models.DEFAULT_TENANT:
        return [console_root] if console_root is not None else None
    return []


def is_allowed(path: Path, roots: list[Path] | None) -> bool:
    return roots is None or any(inside(path, r) for r in roots)


def describe(roots: list[Path] | None) -> str:
    if not roots:
        return "this tenant has no source roots; a platform owner sets them"
    return "the allowed source roots are " + ", ".join(str(r) for r in roots)
