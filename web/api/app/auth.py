"""Who is asking (RFC 0131 §1, RFC 0180).

A request carries one of three credentials:

* a **session cookie** set by the OIDC sign-in — a person, a row in `User`;
* an **API token**, `Authorization: Bearer ekos_…` — an agent, a row in `ApiToken`;
* a **static console token** (cookie or bearer) — development only, refused in production mode.

This module turns a request into a `Principal` and nothing else. What that principal may do is
decided in `authz`, and only there.
"""

from __future__ import annotations

import hashlib
import secrets
from datetime import UTC, datetime, timedelta
from typing import Literal

from fastapi import Request
from pydantic import BaseModel

from . import models
from .settings import Settings

TOKEN_PREFIX = "ekos_"
# `last_used_at` is refreshed at most this often, so reading with a token is not one database
# write per request.
_LAST_USED_GRANULARITY = timedelta(minutes=1)


class Principal(BaseModel):
    kind: Literal["user", "token", "static"]
    # `User.id`, `ApiToken.id`, or "read" / "write" for a static token.
    id: str
    subject: str = ""
    email: str | None = None
    # A platform owner (RFC 0180): named in EKOS_CONSOLE_OWNERS, or the static write token.
    owner: bool = False
    # Values of the OIDC role claim at sign-in; a group may name one of them.
    claims: list[str] = []
    # RFC 0131's mapping, kept outside production mode: what this principal may do in the
    # `default` tenant without any binding.
    compat: Literal["read", "write"] | None = None
    # The tenant an API token belongs to. It has no roles anywhere else.
    tenant_id: str | None = None

    @property
    def label(self) -> str:
        """How the audit log names this principal."""
        if self.kind == "token":
            # The name an admin gave it, and enough of the id to tell two same-named ones apart.
            return f"token:{self.subject}#{self.id[:6]}"
        if self.kind == "static":
            return f"static:{self.id}"
        return self.email or self.subject or self.id


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
            client_kwargs={"scope": settings.oidc_scopes},
        )
        _oauth = o
    return _oauth


def reset_oauth() -> None:
    """Test hook — drop the memoised client so a new issuer is picked up."""
    global _oauth
    _oauth = None


# ── claims ───────────────────────────────────────────────────────────────────


def claim_values(claims: dict, role_claim: str) -> list[str]:
    """The role claim as a sorted list of strings, whatever shape the provider sent."""
    raw = claims.get(role_claim) or []
    values = [raw] if isinstance(raw, str) else list(raw)
    return sorted({str(v) for v in values})


def role_for_claims(
    claims: dict, role_claim: str, write_values: set[str]
) -> Literal["read", "write"]:
    """`write` iff the ID token's `role_claim` shares a value with `write_values`. An empty
    `write_values` means nobody gets write this way."""
    if not write_values:
        return "read"
    return "write" if write_values & set(claim_values(claims, role_claim)) else "read"


# ── API tokens ───────────────────────────────────────────────────────────────


def new_token() -> tuple[str, str, str]:
    """A fresh token: `(token, sha256 hex, display prefix)`. The token itself is returned to its
    creator once and never stored."""
    token = TOKEN_PREFIX + secrets.token_urlsafe(32)
    return token, hash_token(token), token[: len(TOKEN_PREFIX) + 6]


def hash_token(token: str) -> str:
    # A token is 256 random bits, so a plain digest is enough: there is nothing to brute-force
    # that a slow hash would protect.
    return hashlib.sha256(token.encode()).hexdigest()


def _from_api_token(supplied: str) -> Principal | None:
    row = models.token_by_hash(hash_token(supplied))
    if row is None or row.revoked_at is not None:
        return None
    now = datetime.now(UTC)
    if row.expires_at is not None and _aware(row.expires_at) <= now:
        return None
    if row.last_used_at is None or now - _aware(row.last_used_at) > _LAST_USED_GRANULARITY:
        models.update_token(row.id, last_used_at=now)
    return Principal(kind="token", id=row.id, subject=row.name, tenant_id=row.tenant_id)


def _aware(moment: datetime) -> datetime:
    """SQLite returns naive datetimes; they were written as UTC."""
    return moment if moment.tzinfo is not None else moment.replace(tzinfo=UTC)


# ── request → principal ──────────────────────────────────────────────────────


def _static(role: str) -> Principal:
    return Principal(
        kind="static",
        id=role,
        subject="token",
        owner=role == "write",
        compat="write" if role == "write" else "read",
    )


def static_role(supplied: str, settings: Settings) -> str | None:
    """Which static console token `supplied` is, if static tokens are accepted at all."""
    if settings.oidc_enabled or settings.production or not supplied:
        return None
    if settings.console_write_token and secrets.compare_digest(
        supplied, settings.console_write_token
    ):
        return "write"
    if secrets.compare_digest(supplied, settings.console_token):
        return "read"
    return None


def user_principal(user: models.User, claims: list[str], settings: Settings) -> Principal:
    owners = settings.owner_set()
    owner = bool(owners) and (
        (user.email or "").lower() in owners or (user.subject or "").lower() in owners
    )
    compat = None
    if not settings.production:
        compat = "write" if settings.oidc_write_value_set() & set(claims) else "read"
    return Principal(
        kind="user",
        id=user.id,
        subject=user.subject,
        email=user.email or None,
        owner=owner,
        claims=claims,
        compat=compat,
    )


def _from_session(request: Request, settings: Settings) -> Principal | None:
    data = request.session.get("user") if "session" in request.scope else None
    if not isinstance(data, dict):
        return None
    kind = data.get("kind")
    if kind == "static":
        if settings.oidc_enabled or settings.production or data.get("id") not in ("read", "write"):
            return None
        return _static(data["id"])
    if kind == "user" and settings.oidc_enabled:
        user = models.get_user(str(data.get("id", "")))
        if user is None:
            return None
        return user_principal(user, [str(c) for c in data.get("claims", [])], settings)
    return None


def resolve_principal(request: Request, authorization: str, settings: Settings) -> Principal | None:
    prefix = "Bearer "
    supplied = authorization[len(prefix) :] if authorization.startswith(prefix) else ""
    # An explicit API token is a deliberate choice of identity; it is not overridden by a cookie.
    if supplied.startswith(TOKEN_PREFIX):
        return _from_api_token(supplied)
    from_session = _from_session(request, settings)
    if from_session is not None:
        return from_session
    role = static_role(supplied, settings)
    return _static(role) if role is not None else None
