"""RFC 0184 §1-2: console settings in `console.toml`, editable by an owner; secrets in the
keychain."""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app import envs, keychain, models
from app.main import create_app
from app.settings import Settings

OWNER = {"Authorization": "Bearer w-tok"}
READER = {"Authorization": "Bearer r-tok"}
MARKER = "SECRETMARKER-7f3a9c"


@pytest.fixture
def home(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, reset_settings: None) -> Path:
    root = tmp_path / "home"
    (root / "control").mkdir(parents=True)
    monkeypatch.setenv("EKOS_HOME", str(root))
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "r-tok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w-tok")
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_BIN", "/bin/true")
    return root


@pytest.fixture
def client(home: Path) -> Iterator[TestClient]:
    with TestClient(create_app()) as c:
        yield c


def _file(home: Path) -> Path:
    return home / "control" / "console.toml"


def _field(client: TestClient, name: str) -> dict:
    fields = client.get("/api/platform/settings", headers=OWNER).json()["fields"]
    return next(f for f in fields if f["name"] == name)


# ── precedence ───────────────────────────────────────────────────────────────


def test_the_file_is_read(home: Path) -> None:
    _file(home).write_text("mcp_idle_seconds = 120\n")
    assert Settings().mcp_idle_seconds == 120


def test_the_environment_wins_over_the_file(home: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    _file(home).write_text("mcp_idle_seconds = 120\n")
    monkeypatch.setenv("EKOS_CONSOLE_MCP_IDLE_SECONDS", "60")
    assert Settings().mcp_idle_seconds == 60


def test_a_field_with_its_own_variable_name_is_read_from_the_file(home: Path) -> None:
    # `oidc_issuer` is read from EKOS_CONSOLE_OIDC_ISSUER through an alias.
    _file(home).write_text('oidc_issuer = "https://id.example.com"\n')
    assert Settings().oidc_issuer == "https://id.example.com"


def test_a_secret_in_the_keychain_wins_over_the_file(
    home: Path, memory_keyring: dict, monkeypatch: pytest.MonkeyPatch
) -> None:
    _file(home).write_text('session_secret = "from-file"\n')
    memory_keyring[("ekos", "console:session_secret")] = "from-keychain"
    monkeypatch.delenv("EKOS_CONSOLE_SESSION_SECRET")
    monkeypatch.chdir(home)  # away from a developer's web/api/.env
    assert Settings().session_secret == "from-keychain"


def test_no_home_means_no_file(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, reset_settings: None
) -> None:
    monkeypatch.chdir(tmp_path)
    (tmp_path / "console.toml").write_text("mcp_idle_seconds = 5\n")
    assert Settings().mcp_idle_seconds == 900


def test_a_dotenv_file_counts_as_the_environment(
    home: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from app import console_file

    monkeypatch.chdir(home)
    (home / ".env").write_text("EKOS_CONSOLE_MCP_MAX_SERVERS=2\n")
    assert Settings().mcp_max_servers == 2
    assert console_file.set_by_environment(Settings, "mcp_max_servers")
    assert not console_file.set_by_environment(Settings, "mcp_idle_seconds")


# ── reading and saving through the console ───────────────────────────────────


def test_only_an_owner_reads_or_changes_settings(client: TestClient) -> None:
    assert client.get("/api/platform/settings", headers=READER).status_code == 403
    r = client.put("/api/platform/settings", headers=READER, json={"values": {"port": 9000}})
    assert r.status_code == 403
    assert client.get("/api/platform/settings").status_code == 401


def test_every_field_is_described(client: TestClient) -> None:
    body = client.get("/api/platform/settings", headers=OWNER).json()
    names = {f["name"] for f in body["fields"]}
    assert names == set(Settings.model_fields)
    issuer = _field(client, "oidc_issuer")
    assert issuer["source"] == "default" and issuer["editable"] is True
    assert issuer["group"] and issuer["help"]
    assert body["file"].endswith("console.toml")


def test_a_variable_from_the_environment_is_read_only(client: TestClient) -> None:
    f = _field(client, "console_token")
    assert f["source"] == "environment" and f["editable"] is False
    r = client.put(
        "/api/platform/settings", headers=OWNER, json={"values": {"ekos_bin": "/usr/bin/x"}}
    )
    assert r.status_code == 400
    r = client.put(
        "/api/platform/settings", headers=OWNER, json={"secrets": {"console_token": "x"}}
    )
    assert r.status_code == 400


def test_a_save_writes_the_file_and_keeps_comments(client: TestClient, home: Path) -> None:
    _file(home).write_text("# written by hand\nmcp_max_servers = 3  # small machine\n")
    r = client.put(
        "/api/platform/settings",
        headers=OWNER,
        json={"values": {"mcp_idle_seconds": 300, "mcp_max_servers": 4}},
    )
    assert r.status_code == 200, r.text
    assert r.json()["restart_required"] is True
    text = _file(home).read_text()
    assert "# written by hand" in text and "# small machine" in text
    assert "mcp_idle_seconds = 300" in text and "mcp_max_servers = 4" in text
    assert _field(client, "mcp_idle_seconds")["source"] == "file"


def test_null_removes_a_key(client: TestClient, home: Path) -> None:
    _file(home).write_text("mcp_idle_seconds = 300\n")
    r = client.put(
        "/api/platform/settings", headers=OWNER, json={"values": {"mcp_idle_seconds": None}}
    )
    assert r.status_code == 200
    assert "mcp_idle_seconds" not in _file(home).read_text()


def test_an_invalid_result_writes_nothing(client: TestClient, home: Path) -> None:
    _file(home).write_text("mcp_idle_seconds = 300\n")
    before = _file(home).read_text()
    # Production mode without OIDC or owners is refused at start; so it is refused here.
    r = client.put("/api/platform/settings", headers=OWNER, json={"values": {"production": True}})
    assert r.status_code == 400 and "OIDC" in r.text
    r = client.put(
        "/api/platform/settings", headers=OWNER, json={"values": {"mcp_idle_seconds": "soon"}}
    )
    assert r.status_code == 400
    r = client.put("/api/platform/settings", headers=OWNER, json={"values": {"nope": 1}})
    assert r.status_code == 400
    assert _file(home).read_text() == before


def test_a_secret_goes_to_the_keychain_and_is_never_shown(
    client: TestClient, home: Path, memory_keyring: dict
) -> None:
    r = client.put(
        "/api/platform/settings",
        headers=OWNER,
        json={"secrets": {"oidc_client_secret": MARKER}},
    )
    assert r.status_code == 200, r.text
    assert memory_keyring[("ekos", "console:oidc_client_secret")] == MARKER
    f = _field(client, "oidc_client_secret")
    assert f["secret"] and f["set"] and f["source"] == "keychain" and f["value"] is None
    seen = client.get("/api/platform/settings", headers=OWNER).text
    assert MARKER not in seen
    assert not _file(home).exists() or MARKER not in _file(home).read_text()
    assert all(MARKER not in (row.detail + row.action) for row in models.list_audit(None))
    db = Path(home / "control" / "console.db").read_bytes()
    assert MARKER.encode() not in db


def test_a_secret_is_never_written_as_a_value(client: TestClient) -> None:
    r = client.put(
        "/api/platform/settings", headers=OWNER, json={"values": {"oidc_client_secret": "x"}}
    )
    assert r.status_code == 400


def test_removing_a_secret(client: TestClient, memory_keyring: dict) -> None:
    memory_keyring[("ekos", "console:oidc_client_secret")] = "x"
    r = client.put(
        "/api/platform/settings", headers=OWNER, json={"secrets": {"oidc_client_secret": None}}
    )
    assert r.status_code == 200
    assert ("ekos", "console:oidc_client_secret") not in memory_keyring


def test_a_secret_needs_a_keychain(client: TestClient, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("EKOS_CONSOLE_KEYCHAIN", "off")
    r = client.put(
        "/api/platform/settings", headers=OWNER, json={"secrets": {"oidc_client_secret": "x"}}
    )
    assert r.status_code == 409 and "environment" in r.text


# ── restart ──────────────────────────────────────────────────────────────────


def test_restart_needs_a_supervisor(client: TestClient) -> None:
    r = client.post("/api/platform/restart", headers=OWNER)
    assert r.status_code == 409


def test_restart_when_supervised(home: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    from app.routes import platform

    monkeypatch.setenv("EKOS_CONSOLE_SUPERVISED", "1")
    asked: list[int] = []
    monkeypatch.setattr(platform, "_exit_for_restart", lambda: asked.append(1))
    with TestClient(create_app()) as c:
        assert c.post("/api/platform/restart", headers=READER).status_code == 403
        r = c.post("/api/platform/restart", headers=OWNER)
        assert r.status_code == 202
    assert asked == [1]


# ── keychain values for projects ─────────────────────────────────────────────


def test_storing_a_value_for_a_variable(client: TestClient, memory_keyring: dict) -> None:
    r = client.put(
        "/api/platform/keychain/ANTHROPIC_API_KEY", headers=OWNER, json={"value": MARKER}
    )
    assert r.status_code == 200, r.text
    assert memory_keyring[("ekos", "env:ANTHROPIC_API_KEY")] == MARKER
    body = client.get("/api/platform/keychain", headers=OWNER).json()
    assert body["available"] is True and body["names"] == ["ANTHROPIC_API_KEY"]
    assert MARKER not in client.get("/api/platform/keychain", headers=OWNER).text
    assert client.get("/api/platform/keychain", headers=READER).status_code == 403
    r = client.delete("/api/platform/keychain/ANTHROPIC_API_KEY", headers=OWNER)
    assert r.status_code == 200
    assert client.get("/api/platform/keychain", headers=OWNER).json()["names"] == []


@pytest.mark.parametrize("name", ["EKOS_CONSOLE_SESSION_SECRET", "1BAD", "A-B"])
def test_a_console_variable_or_a_bad_name_is_refused(client: TestClient, name: str) -> None:
    r = client.put(f"/api/platform/keychain/{name}", headers=OWNER, json={"value": "x"})
    assert r.status_code == 400


def test_storing_needs_a_keychain(client: TestClient, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("EKOS_CONSOLE_KEYCHAIN", "off")
    assert client.get("/api/platform/keychain", headers=OWNER).json()["available"] is False
    r = client.put("/api/platform/keychain/OPENAI_API_KEY", headers=OWNER, json={"value": "x"})
    assert r.status_code == 409


def _tenant(tid: str, model_env: list[str], metered: bool) -> models.Tenant:
    t = models.Tenant(id=tid, name=tid, model_env=model_env, allow_metered=metered)
    models.add(t)
    return t


def test_a_granted_variable_is_filled_from_the_keychain(client: TestClient) -> None:
    keychain.put_env("ACME_KEY", MARKER)
    acme = _tenant("acme", ["ACME_KEY"], True)
    other = _tenant("other", [], True)
    off = _tenant("off", ["ACME_KEY"], False)
    environ = {"PATH": "/bin"}
    assert envs.for_tenant(acme, environ=environ)["ACME_KEY"] == MARKER
    assert "ACME_KEY" not in envs.for_tenant(other, environ=environ)
    assert "ACME_KEY" not in envs.for_tenant(off, environ=environ)
    # A value in the environment is the operator's choice and wins.
    assert envs.for_tenant(acme, environ={"ACME_KEY": "env"})["ACME_KEY"] == "env"


def test_nothing_is_filled_with_the_keychain_off(
    client: TestClient, monkeypatch: pytest.MonkeyPatch, memory_keyring: dict
) -> None:
    memory_keyring[("ekos", "env:ACME_KEY")] = MARKER
    acme = _tenant("acme", ["ACME_KEY"], True)
    monkeypatch.setenv("EKOS_CONSOLE_KEYCHAIN", "off")
    assert "ACME_KEY" not in envs.for_tenant(acme, environ={})
