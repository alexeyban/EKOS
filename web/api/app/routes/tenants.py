"""Tenants and who may do what in them (RFC 0180): members, groups, role bindings, API tokens,
the audit log. Everything here needs `admin` on the tenant, except creating a tenant and setting
its source roots, which need a platform owner."""

from __future__ import annotations

import re
import uuid
from datetime import timedelta
from pathlib import Path
from typing import Any, Literal

from fastapi import APIRouter, Depends, HTTPException, Query
from pydantic import BaseModel, Field, model_validator

from .. import authz, home, models, roots
from ..auth import Principal, new_token
from ..settings import Settings, get_settings
from ._responses import BAD_REQUEST, CONFLICT, NOT_FOUND

router = APIRouter(prefix="/tenants", tags=["tenants"])

_ADMIN = authz.allow_tenant("admin")

Role = Literal["viewer", "reviewer", "operator", "admin"]


def _id() -> str:
    return uuid.uuid4().hex[:16]


def _iso(moment) -> str | None:
    return moment.isoformat() if moment else None


# ── tenants ──────────────────────────────────────────────────────────────────


class TenantIn(BaseModel):
    id: str
    name: str
    source_roots: list[str] = []


class TenantPatch(BaseModel):
    name: str | None = None
    source_roots: list[str] | None = None


def _tenant_out(tenant: models.Tenant, principal: Principal) -> dict[str, Any]:
    roles = authz.roles_in(principal, tenant.id)
    out: dict[str, Any] = {"id": tenant.id, "name": tenant.name, "roles": sorted(roles)}
    # Host paths are for whoever administers the tenant, not for every viewer of it.
    if principal.owner or "admin" in roles:
        out["source_roots"] = tenant.source_roots
    return out


def _checked_roots(raw: list[str], settings: Settings) -> list[str]:
    """Absolute, existing directories, inside the console-wide root when there is one."""
    console_root = (
        Path(settings.workspaces_root).expanduser().resolve() if settings.workspaces_root else None
    )
    out = []
    for item in raw:
        path = Path(item).expanduser()
        if not path.is_absolute():
            raise HTTPException(status_code=400, detail=f"{item} is not an absolute path")
        resolved = path.resolve()
        if not resolved.is_dir():
            raise HTTPException(status_code=400, detail=f"{resolved} is not a directory")
        if console_root is not None and not roots.inside(resolved, console_root):
            raise HTTPException(
                status_code=400,
                detail=f"{resolved} is outside the configured workspaces root {console_root}",
            )
        out.append(str(resolved))
    return sorted(set(out))


@router.get("")
async def list_tenants(principal: Principal = Depends(authz.authenticated)) -> list[dict]:
    return [
        _tenant_out(t, principal)
        for t in models.list_tenants()
        if principal.owner or authz.sees_tenant(principal, t.id)
    ]


@router.post("", status_code=201, responses={400: BAD_REQUEST, 409: CONFLICT})
async def create_tenant(
    body: TenantIn,
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(authz.allow_platform),
) -> dict:
    if not home.is_valid_name(body.id):
        raise HTTPException(
            status_code=400,
            detail="a tenant id is 1-63 lowercase letters, digits, `-` or `_`, "
            "starting with a letter or digit",
        )
    if models.get_tenant(body.id) is not None:
        raise HTTPException(status_code=409, detail=f"tenant {body.id!r} already exists")
    tenant = models.Tenant(
        id=body.id, name=body.name, source_roots=_checked_roots(body.source_roots, settings)
    )
    models.add(tenant)
    return _tenant_out(tenant, principal)


@router.patch("/{tenant_id}", responses={400: BAD_REQUEST, 404: NOT_FOUND})
async def patch_tenant(
    tenant_id: str,
    body: TenantPatch,
    settings: Settings = Depends(get_settings),
    principal: Principal = Depends(authz.allow_platform),
) -> dict:
    fields: dict[str, Any] = {}
    if body.name is not None:
        fields["name"] = body.name
    if body.source_roots is not None:
        fields["source_roots"] = _checked_roots(body.source_roots, settings)
    tenant = models.update_tenant(tenant_id, **fields)
    if tenant is None:
        raise HTTPException(status_code=404, detail=f"unknown tenant {tenant_id!r}")
    return _tenant_out(tenant, principal)


# ── bindings and members ─────────────────────────────────────────────────────


class BindingIn(BaseModel):
    """`role` for exactly one of: a person by e-mail, a group, an API token."""

    role: Role
    email: str | None = None
    group_id: str | None = None
    token_id: str | None = None
    # Omitted: the whole tenant.
    workspace_id: str | None = None
    # RFC 0181: a rule narrower than the workspace. `allow` with a rule shows only what it
    # matches; `deny` hides what it matches from someone who otherwise sees the workspace.
    effect: Literal["allow", "deny"] = "allow"
    source: str | None = None
    path: str | None = None
    object: str | None = None

    @model_validator(mode="after")
    def _one_principal(self) -> BindingIn:
        if sum(v is not None for v in (self.email, self.group_id, self.token_id)) != 1:
            raise ValueError("give exactly one of `email`, `group_id` and `token_id`")
        return self

    @property
    def has_rule(self) -> bool:
        return bool(self.source or self.path or self.object)


def _user_for_email(email: str) -> models.User:
    """The user with this address, created without a subject when they have not signed in yet."""
    address = email.strip().lower()
    if "@" not in address or len(address) > 254:
        raise HTTPException(status_code=400, detail="not an e-mail address")
    user = models.user_by_email(address)
    if user is None:
        user = models.User(id=_id(), email=address)
        models.add(user)
    return user


def _binding_out(b: models.RoleBinding) -> dict[str, Any]:
    label = b.principal_id
    if b.principal_kind == "user":
        user = models.get_user(b.principal_id)
        label = (user.email or user.subject) if user else b.principal_id
    elif b.principal_kind == "group":
        group = models.get_group(b.principal_id)
        label = group.name if group else b.principal_id
    elif b.principal_kind == "token":
        token = models.get_token(b.principal_id)
        label = token.name if token else b.principal_id
    return {
        "id": b.id,
        "principal_kind": b.principal_kind,
        "principal_id": b.principal_id,
        "principal": label,
        "role": b.role,
        "workspace_id": b.workspace_id,
        "effect": b.effect,
        "source": b.source,
        "path": b.path,
        "object": b.object_id,
        "created_by": b.created_by,
        "created_at": _iso(b.created_at),
    }


_UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")


def _check_rule(body: BindingIn) -> None:
    """A rule (RFC 0181) is for one workspace and the `viewer` role; the `ekos` MCP server
    parses it again and refuses the call if it is malformed, so this is for a clear message."""
    if not body.has_rule:
        if body.effect == "deny":
            raise HTTPException(
                status_code=400,
                detail="a deny binding names what it hides: a source, a path or an object",
            )
        return
    if body.workspace_id is None:
        raise HTTPException(status_code=400, detail="a rule applies to one workspace")
    if body.role != "viewer":
        raise HTTPException(
            status_code=400,
            detail="a rule goes with the viewer role: reviewing, operating and administering "
            "need the whole project",
        )
    if body.object is not None:
        if body.source or body.path:
            raise HTTPException(
                status_code=400, detail="a rule names an object, or a source and path, not both"
            )
        if _UUID.fullmatch(body.object) is None:
            raise HTTPException(status_code=400, detail="`object` is an object id (a UUID)")
    if body.source is not None and not home.is_valid_name(body.source):
        raise HTTPException(status_code=400, detail="`source` is a source name")
    if body.path is not None and (
        not body.path.strip() or len(body.path) > 500 or "\x00" in body.path
    ):
        raise HTTPException(status_code=400, detail="`path` is a pattern such as `secrets/**`")


def _unrestrictable_note(kind: str, principal_id: str, body: BindingIn) -> str | None:
    """Said when a rule is given to someone it cannot bind: an operator or admin of the same
    workspace sees every source through the pipeline's own output."""
    if not body.has_rule or body.workspace_id is None:
        return None
    ws = models.get_workspace(body.workspace_id)
    if ws is None:
        return None
    held = {
        b.role
        for b in models.bindings_for([(kind, principal_id)])
        if b.tenant_id == ws.tenant_id
        and b.workspace_id in (None, ws.id)
        and b.effect != "deny"
        and not authz.has_rule(b)
    }
    if held & authz.UNRESTRICTABLE_ROLES:
        return (
            "this principal is an operator or admin of the workspace, and those roles see every "
            "source; the rule has no effect while they hold one"
        )
    return None


def _check_scope(tenant_id: str, role: str, workspace_id: str | None) -> None:
    if workspace_id is None:
        return
    if role in authz.TENANT_ONLY_ROLES:
        raise HTTPException(status_code=400, detail=f"`{role}` is granted on a tenant only")
    ws = models.get_workspace(workspace_id)
    if ws is None or ws.tenant_id != tenant_id:
        raise HTTPException(status_code=400, detail="no such workspace in this tenant")


@router.get("/{tenant_id}/bindings", dependencies=[Depends(_ADMIN)])
async def list_bindings(tenant_id: str) -> list[dict]:
    return [_binding_out(b) for b in models.list_bindings(tenant_id)]


@router.post("/{tenant_id}/bindings", status_code=201, responses={400: BAD_REQUEST})
async def create_binding(
    tenant_id: str, body: BindingIn, principal: Principal = Depends(_ADMIN)
) -> dict:
    _check_scope(tenant_id, body.role, body.workspace_id)
    _check_rule(body)
    if body.email is not None:
        kind, pid = "user", _user_for_email(body.email).id
    elif body.group_id is not None:
        group = models.get_group(body.group_id)
        if group is None or group.tenant_id != tenant_id:
            raise HTTPException(status_code=400, detail="no such group in this tenant")
        kind, pid = "group", group.id
    else:
        token = models.get_token(body.token_id or "")
        if token is None or token.tenant_id != tenant_id:
            raise HTTPException(status_code=400, detail="no such token in this tenant")
        if body.role not in authz.TOKEN_ROLES:
            raise HTTPException(
                status_code=400,
                detail="a token can be viewer or operator; review and administration are for "
                "signed-in people",
            )
        kind, pid = "token", token.id
    row = models.RoleBinding(
        id=_id(),
        tenant_id=tenant_id,
        principal_kind=kind,
        principal_id=pid,
        role=body.role,
        workspace_id=body.workspace_id,
        effect=body.effect,
        source=body.source,
        path=body.path.strip() if body.path else None,
        object_id=body.object,
        created_by=principal.label,
    )
    models.add(row)
    out = _binding_out(row)
    note = _unrestrictable_note(kind, pid, body)
    if note:
        out["warning"] = note
    return out


@router.delete(
    "/{tenant_id}/bindings/{binding_id}",
    status_code=204,
    dependencies=[Depends(_ADMIN)],
    responses={404: NOT_FOUND},
)
async def delete_binding(tenant_id: str, binding_id: str) -> None:
    row = models.get_binding(binding_id)
    if row is None or row.tenant_id != tenant_id:
        raise HTTPException(status_code=404, detail="no such binding")
    models.delete_binding(binding_id)


@router.get("/{tenant_id}/members", dependencies=[Depends(_ADMIN)])
async def list_members(tenant_id: str) -> list[dict]:
    """Every person with a binding in this tenant, directly or through a group with members."""
    people: dict[str, dict[str, Any]] = {}

    def entry(user_id: str) -> dict[str, Any] | None:
        if user_id not in people:
            user = models.get_user(user_id)
            if user is None:
                return None
            people[user_id] = {
                "id": user.id,
                "email": user.email,
                "signed_in": bool(user.subject),
                "last_login_at": _iso(user.last_login_at),
                "roles": [],
            }
        return people[user_id]

    for b in models.list_bindings(tenant_id):
        via = None
        if b.principal_kind == "user":
            ids = [b.principal_id]
        elif b.principal_kind == "group":
            group = models.get_group(b.principal_id)
            via = group.name if group else b.principal_id
            ids = models.group_members(b.principal_id)
        else:
            continue
        for user_id in ids:
            person = entry(user_id)
            if person is not None:
                person["roles"].append({"role": b.role, "workspace_id": b.workspace_id, "via": via})
    return sorted(people.values(), key=lambda p: p["email"])


# ── groups ───────────────────────────────────────────────────────────────────


class GroupIn(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    oidc_value: str = ""


class MemberIn(BaseModel):
    email: str


def _group_out(g: models.Group) -> dict[str, Any]:
    members = []
    for user_id in models.group_members(g.id):
        user = models.get_user(user_id)
        if user is not None:
            members.append({"id": user.id, "email": user.email})
    return {"id": g.id, "name": g.name, "oidc_value": g.oidc_value, "members": members}


def _group_in(tenant_id: str, group_id: str) -> models.Group:
    group = models.get_group(group_id)
    if group is None or group.tenant_id != tenant_id:
        raise HTTPException(status_code=404, detail="no such group")
    return group


@router.get("/{tenant_id}/groups", dependencies=[Depends(_ADMIN)])
async def list_groups(tenant_id: str) -> list[dict]:
    return [_group_out(g) for g in models.list_groups(tenant_id)]


@router.post("/{tenant_id}/groups", status_code=201, dependencies=[Depends(_ADMIN)])
async def create_group(tenant_id: str, body: GroupIn) -> dict:
    group = models.Group(
        id=_id(), tenant_id=tenant_id, name=body.name.strip(), oidc_value=body.oidc_value.strip()
    )
    models.add(group)
    return _group_out(group)


@router.delete(
    "/{tenant_id}/groups/{group_id}",
    status_code=204,
    dependencies=[Depends(_ADMIN)],
    responses={404: NOT_FOUND},
)
async def delete_group(tenant_id: str, group_id: str) -> None:
    _group_in(tenant_id, group_id)
    models.delete_group(group_id)


@router.post(
    "/{tenant_id}/groups/{group_id}/members",
    dependencies=[Depends(_ADMIN)],
    responses={400: BAD_REQUEST, 404: NOT_FOUND},
)
async def add_group_member(tenant_id: str, group_id: str, body: MemberIn) -> dict:
    group = _group_in(tenant_id, group_id)
    models.set_group_member(group.id, _user_for_email(body.email).id, True)
    return _group_out(group)


@router.delete(
    "/{tenant_id}/groups/{group_id}/members/{user_id}",
    dependencies=[Depends(_ADMIN)],
    responses={404: NOT_FOUND},
)
async def remove_group_member(tenant_id: str, group_id: str, user_id: str) -> dict:
    group = _group_in(tenant_id, group_id)
    models.set_group_member(group.id, user_id, False)
    return _group_out(group)


# ── API tokens ───────────────────────────────────────────────────────────────


class TokenIn(BaseModel):
    name: str = Field(min_length=1, max_length=100)
    # Not `Role`: a token is never a reviewer or an admin.
    role: Literal["viewer", "operator"]
    workspace_id: str | None = None
    expires_days: int | None = Field(default=None, ge=1, le=3650)


def _token_out(t: models.ApiToken) -> dict[str, Any]:
    roles = [
        {"role": b.role, "workspace_id": b.workspace_id}
        for b in models.bindings_for([("token", t.id)])
    ]
    return {
        "id": t.id,
        "name": t.name,
        "prefix": t.prefix,
        "roles": roles,
        "created_by": t.created_by,
        "created_at": _iso(t.created_at),
        "expires_at": _iso(t.expires_at),
        "last_used_at": _iso(t.last_used_at),
        "revoked": t.revoked_at is not None,
    }


@router.get("/{tenant_id}/tokens", dependencies=[Depends(_ADMIN)])
async def list_tokens(tenant_id: str) -> list[dict]:
    return [_token_out(t) for t in models.list_tokens(tenant_id)]


@router.post("/{tenant_id}/tokens", status_code=201, responses={400: BAD_REQUEST})
async def create_token(
    tenant_id: str, body: TokenIn, principal: Principal = Depends(_ADMIN)
) -> dict:
    """Create a token and its one binding. The token value is in this response and nowhere
    else: only its hash is stored."""
    _check_scope(tenant_id, body.role, body.workspace_id)
    token, token_hash, prefix = new_token()
    row = models.ApiToken(
        id=_id(),
        tenant_id=tenant_id,
        name=body.name.strip(),
        token_hash=token_hash,
        prefix=prefix,
        created_by=principal.label,
        expires_at=(models._now() + timedelta(days=body.expires_days))
        if body.expires_days
        else None,
    )
    models.add(row)
    models.add(
        models.RoleBinding(
            id=_id(),
            tenant_id=tenant_id,
            principal_kind="token",
            principal_id=row.id,
            role=body.role,
            workspace_id=body.workspace_id,
            created_by=principal.label,
        )
    )
    return {**_token_out(row), "token": token}


@router.delete(
    "/{tenant_id}/tokens/{token_id}",
    status_code=204,
    dependencies=[Depends(_ADMIN)],
    responses={404: NOT_FOUND},
)
async def revoke_token(tenant_id: str, token_id: str) -> None:
    row = models.get_token(token_id)
    if row is None or row.tenant_id != tenant_id:
        raise HTTPException(status_code=404, detail="no such token")
    if row.revoked_at is None:
        models.update_token(token_id, revoked_at=models._now())


# ── audit ────────────────────────────────────────────────────────────────────


@router.get("/{tenant_id}/audit", dependencies=[Depends(_ADMIN)])
async def audit_log(tenant_id: str, limit: int = Query(200, ge=1, le=1000)) -> list[dict]:
    return [
        {
            "at": _iso(a.at),
            "actor": a.actor,
            "actor_kind": a.actor_kind,
            "action": a.action,
            "workspace_id": a.workspace_id,
            "outcome": a.outcome,
            "detail": a.detail,
        }
        for a in models.list_audit(tenant_id, limit)
    ]
