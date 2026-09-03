"""Auth endpoints (RFC 0131 §1). OIDC login flow + a token-login for the fallback mode; both
land a signed session cookie."""

from __future__ import annotations

import secrets

from fastapi import APIRouter, Depends, Header, HTTPException, Request
from fastapi.responses import RedirectResponse
from pydantic import BaseModel

from ..auth import Principal, oauth, resolve_principal, role_for_claims
from ..settings import Settings, get_settings

router = APIRouter(prefix="/auth", tags=["auth"])


class TokenLogin(BaseModel):
    token: str


@router.get("/me")
async def me(
    request: Request,
    authorization: str = Header(default=""),
    settings: Settings = Depends(get_settings),
) -> dict:
    principal = resolve_principal(request, authorization, settings)
    mode = "oidc" if settings.oidc_enabled else "token"
    if principal is None:
        raise HTTPException(status_code=401, detail={"mode": mode})
    return {"mode": mode, "email": principal.email, "role": principal.role}


@router.post("/logout")
async def logout(request: Request) -> dict:
    request.session.pop("user", None)
    return {"ok": True}


@router.post("/token-login")
async def token_login(
    body: TokenLogin,
    request: Request,
    settings: Settings = Depends(get_settings),
) -> dict:
    if settings.oidc_enabled:
        raise HTTPException(status_code=400, detail="OIDC is configured; use /api/auth/login")
    role: str | None = None
    if settings.console_write_token and secrets.compare_digest(
        body.token, settings.console_write_token
    ):
        role = "write"
    elif secrets.compare_digest(body.token, settings.console_token):
        role = "read"
    if role is None:
        raise HTTPException(status_code=401, detail="invalid token")
    request.session["user"] = Principal(subject="token", role=role).model_dump()
    return {"role": role}


@router.get("/login")
async def login(request: Request, settings: Settings = Depends(get_settings)):
    if not settings.oidc_enabled:
        raise HTTPException(status_code=400, detail="OIDC is not configured")
    return await oauth(settings).oidc.authorize_redirect(request, settings.oidc_redirect_uri)


@router.get("/callback")
async def callback(request: Request, settings: Settings = Depends(get_settings)):
    if not settings.oidc_enabled:
        raise HTTPException(status_code=400, detail="OIDC is not configured")
    token = await oauth(settings).oidc.authorize_access_token(request)
    claims = token.get("userinfo") or {}
    role = role_for_claims(claims, settings.oidc_role_claim, settings.oidc_write_value_set())
    request.session["user"] = Principal(
        subject=str(claims.get("sub", "unknown")),
        email=claims.get("email"),
        role=role,
    ).model_dump()
    return RedirectResponse(settings.post_login_redirect, status_code=302)
