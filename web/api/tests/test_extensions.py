"""RFC 0198: extension pages in the console — a panel runs only a read command of the catalogue,
with its own flags, in the workspace; a page cannot make the console run anything else."""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app import catalogue, extensions, models, readproc
from app.main import create_app

OWNER = {"Authorization": "Bearer w-tok"}
READER = {"Authorization": "Bearer r-tok"}


def _flag(name: str) -> dict:
    return {
        "id": name,
        "long": name,
        "index": None,
        "kind": "flag",
        "multiple": False,
        "required": False,
        "possible": [],
        "default": [],
        "value_name": None,
        "help": "",
    }


CATALOGUE = {
    "version": "test",
    "commands": [
        {
            "path": ["lake", "status"],
            "about": "",
            "class": "read",
            "metered": False,
            "args": [_flag("json")],
        },
        {
            "path": ["lake", "sync"],
            "about": "",
            "class": "external",
            "metered": False,
            "args": [_flag("json")],
        },
    ],
}


def _page(command: list[str]) -> dict:
    return {
        "schema_version": 1,
        "extensions": [
            {
                "name": "lake",
                "pages": [
                    {
                        "id": "lake",
                        "title": "Lake",
                        "description": "",
                        "panels": [
                            {
                                "id": "health",
                                "title": "Health",
                                "command": command,
                                "view": {"kind": "table", "rows": "/rows", "columns": []},
                                "empty": "none",
                            }
                        ],
                        "actions": [{"label": "Sync", "command": ["lake", "sync"]}],
                    }
                ],
            }
        ],
    }


@pytest.fixture
def setup(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None
) -> Iterator[tuple[TestClient, str, list]]:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "r-tok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w-tok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setattr(catalogue, "_cached", CATALOGUE)
    monkeypatch.setattr(extensions, "_cached", _page(["lake", "status", "--json"]))
    ran: list = []

    async def fake_read(ekos_bin, path, argv, **_):
        ran.append(argv)
        return {"rows": [{"table": "public.orders", "bronze": "healthy"}]}

    monkeypatch.setattr(readproc, "read_checked_json", fake_read)
    ws = tmp_path / "ws"
    ws.mkdir()
    (ws / "ekos.toml").write_text("")
    with TestClient(create_app()) as c:
        models.add_workspace(models.Workspace(id="w", name="w", path=str(ws)))
        yield c, "w", ran


def test_the_pages_are_listed_with_their_extension(setup) -> None:
    c, _, _ = setup
    r = c.get("/api/extensions", headers=READER)
    assert r.status_code == 200
    page = r.json()["pages"][0]
    assert (page["extension"], page["id"], page["actions"][0]["command"]) == (
        "lake",
        "lake",
        ["lake", "sync"],
    )


def test_a_reader_gets_a_panels_data_from_its_read_command(setup) -> None:
    c, wid, ran = setup
    r = c.get(f"/api/workspaces/{wid}/extensions/lake/lake/health", headers=READER)
    assert r.status_code == 200, r.text
    assert r.json()["rows"][0]["bronze"] == "healthy"
    assert ran == [["lake", "status", "--json"]]


@pytest.mark.parametrize(
    "command",
    [
        ["lake", "sync", "--json"],  # not a read command
        ["lake", "status", "--output=/x"],  # not one of its flags
        ["lake", "status", "/etc/passwd"],  # not a flag at all
        ["nothing"],  # not in the catalogue
    ],
)
def test_a_panel_that_is_not_a_plain_read_is_refused_and_nothing_runs(
    setup, monkeypatch: pytest.MonkeyPatch, command: list[str]
) -> None:
    c, wid, ran = setup
    monkeypatch.setattr(extensions, "_cached", _page(command))
    r = c.get(f"/api/workspaces/{wid}/extensions/lake/lake/health", headers=OWNER)
    assert r.status_code == 404
    assert ran == []


def test_an_unknown_panel_is_404(setup) -> None:
    c, wid, ran = setup
    r = c.get(f"/api/workspaces/{wid}/extensions/lake/lake/nope", headers=READER)
    assert r.status_code == 404
    assert ran == []


def test_a_binary_without_the_command_has_no_pages(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    import asyncio

    monkeypatch.setattr(extensions, "_cached", None)
    fake = tmp_path / "old-ekos"

    class Proc:
        returncode = 2

        async def communicate(self):
            return b"", b"error: unrecognized subcommand 'extensions'"

    async def spawn(argv, **_):
        assert argv[1:] == ["extensions", "--json"]
        return Proc()

    monkeypatch.setattr(extensions._proc, "spawn", spawn)
    spec = asyncio.run(extensions.load(str(fake)))
    assert extensions.pages(spec) == []
