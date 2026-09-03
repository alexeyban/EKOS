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
