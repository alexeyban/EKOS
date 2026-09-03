"""Config endpoints end to end against a real `ekos` binary (RFC 0130 §3).

Skipped unless $EKOS_BIN points at a built `ekos` binary.
"""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app.main import create_app

AUTH = {"Authorization": "Bearer cfg-console"}


@pytest.fixture
def cfg_client(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None, ekos_bin: str
) -> Iterator[tuple[TestClient, Path]]:
    import subprocess

    ws = tmp_path / "ws"
    ws.mkdir()
    subprocess.run([ekos_bin, "init"], cwd=ws, check=True, capture_output=True)

    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "cfg-console")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "cfg-console")
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "test-secret")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "console.db"))
    monkeypatch.setenv("EKOS_BIN", ekos_bin)
    monkeypatch.setenv("EKOS_CONSOLE_MCP_PORT_BASE", "19100")
    with TestClient(create_app()) as c:
        r = c.post(
            "/api/workspaces",
            headers=AUTH,
            json={"id": "w", "name": "W", "path": str(ws)},
        )
        assert r.status_code == 201, r.text
        yield c, ws


def test_get_returns_raw_and_observe(cfg_client) -> None:
    client, _ws = cfg_client
    r = client.get("/api/workspaces/w/config", headers=AUTH)
    assert r.status_code == 200, r.text
    body = r.json()
    assert "[observe]" in body["raw"]
    assert "paths" in body["observe"]


def test_validate_flags_a_glob_ignore_pattern(cfg_client) -> None:
    client, _ws = cfg_client
    raw = '[observe]\npaths = []\nignore-patterns = ["target", "src/fixtures"]\n'
    r = client.post("/api/workspaces/w/config/validate", headers=AUTH, json={"raw": raw})
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["ok"] is True
    assert any(w["code"] == "ignore-pattern-looks-like-a-path" for w in body["warnings"])


def test_validate_rejects_unknown_section(cfg_client) -> None:
    client, _ws = cfg_client
    r = client.post(
        "/api/workspaces/w/config/validate", headers=AUTH, json={"raw": "[nope]\nx = 1\n"}
    )
    assert r.status_code == 200
    assert r.json()["ok"] is False


def test_preview_scan_counts_files(cfg_client) -> None:
    client, ws = cfg_client
    (ws / "a.py").write_text("x = 1")
    (ws / "b.py").write_text("y = 2")
    r = client.post("/api/workspaces/w/config/preview-scan", headers=AUTH)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["total_files"] >= 2
    assert any(e["ext"] == "py" for e in body["by_extension"])


def test_put_narrowing_warns_and_leaves_a_bak(cfg_client) -> None:
    client, ws = cfg_client
    (ws / "src").mkdir()
    client.put(
        "/api/workspaces/w/config",
        headers=AUTH,
        json={"raw": '[observe]\npaths = ["src", "docs"]\nignore-patterns = ["target"]\n'},
    )
    (ws / "docs").mkdir()
    narrowed = client.put(
        "/api/workspaces/w/config",
        headers=AUTH,
        json={"raw": '[observe]\npaths = ["src"]\nignore-patterns = ["target"]\n'},
    )
    assert narrowed.status_code == 200, narrowed.text
    body = narrowed.json()
    assert body["observe_delta"]["removed_paths"] == ["docs"]
    assert body["append_only_warning"] is not None
    assert (ws / "ekos.toml.bak").is_file()


def test_put_rejects_broken_toml_without_touching_the_file(cfg_client) -> None:
    client, ws = cfg_client
    before = (ws / "ekos.toml").read_text()
    r = client.put("/api/workspaces/w/config", headers=AUTH, json={"raw": "[observe\n"})
    assert r.status_code == 422
    assert (ws / "ekos.toml").read_text() == before
