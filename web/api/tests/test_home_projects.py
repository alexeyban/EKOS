"""RFC 0199: a project created in the EKOS home while the console runs appears in the project list
without a restart (the CLI or an installer made it; the console only finds its directory)."""

from __future__ import annotations

from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app.main import create_app

OWNER = {"Authorization": "Bearer w-tok"}


def _project(home: Path, name: str) -> None:
    d = home / "data" / "default" / name
    d.mkdir(parents=True)
    (d / "ekos.toml").write_text("")


def test_a_project_added_after_start_up_is_listed(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None
) -> None:
    home = tmp_path / "home"
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "r-tok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w-tok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_HOME", str(home))
    _project(home, "first")
    with TestClient(create_app()) as c:
        names = {w["name"] for w in c.get("/api/workspaces", headers=OWNER).json()}
        assert names == {"first"}
        _project(home, "sales")  # e.g. `ekos-lakehouse lakehouse install --source …`
        names = {w["name"] for w in c.get("/api/workspaces", headers=OWNER).json()}
        assert names == {"first", "sales"}
        # Listing again adds nothing twice.
        assert len(c.get("/api/workspaces", headers=OWNER).json()) == 2
