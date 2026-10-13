"""What a principal may do (RFC 0180).

`authorize` is the only place a role is compared. Every route reaches it through one of the
dependencies below (`authenticated`, `allow`, `allow_tenant`, `allow_platform`), each of which
carries the action it enforces in `ACTION_ATTR`; `tests/test_authz_routes.py` walks the
registered routes and fails on one that declares none.

Answers follow one rule: a workspace or tenant the caller has no role in is 404, exactly as if
it did not exist. 403 is for a caller who can see it and lacks the role.
"""

from __future__ import annotations

from typing import Literal

from fastapi import Depends, Header, HTTPException, Request

from . import models
from .auth import Principal, resolve_principal
from .settings import Settings, get_settings

Action = Literal["read", "review", "operate", "admin", "platform"]

ROLES = ("viewer", "reviewer", "operator", "admin")
ROLE_ACTIONS: dict[str, frozenset[str]] = {
    "viewer": frozenset({"read"}),
    "reviewer": frozenset({"read", "review"}),
    "operator": frozenset({"read", "operate"}),
    "admin": frozenset({"read", "review", "operate", "admin"}),
}
# Review decisions and administration are for people. A token is an agent's credential: it may
# hold these roles and perform these actions and no others, whatever a binding row says.
TOKEN_ROLES = frozenset({"viewer", "operator"})
TOKEN_ACTIONS = frozenset({"read", "operate"})
# `admin` is about a tenant's members and projects; it has no meaning on one project.
TENANT_ONLY_ROLES = frozenset({"admin"})

ACTION_ATTR = "__ekos_action__"


# ── the decision ─────────────────────────────────────────────────────────────


def _bindings(principal: Principal) -> list[models.RoleBinding]:
    if principal.kind == "token":
        return models.bindings_for([("token", principal.id)])
    if principal.kind == "user":
        holders = [("user", principal.id)]
        holders += [("group", g) for g in models.groups_of(principal.id, set(principal.claims))]
        return models.bindings_for(holders)
    return []


def _compat_roles(principal: Principal, tenant_id: str) -> set[str]:
    """RFC 0131's mapping, which applies to the `default` tenant only and is absent in
    production mode (`Principal.compat` is then `None`)."""
    if tenant_id != models.DEFAULT_TENANT or principal.compat is None:
        return set()
    return {"viewer", "operator", "reviewer"} if principal.compat == "write" else {"viewer"}


def roles_in(principal: Principal, tenant_id: str, workspace_id: str | None = None) -> set[str]:
    """The roles `principal` holds on the tenant, or on one workspace of it."""
    if principal.kind == "token":
        if principal.tenant_id != tenant_id:
            return set()
    elif principal.owner:
        return set(ROLES)

    roles = _compat_roles(principal, tenant_id)
    for b in _bindings(principal):
        if b.tenant_id != tenant_id or b.role not in ROLE_ACTIONS:
            continue
        if b.workspace_id is not None and b.workspace_id != workspace_id:
            continue
        roles.add(b.role)
    return roles & TOKEN_ROLES if principal.kind == "token" else roles


def authorize(
    principal: Principal, action: str, tenant_id: str, workspace_id: str | None = None
) -> bool:
    if principal.kind == "token" and action not in TOKEN_ACTIONS:
        return False
    if action == "platform":
        return principal.owner
    return any(action in ROLE_ACTIONS[r] for r in roles_in(principal, tenant_id, workspace_id))


def sees_tenant(principal: Principal, tenant_id: str) -> bool:
    """True when the principal has any role in the tenant, on it or on one of its workspaces."""
    if roles_in(principal, tenant_id):
        return True
    return any(b.tenant_id == tenant_id for b in _bindings(principal)) and (
        principal.kind != "token" or principal.tenant_id == tenant_id
    )


def can_anywhere(principal: Principal, action: str) -> bool:
    """True when the principal may do `action` on at least one workspace. Routes that look a
    thing up by its own id (a run, a schedule) check this first, so a caller without the role
    gets 403 for every id and cannot probe which ones exist."""
    if principal.kind != "token" and principal.owner:
        return True
    return any(authorize(principal, action, ws.tenant_id, ws.id) for ws in models.list_workspaces())


def require_anywhere(principal: Principal, action: str) -> None:
    if not can_anywhere(principal, action):
        raise HTTPException(status_code=403, detail=f"this needs the right to {action}")


def sees_workspace(principal: Principal, ws: models.Workspace) -> bool:
    return bool(roles_in(principal, ws.tenant_id, ws.id))


# ── audit ────────────────────────────────────────────────────────────────────


def audit(
    principal: Principal | None,
    action: str,
    outcome: str,
    *,
    tenant_id: str = "",
    workspace_id: str = "",
    detail: str = "",
) -> None:
    models.add(
        models.AuditLog(
            actor=principal.label if principal else "",
            actor_kind=principal.kind if principal else "anonymous",
            action=action,
            tenant_id=tenant_id,
            workspace_id=workspace_id,
            outcome=outcome,
            detail=detail[:500],
        )
    )


def require(
    principal: Principal,
    action: str,
    tenant_id: str,
    workspace_id: str | None = None,
    *,
    request: Request | None = None,
) -> None:
    """Raise 403 unless `principal` may do `action`. A request that changes something is written
    to the audit log with its outcome, and so is every refusal beyond `read`. An allowed `GET`
    is not: an admin opening the member list has changed nothing, and logging it would bury
    the entries that matter."""
    ok = authorize(principal, action, tenant_id, workspace_id)
    changes = request is None or request.method != "GET"
    if action != "read" and (changes or not ok):
        what = f"{action}: {request.method} {request.url.path}" if request is not None else action
        audit(
            principal,
            what,
            "allowed" if ok else "denied",
            tenant_id=tenant_id,
            workspace_id=workspace_id or "",
        )
    if not ok:
        raise HTTPException(status_code=403, detail=f"this needs the right to {action}")


# ── dependencies ─────────────────────────────────────────────────────────────


def _declares(action: str):
    def mark(fn):
        setattr(fn, ACTION_ATTR, action)
        return fn

    return mark


# Async on purpose throughout: these run on the event loop rather than a thread-pool hop per
# request.
@_declares("authenticated")
async def authenticated(  # NOSONAR(python:S7503)
    request: Request,
    authorization: str = Header(default=""),
    settings: Settings = Depends(get_settings),
) -> Principal:
    """Any signed-in principal. For routes that filter their own result (a list) or decide the
    action from the request body; such a handler calls `require` itself."""
    principal = resolve_principal(request, authorization, settings)
    if principal is None:
        raise HTTPException(status_code=401, detail="not authenticated")
    return principal


def workspace_for(principal: Principal, workspace_id: str) -> models.Workspace:
    """The workspace, or 404 when it does not exist or the principal has no role on it."""
    ws = models.get_workspace(workspace_id)
    if ws is None or not sees_workspace(principal, ws):
        raise HTTPException(status_code=404, detail=f"unknown workspace {workspace_id!r}")
    return ws


async def require_workspace(  # NOSONAR(python:S7503)
    workspace_id: str, principal: Principal = Depends(authenticated)
) -> models.Workspace:
    return workspace_for(principal, workspace_id)


def allow(action: Action):
    """A workspace route (`{workspace_id}` in the path): the caller may do `action` there."""

    @_declares(action)
    async def dependency(  # NOSONAR(python:S7503)
        request: Request,
        principal: Principal = Depends(authenticated),
        ws: models.Workspace = Depends(require_workspace),
    ) -> Principal:
        require(principal, action, ws.tenant_id, ws.id, request=request)
        return principal

    return dependency


def tenant_for(principal: Principal, tenant_id: str) -> models.Tenant:
    tenant = models.get_tenant(tenant_id)
    if tenant is None or not (principal.owner or sees_tenant(principal, tenant_id)):
        raise HTTPException(status_code=404, detail=f"unknown tenant {tenant_id!r}")
    return tenant


def allow_tenant(action: Action):
    """A tenant route (`{tenant_id}` in the path): the caller may do `action` on the tenant."""

    @_declares(f"tenant:{action}")
    async def dependency(  # NOSONAR(python:S7503)
        tenant_id: str,
        request: Request,
        principal: Principal = Depends(authenticated),
    ) -> Principal:
        tenant_for(principal, tenant_id)
        require(principal, action, tenant_id, request=request)
        return principal

    return dependency


@_declares("platform")
async def allow_platform(  # NOSONAR(python:S7503)
    request: Request, principal: Principal = Depends(authenticated)
) -> Principal:
    require(principal, "platform", "", request=request)
    return principal
