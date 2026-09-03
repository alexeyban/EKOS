"""FastAPI surface: health is public, everything else needs the console token."""

from __future__ import annotations

import json

import pytest
from fastapi.testclient import TestClient

from app.main import create_app
from app.settings import get_settings


@pytest.fixture
def client(monkeypatch: pytest.MonkeyPatch) -> TestClient:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "secret-console")
    monkeypatch.setenv(
        "EKOS_CONSOLE_WORKSPACES_JSON",
        json.dumps([{"id": "self", "name": "EKOS", "path": "/repo", "mcp_port": 7331}]),
    )
    get_settings.cache_clear() if hasattr(get_settings, "cache_clear") else None
    import app.settings as settings_mod

    settings_mod._settings = None
    with TestClient(create_app()) as c:
        yield c


def test_health_is_public(client: TestClient) -> None:
    r = client.get("/api/health")
    assert r.status_code == 200
    assert r.json()["status"] == "ok"


def test_workspaces_requires_the_console_token(client: TestClient) -> None:
    assert client.get("/api/workspaces").status_code == 401
    assert (
        client.get("/api/workspaces", headers={"Authorization": "Bearer wrong"}).status_code == 401
    )

    r = client.get("/api/workspaces", headers={"Authorization": "Bearer secret-console"})
    assert r.status_code == 200
    assert [w["id"] for w in r.json()] == ["self"]


def test_unknown_workspace_is_404(client: TestClient) -> None:
    r = client.get("/api/workspaces/nope/stats", headers={"Authorization": "Bearer secret-console"})
    assert r.status_code == 404
