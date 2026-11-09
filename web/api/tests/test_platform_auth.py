"""RFC 0251: sign-in done by Azure Container Apps in front of the console. The headers it sets name
the person; an API token still wins; without the setting the headers mean nothing; the signed-in
group gives everyone who signs in the role it is bound to, and never a token."""

from __future__ import annotations

import shutil
from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient
from pydantic import ValidationError

from app.main import create_app
from app.settings import Settings, get_settings

FALSE = shutil.which("false") or "/bin/false"


def headers(idp: str, name: str) -> dict[str, str]:
    return {"X-MS-CLIENT-PRINCIPAL-IDP": idp, "X-MS-CLIENT-PRINCIPAL-NAME": name}


ROOT = headers("github", "root")


@pytest.fixture
def env(monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None) -> Path:
    monkeypatch.setenv("EKOS_CONSOLE_PLATFORM_AUTH", "aca")
    monkeypatch.setenv("EKOS_CONSOLE_OWNERS", "github:root")
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "test-session-secret")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "console.db"))
    monkeypatch.setenv("EKOS_CONSOLE_RUNS_DIR", str(tmp_path / "runs"))
    monkeypatch.setenv("EKOS_CONSOLE_WORKSPACES_ROOT", str(tmp_path / "src"))
    monkeypatch.setenv("EKOS_CONSOLE_WORKSPACES_JSON", "[]")
    monkeypatch.setenv("EKOS_CONSOLE_PRODUCTION", "true")
    monkeypatch.setenv("EKOS_BIN", FALSE)
    monkeypatch.delenv("EKOS_CONSOLE_OIDC_ISSUER", raising=False)
    monkeypatch.delenv("EKOS_HOME", raising=False)
    (tmp_path / "src" / "demo").mkdir(parents=True)
    return tmp_path


@pytest.fixture
def client(env: Path) -> Iterator[TestClient]:
    with TestClient(create_app()) as c:
        yield c


def visible(client: TestClient, hdrs: dict) -> list[str]:
    r = client.get("/api/workspaces", headers=hdrs)
    return [w["id"] for w in r.json()] if r.status_code == 200 else []


def _demo_project(client: TestClient, env: Path) -> str:
    r = client.post(
        "/api/tenants",
        headers=ROOT,
        json={"id": "demo", "name": "Demo", "source_roots": [str(env / "src" / "demo")]},
    )
    assert r.status_code == 201, r.text
    ws = env / "src" / "demo" / "idempiere"
    (ws / ".ekos").mkdir(parents=True)
    (ws / "ekos.toml").write_text("[observe]\npaths = []\n")
    r = client.post(
        "/api/workspaces",
        headers=ROOT,
        json={"id": "idempiere", "name": "iDempiere", "path": str(ws), "tenant": "demo"},
    )
    assert r.status_code == 201, r.text
    return r.json()["id"]


def test_the_platform_headers_name_the_person(client) -> None:
    me = client.get("/api/auth/me", headers=headers("GitHub", "Alice"))
    assert me.status_code == 200, me.text
    body = me.json()
    assert body["mode"] == "oidc" and body["kind"] == "user" and body["name"] == "github:alice"
    assert body["owner"] is False
    assert client.get("/api/auth/me", headers=ROOT).json()["owner"] is True
    google = client.get("/api/auth/me", headers=headers("google", "bob@example.com")).json()
    assert google["email"] == "bob@example.com"


def test_no_headers_is_nobody_and_static_tokens_are_refused(client) -> None:
    assert client.get("/api/auth/me").status_code == 401
    assert (
        client.get("/api/auth/me", headers={"Authorization": "Bearer dev-token"}).status_code == 401
    )
    assert client.post("/api/auth/token-login", json={"token": "x"}).status_code == 400
    # Only one header, or a malformed one, is nobody too.
    assert (
        client.get("/api/auth/me", headers={"X-MS-CLIENT-PRINCIPAL-NAME": "alice"}).status_code
        == 401
    )
    assert client.get("/api/auth/me", headers=headers("github", "a b")).status_code == 401


def test_the_sign_in_page_offers_the_providers_and_logout_ends_it_there(client) -> None:
    page = client.get("/api/auth/login")
    assert page.status_code == 200
    assert "/.auth/login/github?post_login_redirect_uri=/" in page.text
    assert "/.auth/login/google" in page.text and "Sign in with GitHub" in page.text
    assert client.post("/api/auth/logout").json()["redirect"] == "/.auth/logout"


def test_the_signed_in_group_gives_everyone_its_role_and_never_a_token(
    client, env, monkeypatch
) -> None:
    ws_id = _demo_project(client, env)
    stranger = headers("google", "someone@example.com")
    assert ws_id not in visible(client, stranger)

    group = client.post("/api/tenants/demo/groups", headers=ROOT, json={"name": "everyone"})
    assert group.status_code == 201, group.text
    gid = group.json()["id"]
    bound = client.post(
        "/api/tenants/demo/bindings",
        headers=ROOT,
        json={"role": "viewer", "group_id": gid, "workspace_id": ws_id},
    )
    assert bound.status_code == 201, bound.text
    monkeypatch.setattr(get_settings(), "signed_in_group", gid)

    assert ws_id in visible(client, stranger)
    # A viewer: no admin of the tenant.
    assert client.get("/api/tenants/demo/groups", headers=stranger).status_code in (403, 404)

    token = client.post(
        "/api/tenants/demo/tokens",
        headers=ROOT,
        json={"name": "superset", "role": "viewer", "workspace_id": ws_id},
    ).json()["token"]
    assert ws_id in visible(client, {"Authorization": f"Bearer {token}"})
    # The token's own binding gave that; the group never reaches a token.
    from app import authz
    from app.auth import resolve_principal

    class R:
        def __init__(self) -> None:
            self.headers: dict = {}
            self.scope: dict = {}

    principal = resolve_principal(R(), f"Bearer {token}", get_settings())  # type: ignore[arg-type]
    assert principal is not None and principal.kind == "token"
    assert all(b.principal_kind == "token" for b in authz._bindings(principal))


def test_an_api_token_wins_over_the_headers(client, env) -> None:
    ws_id = _demo_project(client, env)
    token = client.post(
        "/api/tenants/demo/tokens",
        headers=ROOT,
        json={"name": "agent", "role": "viewer", "workspace_id": ws_id},
    ).json()["token"]
    me = client.get("/api/auth/me", headers={**ROOT, "Authorization": f"Bearer {token}"}).json()
    assert me["kind"] == "token" and me["owner"] is False


def test_the_headers_mean_nothing_without_the_setting(
    monkeypatch, tmp_path, reset_settings
) -> None:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_RUNS_DIR", str(tmp_path / "runs"))
    monkeypatch.setenv("EKOS_CONSOLE_WORKSPACES_JSON", "[]")
    monkeypatch.setenv("EKOS_BIN", FALSE)
    monkeypatch.delenv("EKOS_CONSOLE_PLATFORM_AUTH", raising=False)
    monkeypatch.delenv("EKOS_CONSOLE_OIDC_ISSUER", raising=False)
    monkeypatch.delenv("EKOS_HOME", raising=False)
    with TestClient(create_app()) as c:
        assert c.get("/api/auth/me", headers=ROOT).status_code == 401


def test_platform_auth_cannot_be_combined_with_oidc_or_local_mode(
    monkeypatch, reset_settings
) -> None:
    monkeypatch.setenv("EKOS_CONSOLE_OIDC_ISSUER", "https://idp.test")
    with pytest.raises(ValidationError, match="platform"):
        Settings(platform_auth="aca")
    monkeypatch.delenv("EKOS_CONSOLE_OIDC_ISSUER")
    with pytest.raises(ValidationError, match="platform_auth"):
        Settings(platform_auth="aca", mode="local")


def _read_only(monkeypatch: pytest.MonkeyPatch) -> None:
    import app.settings as settings_mod

    monkeypatch.setenv("EKOS_CONSOLE_READ_ONLY", "true")
    settings_mod._settings = None


def test_a_read_only_console_lets_everyone_read_and_nobody_act(client, env, monkeypatch) -> None:
    # RFC 0251 amendment 1: owners included; the pages that act on `read` are refused too.
    project = _demo_project(client, env)
    _read_only(monkeypatch)
    me = client.get("/api/auth/me", headers=ROOT).json()
    assert me["read_only"] is True and me["role"] == "read" and me["owner"] is True
    assert visible(client, ROOT) == [project], "reading still works"
    refused = {
        "new tenant": client.post("/api/tenants", headers=ROOT, json={"id": "x", "name": "X"}),
        "platform settings": client.get("/api/platform/settings", headers=ROOT),
        "commands": client.get(f"/api/workspaces/{project}/cli", headers=ROOT),
        "chat": client.get(f"/api/workspaces/{project}/chat", headers=ROOT),
        "ekos.toml": client.get(f"/api/workspaces/{project}/config", headers=ROOT),
    }
    assert {k: r.status_code for k, r in refused.items()} == {k: 403 for k in refused}, {
        k: r.text[:120] for k, r in refused.items()
    }


def test_without_the_switch_nothing_changes(client) -> None:
    me = client.get("/api/auth/me", headers=ROOT).json()
    assert me["read_only"] is False and me["role"] == "write"
