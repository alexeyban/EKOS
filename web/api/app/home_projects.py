"""Projects in the EKOS home that the registry does not hold yet (RFC 0179 §2).

A project exists because its directory does, so one created with the CLI in the same home
(`ekos project add`, `ekos-lakehouse lakehouse install`) is registered here. This runs at start-up
and whenever the project list is read (RFC 0199): a project added while the console runs appears
without a restart.
"""

from __future__ import annotations

import logging

from . import home, models

log = logging.getLogger("ekos.console")


def workspace_id_for(tenant_id: str, name: str) -> str:
    """A name cannot contain a dot, so `<tenant>.<name>` never collides with a `default` id."""
    return name if tenant_id == models.DEFAULT_TENANT else f"{tenant_id}.{name}"


def register_new() -> int:
    """Register every project of a known tenant that the registry lacks; how many were added."""
    root = home.configured_home()
    if root is None:
        return 0
    known = {ws.path for ws in models.list_workspaces()}
    added = 0
    # Only tenants the console has a row for: a directory under data/ is not a tenant until a
    # platform owner creates it (RFC 0180).
    for tenant in models.list_tenants():
        for project in home.projects_in(root, tenant.id):
            if str(project.dir) in known:
                continue
            workspace_id = workspace_id_for(tenant.id, project.name)
            if models.get_workspace(workspace_id) is not None:
                log.warning("project %r not registered: the id is taken", workspace_id)
                continue
            models.add_workspace(
                models.Workspace(
                    id=workspace_id, name=project.name, path=str(project.dir), tenant_id=tenant.id
                )
            )
            added += 1
    return added
