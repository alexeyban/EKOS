"""Command allowlist + submission gating (RFC 0131 §2)."""

from __future__ import annotations

from collections.abc import Iterator

import pytest
from fastapi.testclient import TestClient

from app.commands import BY_NAME, Command, Param
from app.main import create_app

R = {"Authorization": "Bearer r"}
W = {"Authorization": "Bearer w"}


@pytest.fixture
def client(monkeypatch: pytest.MonkeyPatch, tmp_path, reset_settings: None) -> Iterator[TestClient]:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "r")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_BIN", "/bin/true")
    ws = tmp_path / "ws"
    ws.mkdir()
    (ws / "ekos.toml").write_text("[observe]\n")
    (ws / ".ekos").mkdir()
    with TestClient(create_app()) as c:
        c.post("/api/workspaces", headers=W, json={"id": "w", "name": "W", "path": str(ws)})
        yield c


def test_render_argv_validates_params() -> None:
    c = Command("x", ("x",), params={"q": Param("string", required=True)})
    assert c.render_argv({"q": "FIND T"}) == ["x", "--q", "FIND T"]
    with pytest.raises(ValueError):
        c.render_argv({})
    with pytest.raises(ValueError):
        c.render_argv({"q": "has\x00null"})
    b = Command("y", ("y",), params={"force": Param("bool")})
    assert b.render_argv({"force": True}) == ["y", "--force"]
    assert b.render_argv({"force": False}) == ["y"]


def test_catalogue_lists_write_flags(client: TestClient) -> None:
    cat = client.get("/api/commands", headers=R).json()
    by = {c["name"]: c for c in cat}
    assert by["build"]["is_write"] is True
    assert by["status"]["is_write"] is False
    assert by["pipeline"]["stages"] == ["build", "recover", "resolve", "compile", "commit"]


def test_unknown_command_is_404(client: TestClient) -> None:
    assert client.post("/api/workspaces/w/commands/nope", headers=W).status_code == 404


def test_write_command_needs_write_role(client: TestClient) -> None:
    assert client.post("/api/workspaces/w/commands/build", headers=R).status_code == 403
    r = client.post("/api/workspaces/w/commands/build", headers=W)
    assert r.status_code == 200 and "run_id" in r.json()


def test_read_command_needs_only_read(client: TestClient) -> None:
    r = client.post("/api/workspaces/w/commands/status", headers=R)
    assert r.status_code == 200


def test_missing_required_param_is_422(client: TestClient) -> None:
    assert BY_NAME["ekl"].params["query"].required
    assert client.post("/api/workspaces/w/commands/ekl", headers=R, json={}).status_code == 422
