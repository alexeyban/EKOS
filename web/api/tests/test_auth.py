"""Auth — token mode + the OIDC claim→role mapping (RFC 0131 §1)."""

from __future__ import annotations

from collections.abc import Iterator

import pytest
from fastapi.testclient import TestClient

from app.auth import role_for_claims
from app.main import create_app


@pytest.fixture
def client(monkeypatch: pytest.MonkeyPatch, tmp_path, reset_settings: None) -> Iterator[TestClient]:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "r-tok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w-tok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_BIN", "/bin/true")
    with TestClient(create_app()) as c:
        yield c


def test_me_is_401_without_credentials(client: TestClient) -> None:
    r = client.get("/api/auth/me")
    assert r.status_code == 401
    assert r.json()["detail"]["mode"] == "token"


def test_bearer_tokens_map_to_roles(client: TestClient) -> None:
    assert (
        client.get("/api/auth/me", headers={"Authorization": "Bearer r-tok"}).json()["role"]
        == "read"
    )
    assert (
        client.get("/api/auth/me", headers={"Authorization": "Bearer w-tok"}).json()["role"]
        == "write"
    )
    assert client.get("/api/auth/me", headers={"Authorization": "Bearer nope"}).status_code == 401


def test_token_login_sets_a_session_cookie(client: TestClient) -> None:
    r = client.post("/api/auth/token-login", json={"token": "w-tok"})
    assert r.status_code == 200 and r.json()["role"] == "write"
    # the cookie now carries the session — no header needed
    assert client.get("/api/auth/me").json()["role"] == "write"
    client.post("/api/auth/logout")
    assert client.get("/api/auth/me").status_code == 401


def test_read_principal_cannot_hit_a_write_route(client: TestClient) -> None:
    # /api/runs/<x>/cancel needs write; a read token gets 403 (not 401)
    r = client.post("/api/runs/does-not-exist/cancel", headers={"Authorization": "Bearer r-tok"})
    assert r.status_code == 403


def test_write_token_unset_means_no_write(
    monkeypatch: pytest.MonkeyPatch, tmp_path, reset_settings: None
) -> None:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "only-read")
    monkeypatch.delenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", raising=False)
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_BIN", "/bin/true")
    with TestClient(create_app()) as c:
        assert (
            c.get("/api/auth/me", headers={"Authorization": "Bearer only-read"}).json()["role"]
            == "read"
        )
        assert (
            c.post("/api/runs/x/cancel", headers={"Authorization": "Bearer only-read"}).status_code
            == 403
        )


def test_oidc_role_mapping() -> None:
    assert role_for_claims({}, "groups", set()) == "read"  # read-only deployment
    assert role_for_claims({"groups": ["ekos-write"]}, "groups", {"ekos-write"}) == "write"
    assert role_for_claims({"groups": ["other"]}, "groups", {"ekos-write"}) == "read"
    assert role_for_claims({"roles": "admin"}, "roles", {"admin"}) == "write"  # scalar claim


def _forged_session_cookie(secret: str, role: str) -> str:
    """A session cookie signed the way Starlette's `SessionMiddleware` signs one."""
    import base64
    import json

    import itsdangerous

    payload = base64.b64encode(json.dumps({"user": {"subject": "attacker", "role": role}}).encode())
    return itsdangerous.TimestampSigner(secret).sign(payload).decode()


def test_the_published_default_session_secret_cannot_forge_a_session(
    monkeypatch: pytest.MonkeyPatch, tmp_path, reset_settings: None
) -> None:
    """The session secret's default was a literal in this public repo, and the session cookie is
    trusted in every auth mode — so with no secret configured, anyone could sign
    `{"role": "write"}` and hold write access, even with no write token set. The default must not
    be usable as a key."""
    from app.settings import Settings

    published_default = Settings.model_fields["session_secret"].default
    monkeypatch.chdir(tmp_path)  # no stray .env
    monkeypatch.delenv("EKOS_CONSOLE_SESSION_SECRET", raising=False)
    monkeypatch.delenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", raising=False)
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_BIN", "/bin/true")
    for secret in {published_default, "dev-session-secret-change-me"}:
        with TestClient(create_app()) as c:
            c.cookies.set("session", _forged_session_cookie(secret, "write"))
            assert c.get("/api/auth/me").status_code == 401, secret


# ── OIDC sign-in failures answer with a reason, not a 500 ─────────────────────


@pytest.fixture
def oidc_client(
    monkeypatch: pytest.MonkeyPatch, tmp_path, reset_settings: None
) -> Iterator[TestClient]:
    from app import auth

    # Nothing listens on port 1: the provider is unreachable.
    monkeypatch.setenv("EKOS_CONSOLE_OIDC_ISSUER", "http://127.0.0.1:1/dex")
    monkeypatch.setenv("EKOS_CONSOLE_OIDC_CLIENT_ID", "ekos")
    monkeypatch.setenv("EKOS_CONSOLE_OIDC_CLIENT_SECRET", "x")
    monkeypatch.setenv(
        "EKOS_CONSOLE_OIDC_REDIRECT_URI", "https://ekos.example.com/api/auth/callback"
    )
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_BIN", "/bin/true")
    auth.reset_oauth()
    with TestClient(create_app(), base_url="https://ekos.example.com") as c:
        yield c
    auth.reset_oauth()


def test_login_with_an_unreachable_provider_is_502(oidc_client: TestClient) -> None:
    r = oidc_client.get("/api/auth/login", follow_redirects=False)
    assert r.status_code == 502
    assert "identity provider" in r.json()["detail"]


def test_a_cancelled_sign_in_is_400(oidc_client: TestClient) -> None:
    r = oidc_client.get("/api/auth/callback?error=access_denied", follow_redirects=False)
    assert r.status_code == 400
    assert "access_denied" in r.json()["detail"]


def test_a_callback_this_browser_did_not_start_is_400(oidc_client: TestClient) -> None:
    r = oidc_client.get("/api/auth/callback?code=x&state=y", follow_redirects=False)
    assert r.status_code == 400


def test_the_session_cookie_is_secure_when_the_console_is_served_over_https(
    oidc_client: TestClient,
) -> None:
    from app.main import cookie_is_https_only
    from app.settings import get_settings

    assert cookie_is_https_only(get_settings()) is True


def test_the_session_cookie_is_not_secure_on_plain_http(client: TestClient) -> None:
    from app.main import cookie_is_https_only
    from app.settings import get_settings

    # The default redirect URI is http://localhost: a Secure cookie would never be sent back.
    assert cookie_is_https_only(get_settings()) is False
    r = client.post("/api/auth/token-login", json={"token": "w-tok"})
    assert "secure" not in r.headers["set-cookie"].lower()


# ── OIDC scopes ──────────────────────────────────────────────────────────────


def _registered_scope() -> str:
    from app import auth
    from app.settings import get_settings

    auth.reset_oauth()
    try:
        return auth.oauth(get_settings()).oidc.client_kwargs["scope"]
    finally:
        auth.reset_oauth()


def test_sign_in_asks_for_identity_and_email_by_default(oidc_client: TestClient) -> None:
    assert _registered_scope() == "openid email profile"


def test_sign_in_asks_for_the_configured_scopes(
    monkeypatch: pytest.MonkeyPatch, tmp_path, reset_settings: None
) -> None:
    # Dex, Keycloak and Okta put group membership in the token only when `groups` is asked for.
    monkeypatch.setenv("EKOS_CONSOLE_OIDC_ISSUER", "http://127.0.0.1:1/dex")
    monkeypatch.setenv("EKOS_CONSOLE_OIDC_SCOPES", "openid email profile groups")
    assert _registered_scope() == "openid email profile groups"


def test_sign_in_always_asks_for_openid(
    monkeypatch: pytest.MonkeyPatch, tmp_path, reset_settings: None
) -> None:
    from pydantic import ValidationError

    from app.settings import Settings

    monkeypatch.setenv("EKOS_CONSOLE_OIDC_ISSUER", "http://127.0.0.1:1/dex")
    monkeypatch.setenv("EKOS_CONSOLE_OIDC_SCOPES", "email groups")
    with pytest.raises(ValidationError) as err:
        Settings()
    assert "openid" in str(err.value)
