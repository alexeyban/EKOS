"""Auth endpoints (RFC 0131 §1, RFC 0180). OIDC sign-in, and a token sign-in for development;
both land a signed session cookie that names the principal and nothing about its rights."""

from __future__ import annotations

import uuid

import httpx
from authlib.integrations.base_client import OAuthError
from fastapi import APIRouter, Depends, Header, HTTPException, Request
from fastapi.responses import RedirectResponse
from pydantic import BaseModel

from .. import authz, local_signin, models
from ..auth import claim_values, oauth, resolve_principal, static_role
from ..settings import Settings, get_settings
from ._responses import BAD_REQUEST, UNAUTHORIZED

BAD_GATEWAY = {"description": "The identity provider could not be reached"}


def _unreachable(exc: Exception) -> HTTPException:
    """The provider did not answer. Said as such: a bare 500 reads as a console fault, and the
    cause is nearly always the issuer URL or the network between the two."""
    return HTTPException(
        status_code=502,
        detail=f"the identity provider could not be reached ({type(exc).__name__}); "
        "check EKOS_CONSOLE_OIDC_ISSUER and that the console can connect to it",
    )


router = APIRouter(prefix="/auth", tags=["auth"])


class TokenLogin(BaseModel):
    token: str


class LocalCode(BaseModel):
    code: str


def _mode(settings: Settings) -> str:
    if settings.local:
        return "local"
    # RFC 0251: platform sign-in looks like OIDC to the UI — a "Sign in" button to /api/auth/login.
    return "oidc" if settings.signs_in else "token"


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
    writes = not settings.read_only and (
        principal.owner
        or authz.can_anywhere(principal, "operate")
        or authz.can_anywhere(principal, "review")
        or any("admin" in t["roles"] for t in tenants)
    )
    return {
        "mode": _mode(settings),
        "kind": principal.kind,
        "email": principal.email,
        "name": principal.subject if principal.kind == "user" else None,
        "role": "write" if writes else "read",
        "owner": principal.owner,
        # RFC 0251 amendment 1: the UI hides every control that acts (the API refuses them).
        "read_only": settings.read_only,
        "tenants": tenants,
    }


@router.post("/logout")
async def logout(request: Request, settings: Settings = Depends(get_settings)) -> dict:
    request.session.pop("user", None)
    # RFC 0251: the platform holds the sign-in; the UI follows `redirect` to end it there.
    return {"ok": True, **({"redirect": "/.auth/logout"} if settings.aca_auth else {})}


@router.post("/token-login", responses={400: BAD_REQUEST, 401: UNAUTHORIZED})
async def token_login(
    body: TokenLogin,
    request: Request,
    settings: Settings = Depends(get_settings),
) -> dict:
    if settings.signs_in:
        raise HTTPException(status_code=400, detail="sign-in is configured; use /api/auth/login")
    if settings.production:
        raise HTTPException(status_code=400, detail="static tokens are off in production mode")
    if settings.local:
        raise HTTPException(status_code=400, detail="local mode: sign in with `ekos console open`")
    role = static_role(body.token, settings)
    if role is None:
        authz.audit(None, "sign-in", "denied", detail="static token")
        raise HTTPException(status_code=401, detail="invalid token")
    request.session["user"] = {"kind": "static", "id": role}
    return {"role": role}


def _redeem(request: Request, settings: Settings, code: str) -> None:
    """RFC 0184 §3: a one-time code from the home's control directory starts a local session."""
    if not settings.local:
        raise HTTPException(status_code=400, detail="the console is not in local mode")
    if not local_signin.redeem(settings.control_dir, code):
        authz.audit(None, "sign-in", "denied", detail="local code")
        raise HTTPException(
            status_code=401,
            detail="this sign-in code is wrong, used or expired; run `ekos console open` again",
        )
    request.session["user"] = {"kind": "local"}
    authz.audit(None, "sign-in", "ok", detail="local")


@router.get("/local", responses={400: BAD_REQUEST, 401: UNAUTHORIZED})
async def local_link(code: str, request: Request, settings: Settings = Depends(get_settings)):
    _redeem(request, settings, code)
    return RedirectResponse(settings.post_login_redirect, status_code=303)


@router.post("/local", responses={400: BAD_REQUEST, 401: UNAUTHORIZED})
async def local_code(
    body: LocalCode, request: Request, settings: Settings = Depends(get_settings)
) -> dict:
    _redeem(request, settings, body.code)
    return {"ok": True}


_PROVIDER_LABELS = {"github": "GitHub", "google": "Google", "aad": "Microsoft", "x": "X"}


def platform_login_page(settings: Settings) -> str:
    """RFC 0251: one link per provider the platform's authentication offers."""
    import html

    links = "".join(
        f'<p><a class="b" href="/.auth/login/{html.escape(p)}?post_login_redirect_uri=/">'
        f"Sign in with {html.escape(_PROVIDER_LABELS.get(p, p))}</a></p>"
        for p in settings.platform_provider_list()
    )
    return (
        "<!doctype html><html><head><meta charset='utf-8'>"
        "<meta name='viewport' content='width=device-width,initial-scale=1'>"
        "<title>Sign in · EKOS</title><style>"
        "body{font-family:system-ui,sans-serif;max-width:28rem;margin:4rem auto;padding:0 1rem}"
        ".b{display:block;padding:.7rem 1rem;border:1px solid #888;border-radius:6px;"
        "text-decoration:none;color:inherit}</style></head><body><h1>Sign in to EKOS</h1>"
        "<p>Sign in to read this demo. EKOS keeps only the name your provider gives.</p>"
        f"{links}</body></html>"
    )


@router.get("/login", responses={400: BAD_REQUEST, 502: BAD_GATEWAY})
async def login(request: Request, settings: Settings = Depends(get_settings)):
    if settings.aca_auth:
        from fastapi.responses import HTMLResponse

        return HTMLResponse(platform_login_page(settings))
    if not settings.oidc_enabled:
        raise HTTPException(status_code=400, detail="OIDC is not configured")
    try:
        return await oauth(settings).oidc.authorize_redirect(request, settings.oidc_redirect_uri)
    except httpx.HTTPError as exc:
        raise _unreachable(exc) from exc


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


@router.get("/callback", responses={400: BAD_REQUEST, 502: BAD_GATEWAY})
async def callback(request: Request, settings: Settings = Depends(get_settings)):
    if not settings.oidc_enabled:
        raise HTTPException(status_code=400, detail="OIDC is not configured")
    try:
        token = await oauth(settings).oidc.authorize_access_token(request)
    except OAuthError as exc:
        # The person cancelled, the provider refused, or this browser did not start the
        # sign-in it is finishing (an old tab, a replayed link).
        authz.audit(None, "sign-in", "denied", detail=str(exc.error or "")[:100])
        raise HTTPException(
            status_code=400, detail=f"sign-in did not complete: {exc.error}; start again"
        ) from exc
    except httpx.HTTPError as exc:
        raise _unreachable(exc) from exc
    claims = token.get("userinfo") or {}
    user = user_for_claims(claims)
    request.session["user"] = {
        "kind": "user",
        "id": user.id,
        "claims": claim_values(claims, settings.oidc_role_claim),
    }
    authz.audit(None, "sign-in", "ok", detail=user.email or user.subject)
    return RedirectResponse(settings.post_login_redirect, status_code=302)
