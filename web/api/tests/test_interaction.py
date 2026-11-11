"""RFC 0256 in the console: the interaction map and its findings, read through the CLI.

A fake `ekos` records its argv, so these tests pin down exactly what the console runs.
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

FAKE = r"""#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ["FAKE_LOG"], "a") as f:
    f.write(json.dumps(args) + "\n")
if args[:1] == ["mcp"]:
    sys.exit(0)
if args[:2] == ["interaction", "matrix"]:
    print(json.dumps({"tables": [{"table": "orders", "cells": [
        {"actor": "db_role:etl", "kind": "db_role", "name": "etl", "declared": [],
         "observed": {"update": {"calls": 3, "rows": 3, "window_end": "2026-11-11T00:00:00Z"}},
         "via": [], "writes": True}]}], "observed_facts": 1, "observed_note": "dated window"}))
elif args[:2] == ["interaction", "findings"]:
    print(json.dumps([{"id": "f1", "finding": "unexplained_writer", "table": "orders",
                       "actor": "db_role:etl", "operations": ["update"], "detail": {"calls": 3}}]))
"""


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


def test_the_reads_are_on_the_allowlist_and_nothing_else() -> None:
    for argv in (
        ["interaction", "matrix", "--json"],
        ["interaction", "matrix", "--json", "--table", "orders", "--actor", "db_role:etl"],
        ["interaction", "findings", "--json"],
    ):
        _check_allowed(argv)
    for argv in (
        ["interaction", "observe"],
        ["interaction", "observe", "--json"],
        ["interaction", "matrix", "--json", "--out", "x"],
        ["interaction", "findings", "--json", "--table", "x"],
        ["review", "interaction", "etl", "--confirm"],
    ):
        with pytest.raises(ReadProcError):
            _check_allowed(argv)


@pytest.mark.skipif(sys.platform == "win32", reason="the fake ekos is a #! script")
def test_the_matrix_and_findings_are_read_with_their_filters(
    setup: tuple[TestClient, Path],
) -> None:
    c, log = setup
    r = c.get("/api/workspaces/w/interaction", headers=R, params={"table": "orders"})
    assert r.status_code == 200, r.text
    assert r.json()["tables"][0]["table"] == "orders"
    r = c.get("/api/workspaces/w/interaction/findings", headers=R)
    assert r.status_code == 200, r.text
    assert r.json()[0]["finding"] == "unexplained_writer"
    assert _calls(log) == [
        ["interaction", "matrix", "--json", "--table", "orders"],
        ["interaction", "findings", "--json"],
    ]


@pytest.mark.skipif(sys.platform == "win32", reason="the fake ekos is a #! script")
def test_a_filter_that_looks_like_a_flag_stays_a_value(setup: tuple[TestClient, Path]) -> None:
    c, log = setup
    r = c.get("/api/workspaces/w/interaction", headers=R, params={"actor": "--out=/etc/x"})
    assert r.status_code in (200, 400), r.text
    for call in _calls(log):
        assert "--out=/etc/x" not in call[3::2], call
