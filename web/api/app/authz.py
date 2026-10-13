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
# Set on an `allow(...)` dependency: whether the route is safe for a restricted reader.
SCOPED_ATTR = "__ekos_scoped__"


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


def has_rule(b: models.RoleBinding) -> bool:
    """True for a binding narrower than its workspace (RFC 0181)."""
    return bool(b.source or b.path or b.object_id)


def _applies(b: models.RoleBinding, tenant_id: str, workspace_id: str | None) -> bool:
    if b.tenant_id != tenant_id or b.role not in ROLE_ACTIONS:
        return False
    return b.workspace_id is None or b.workspace_id == workspace_id


def _plain_roles(principal: Principal, tenant_id: str, workspace_id: str | None) -> set[str]:
    """Roles held outright: the compatibility mapping and bindings that carry no rule."""
    roles = _compat_roles(principal, tenant_id)
    for b in _bindings(principal):
        if _applies(b, tenant_id, workspace_id) and b.effect != "deny" and not has_rule(b):
            roles.add(b.role)
    return roles & TOKEN_ROLES if principal.kind == "token" else roles


def roles_in(principal: Principal, tenant_id: str, workspace_id: str | None = None) -> set[str]:
    """The roles `principal` holds on the tenant, or on one workspace of it. A binding that
    allows part of a workspace (RFC 0181) makes its holder a viewer of it; what they then see
    is `scope_for`'s business. A deny binding grants nothing."""
    if principal.kind == "token":
        if principal.tenant_id != tenant_id:
            return set()
    elif principal.owner:
        return set(ROLES)

    roles = _plain_roles(principal, tenant_id, workspace_id)
    for b in _bindings(principal):
        if (
            _applies(b, tenant_id, workspace_id)
            and has_rule(b)
            and b.effect == "allow"
            and b.workspace_id == workspace_id
            and b.role == "viewer"
        ):
            roles.add("viewer")
    return roles


def _rule(b: models.RoleBinding) -> dict[str, str]:
    rule = {"effect": "deny" if b.effect == "deny" else "allow"}
    if b.object_id:
        rule["object"] = b.object_id
        return rule
    if b.source:
        rule["source"] = b.source
    if b.path:
        rule["path"] = b.path
    return rule


# Roles whose work shows every source anyway: running the pipeline prints file names, and its
# logs are theirs to read. A rule on such a principal would promise what it cannot deliver.
UNRESTRICTABLE_ROLES = frozenset({"operator", "admin"})


def scope_for(principal: Principal, ws: models.Workspace) -> dict | None:
    """The read scope (RFC 0181) for this principal on this workspace, in the form the `ekos`
    MCP server takes; `None` when they see all of it.

    * An owner, an operator or an admin is never restricted.
    * Someone with a plain role and deny bindings sees everything but what those hide.
    * Someone whose only access is rule-carrying allow bindings sees only what those match.
    """
    if principal.kind != "token" and principal.owner:
        return None
    plain = _plain_roles(principal, ws.tenant_id, ws.id)
    if plain & UNRESTRICTABLE_ROLES:
        return None
    rules = [
        b
        for b in _bindings(principal)
        if _applies(b, ws.tenant_id, ws.id) and has_rule(b) and b.workspace_id == ws.id
    ]
    denies = [_rule(b) for b in rules if b.effect == "deny"]
    allows = [_rule(b) for b in rules if b.effect != "deny" and b.role == "viewer"]
    if plain:
        return {"default": "allow", "rules": denies} if denies else None
    # No plain role: only what the allow rules name. With none, nothing (and `sees_workspace`
    # is already false for such a principal).
    return {"default": "deny", "rules": allows + denies}


def is_restricted(principal: Principal, ws: models.Workspace) -> bool:
    return scope_for(principal, ws) is not None


def sees_all_of(principal: Principal, ws: models.Workspace) -> bool:
    """Visible and unrestricted: the test for anything that shows more than the ledger (run
    logs, the config, command output)."""
    return sees_workspace(principal, ws) and not is_restricted(principal, ws)


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
    return any(b.tenant_id == tenant_id and b.effect != "deny" for b in _bindings(principal)) and (
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


RESTRICTED_DETAIL = "not available with restricted access to this project"


def require_all_of(principal: Principal, ws: models.Workspace) -> None:
    """403 for a principal who sees only part of the workspace (RFC 0181)."""
    if is_restricted(principal, ws):
        raise HTTPException(status_code=403, detail=RESTRICTED_DETAIL)


def allow(action: Action, *, scoped: bool = False):
    """A workspace route (`{workspace_id}` in the path): the caller may do `action` there.

    `scoped=True` marks a route that reads knowledge only through the workspace's MCP server
    with the caller's scope attached (`deps.mcp_for_workspace`), so it is safe for someone who
    sees part of the workspace. Every other route refuses such a caller: it runs a command or
    reads a file that shows more than the ledger, and nothing there would apply the scope.
    """

    @_declares(action)
    async def dependency(  # NOSONAR(python:S7503)
        request: Request,
        principal: Principal = Depends(authenticated),
        ws: models.Workspace = Depends(require_workspace),
    ) -> Principal:
        require(principal, action, ws.tenant_id, ws.id, request=request)
        if not scoped:
            require_all_of(principal, ws)
        return principal

    setattr(dependency, SCOPED_ATTR, scoped)
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
