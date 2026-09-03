"""FastAPI surface: health is public, everything else needs the console token; the workspace
registry validates paths and persists to SQLite."""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app.main import create_app

AUTH = {"Authorization": "Bearer secret-console"}  # read role
WAUTH = {"Authorization": "Bearer secret-write"}  # write role


@pytest.fixture
def client(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None
) -> Iterator[TestClient]:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "secret-console")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "secret-write")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "console.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "test-secret")
    # A binary that exits immediately: the supervisor can "spawn" it but the server never readies,
    # which is all these registry-level tests need.
    monkeypatch.setenv("EKOS_BIN", "/bin/false")
    monkeypatch.setenv("EKOS_CONSOLE_WORKSPACES_JSON", "[]")
    with TestClient(create_app()) as c:
        yield c


def test_health_is_public(client: TestClient) -> None:
    r = client.get("/api/health")
    assert r.status_code == 200
    assert r.json()["status"] == "ok"


def test_workspaces_requires_the_console_token(client: TestClient) -> None:
    assert client.get("/api/workspaces").status_code == 401
    assert client.get("/api/workspaces", headers={"Authorization": "Bearer no"}).status_code == 401
    r = client.get("/api/workspaces", headers=AUTH)
    assert r.status_code == 200
    assert r.json() == []


def test_register_validates_the_path(client: TestClient, tmp_path: Path) -> None:
    # a read principal cannot register
    assert (
        client.post(
            "/api/workspaces", headers=AUTH, json={"id": "x", "name": "X", "path": str(tmp_path)}
        ).status_code
        == 403
    )

    missing = client.post(
        "/api/workspaces",
        headers=WAUTH,
        json={"id": "x", "name": "X", "path": str(tmp_path / "nope")},
    )
    assert missing.status_code == 400

    (tmp_path / "ekos.toml").write_text("[observe]\n")
    no_ekos_dir = client.post(
        "/api/workspaces", headers=WAUTH, json={"id": "x", "name": "X", "path": str(tmp_path)}
    )
    assert no_ekos_dir.status_code == 400


def test_register_list_and_delete_roundtrip(client: TestClient, fake_workspace: Path) -> None:
    created = client.post(
        "/api/workspaces",
        headers=WAUTH,
        json={"id": "w1", "name": "One", "path": str(fake_workspace)},
    )
    assert created.status_code == 201
    body = created.json()
    assert body["id"] == "w1"
    assert body["server"] is not None  # a handle exists even if the fake server never readies

    listed = client.get("/api/workspaces", headers=AUTH).json()
    assert [w["id"] for w in listed] == ["w1"]

    dup = client.post(
        "/api/workspaces",
        headers=WAUTH,
        json={"id": "w1", "name": "One", "path": str(fake_workspace)},
    )
    assert dup.status_code == 409

    assert client.delete("/api/workspaces/w1", headers=WAUTH).status_code == 204
    assert client.get("/api/workspaces", headers=AUTH).json() == []


def test_unknown_workspace_is_404(client: TestClient) -> None:
    r = client.get("/api/workspaces/nope/stats", headers=AUTH)
    assert r.status_code == 404
