"""Authentication + the read/write role split (RFC 0131 §1).

Two modes, chosen by whether `OIDC_ISSUER` is set:

* **OIDC** — Authorization Code + PKCE against an external provider (`authlib`). A configurable
  ID-token claim maps to the `write` role; everyone else authenticated is `read`.
* **token** — two static bearer tokens: `CONSOLE_TOKEN` → `read`, `CONSOLE_WRITE_TOKEN` → `write`.
  The fallback for CI / compose / local dev where there is no IdP.

In **both** modes the browser ends up with a signed session cookie (so `EventSource`, which can't
set headers, works for the SSE log stream). `Authorization: Bearer` is additionally accepted in
token mode for curl / tests.
"""

from __future__ import annotations

import secrets
from typing import Literal

from fastapi import Depends, Header, HTTPException, Request
from pydantic import BaseModel

from .settings import Settings, get_settings

Role = Literal["read", "write"]


class Principal(BaseModel):
    subject: str
    email: str | None = None
    role: Role


# ── OIDC client (lazy) ───────────────────────────────────────────────────────

_oauth = None


def oauth(settings: Settings):
    """The registered authlib OAuth client. Only touched in OIDC mode."""
    global _oauth
    if _oauth is None:
        from authlib.integrations.starlette_client import OAuth

        o = OAuth()
        o.register(
            name="oidc",
            client_id=settings.oidc_client_id,
            client_secret=settings.oidc_client_secret,
            server_metadata_url=(
                settings.oidc_issuer.rstrip("/") + "/.well-known/openid-configuration"
            ),
            client_kwargs={"scope": "openid email profile"},
        )
        _oauth = o
    return _oauth


def reset_oauth() -> None:
    """Test hook — drop the memoised client so a new issuer is picked up."""
    global _oauth
    _oauth = None


# ── role mapping ─────────────────────────────────────────────────────────────


def role_for_claims(claims: dict, role_claim: str, write_values: set[str]) -> Role:
    """`write` iff the ID token's `role_claim` shares a value with `write_values`. An empty
    `write_values` means the deployment is read-only for everyone."""
    if not write_values:
        return "read"
    raw = claims.get(role_claim) or []
    values = {raw} if isinstance(raw, str) else set(raw)
    return "write" if write_values & values else "read"


# ── request → principal ──────────────────────────────────────────────────────


def _from_session(request: Request) -> Principal | None:
    user = request.session.get("user") if "session" in request.scope else None
    return Principal(**user) if user else None


def _from_bearer(authorization: str, settings: Settings) -> Principal | None:
    if settings.oidc_enabled:
        return None  # bearer tokens are not a thing in OIDC mode
    prefix = "Bearer "
    supplied = authorization[len(prefix) :] if authorization.startswith(prefix) else ""
    if not supplied:
        return None
    if settings.console_write_token and secrets.compare_digest(
        supplied, settings.console_write_token
    ):
        return Principal(subject="token", role="write")
    if secrets.compare_digest(supplied, settings.console_token):
        return Principal(subject="token", role="read")
    return None


def resolve_principal(request: Request, authorization: str, settings: Settings) -> Principal | None:
    return _from_session(request) or _from_bearer(authorization, settings)


def check_role(
    request: Request, authorization: str, settings: Settings, minimum: Role
) -> Principal:
    """Resolve the principal and enforce `minimum`. 401 = not authenticated, 403 = wrong role.
    Framework-independent so both the dependency and manual call sites can use it."""
    principal = resolve_principal(request, authorization, settings)
    if principal is None:
        raise HTTPException(status_code=401, detail="not authenticated")
    if minimum == "write" and principal.role != "write":
        raise HTTPException(status_code=403, detail="write role required")
    return principal


def require_role(minimum: Role):
    """Dependency factory. `require_role("read")` accepts read or write; `require_role("write")`
    needs write."""

    async def dependency(
        request: Request,
        authorization: str = Header(default=""),
        settings: Settings = Depends(get_settings),
    ) -> Principal:
        return check_role(request, authorization, settings, minimum)

    return dependency
