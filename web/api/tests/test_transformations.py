"""Transformation chain routes (RFC 0257).

A fake `ekos` records its argv: the console runs only `ekos transformation list|graph --json`
with checked arguments, and never anything a caller typed beyond an id, statement numbers and a
name filter.
"""

from __future__ import annotations

import json
import stat
import sys
from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app.main import create_app
from app.readproc import ReadProcError, _check_allowed

R = {"Authorization": "Bearer r"}
W = {"Authorization": "Bearer w"}
NODE = "404b288d-d5d0-5a20-85c1-f7b8541c1d30"

FAKE = r"""#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ["FAKE_LOG"], "a") as f:
    f.write(json.dumps(args) + "\n")
if args[:1] == ["mcp"]:
    sys.exit(0)
if args[:2] == ["transformation", "list"]:
    print(json.dumps({"total": 1, "hidden": 0, "transformations": [
        {"key": "a.sql#0", "name": "v", "kind": "view", "object": "x", "steps": 3,
         "unmapped": 0}]}))
elif args[:2] == ["transformation", "graph"]:
    print(json.dumps({"format": "ekos.transformation-graph/1", "nodes": [], "edges": []}))
"""


def _not_on_windows(reason: str) -> pytest.MarkDecorator:
    return pytest.mark.skipif(sys.platform == "win32", reason=reason)


@pytest.fixture
def setup(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None
) -> Iterator[tuple[TestClient, Path]]:
    fake = tmp_path / "ekos"
    fake.write_text(FAKE)
    fake.chmod(fake.stat().st_mode | stat.S_IEXEC)
    log = tmp_path / "argv.log"
    log.write_text("")
    monkeypatch.setenv("FAKE_LOG", str(log))
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "r")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_BIN", str(fake))
    ws = tmp_path / "ws"
    ws.mkdir()
    (ws / "ekos.toml").write_text("[observe]\n")
    (ws / ".ekos").mkdir()
    with TestClient(create_app()) as c:
        c.post("/api/workspaces", headers=W, json={"id": "w", "name": "W", "path": str(ws)})
        yield c, log


def _calls(log: Path) -> list[list[str]]:
    calls = [json.loads(line) for line in log.read_text().splitlines() if line]
    return [c for c in calls if c[:1] != ["mcp"]]


def test_only_the_transformation_reads_are_allowed() -> None:
    _check_allowed(["transformation", "graph", "--json", "--object", NODE, "--expand", "1,2"])
    _check_allowed(["transformation", "list", "--json", "--all", "--filter", "x"])
    for argv in (
        ["transformation", "graph", "--object", NODE],  # not the --json shape
        ["transformation", "graph", "--json", "--object", NODE, "--workspace", "/"],
        ["transformation", "list", "--json", "--config", "/etc/passwd"],
    ):
        with pytest.raises(ReadProcError):
            _check_allowed(argv)


@_not_on_windows("the fake ekos is a #! script")
def test_a_reader_lists_and_opens_a_chain(setup: tuple[TestClient, Path]) -> None:
    c, log = setup
    r = c.get("/api/workspaces/w/transformations?filter=order&all=true", headers=R)
    assert r.status_code == 200, r.text
    assert r.json()["transformations"][0]["name"] == "v"
    r = c.get(f"/api/workspaces/w/transformations/graph?object={NODE}&expand=2,3", headers=R)
    assert r.status_code == 200, r.text
    assert r.json()["format"] == "ekos.transformation-graph/1"
    assert _calls(log) == [
        ["transformation", "list", "--json", "--all", "--filter", "order", "--limit", "500"],
        ["transformation", "graph", "--json", "--object", NODE, "--expand", "2,3"],
    ]


@_not_on_windows("the fake ekos is a #! script")
def test_arguments_are_checked_before_anything_runs(setup: tuple[TestClient, Path]) -> None:
    c, log = setup
    for url in (
        "/api/workspaces/w/transformations/graph?object=--config",
        "/api/workspaces/w/transformations/graph?object=../../x",
        f"/api/workspaces/w/transformations/graph?object={NODE}&expand=0",
        f"/api/workspaces/w/transformations/graph?object={NODE}&expand=--all",
        "/api/workspaces/w/transformations?filter=a;b",
    ):
        assert c.get(url, headers=R).status_code in (400, 422), url
    assert _calls(log) == []


def test_an_unknown_workspace_is_404(setup: tuple[TestClient, Path]) -> None:
    c, _ = setup
    assert c.get("/api/workspaces/nope/transformations", headers=R).status_code == 404
