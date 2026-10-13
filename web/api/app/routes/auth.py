"""Auth endpoints (RFC 0131 §1, RFC 0180). OIDC sign-in, and a token sign-in for development;
both land a signed session cookie that names the principal and nothing about its rights."""

from __future__ import annotations

import uuid

from fastapi import APIRouter, Depends, Header, HTTPException, Request
from fastapi.responses import RedirectResponse
from pydantic import BaseModel

from .. import authz, models
from ..auth import claim_values, oauth, resolve_principal, static_role
from ..settings import Settings, get_settings
from ._responses import BAD_REQUEST, UNAUTHORIZED

router = APIRouter(prefix="/auth", tags=["auth"])


class TokenLogin(BaseModel):
    token: str


def _mode(settings: Settings) -> str:
    return "oidc" if settings.oidc_enabled else "token"


@router.get("/me", responses={401: UNAUTHORIZED})
async def me(
    request: Request,
    authorization: str = Header(default=""),
    settings: Settings = Depends(get_settings),
) -> dict:
    principal = resolve_principal(request, authorization, settings)
    if principal is None:
        raise HTTPException(status_code=401, detail={"mode": _mode(settings)})
    tenants = []
    for tenant in models.list_tenants():
        if not (principal.owner or authz.sees_tenant(principal, tenant.id)):
            continue
        tenants.append(
            {
                "id": tenant.id,
                "name": tenant.name,
                "roles": sorted(authz.roles_in(principal, tenant.id)),
            }
        )
    # `role` is the two-value summary older UI code reads: "write" when the caller may change
    # anything anywhere. It grants nothing; every route checks its own action.
    writes = (
        principal.owner
        or authz.can_anywhere(principal, "operate")
        or authz.can_anywhere(principal, "review")
        or any("admin" in t["roles"] for t in tenants)
    )
    return {
        "mode": _mode(settings),
        "kind": principal.kind,
        "email": principal.email,
        "role": "write" if writes else "read",
        "owner": principal.owner,
        "tenants": tenants,
    }


@router.post("/logout")
async def logout(request: Request) -> dict:
    request.session.pop("user", None)
    return {"ok": True}


@router.post("/token-login", responses={400: BAD_REQUEST, 401: UNAUTHORIZED})
async def token_login(
    body: TokenLogin,
    request: Request,
    settings: Settings = Depends(get_settings),
) -> dict:
    if settings.oidc_enabled:
        raise HTTPException(status_code=400, detail="OIDC is configured; use /api/auth/login")
    if settings.production:
        raise HTTPException(status_code=400, detail="static tokens are off in production mode")
    role = static_role(body.token, settings)
    if role is None:
        authz.audit(None, "sign-in", "denied", detail="static token")
        raise HTTPException(status_code=401, detail="invalid token")
    request.session["user"] = {"kind": "static", "id": role}
    return {"role": role}


@router.get("/login", responses={400: BAD_REQUEST})
async def login(request: Request, settings: Settings = Depends(get_settings)):
    if not settings.oidc_enabled:
        raise HTTPException(status_code=400, detail="OIDC is not configured")
    return await oauth(settings).oidc.authorize_redirect(request, settings.oidc_redirect_uri)


def user_for_claims(claims: dict) -> models.User:
    """The `User` this sign-in is: by subject, else a record made ahead of time for the same
    e-mail (RFC 0180), else a new one."""
    subject = str(claims.get("sub", "") or "")
    if not subject:
        raise HTTPException(status_code=400, detail="the identity provider sent no subject")
    email = str(claims.get("email", "") or "").lower()
    now = models._now()

    user = models.user_by_subject(subject)
    if user is not None:
        models.touch_user(user.id, last_login_at=now, **({"email": email} if email else {}))
        return models.get_user(user.id) or user

    # An address the provider says is unverified cannot claim a record made for that address.
    verified = claims.get("email_verified") is not False
    pending = models.user_by_email(email) if email and verified else None
    if pending is not None and not pending.subject:
        models.touch_user(pending.id, subject=subject, last_login_at=now)
        return models.get_user(pending.id) or pending

    user = models.User(id=uuid.uuid4().hex[:16], subject=subject, email=email, last_login_at=now)
    models.add(user)
    return user


@router.get("/callback", responses={400: BAD_REQUEST})
async def callback(request: Request, settings: Settings = Depends(get_settings)):
    if not settings.oidc_enabled:
        raise HTTPException(status_code=400, detail="OIDC is not configured")
    token = await oauth(settings).oidc.authorize_access_token(request)
    claims = token.get("userinfo") or {}
    user = user_for_claims(claims)
    request.session["user"] = {
        "kind": "user",
        "id": user.id,
        "claims": claim_values(claims, settings.oidc_role_claim),
    }
    authz.audit(None, "sign-in", "ok", detail=user.email or user.subject)
    return RedirectResponse(settings.post_login_redirect, status_code=302)
