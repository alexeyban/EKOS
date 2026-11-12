"""What the data says about one table (RFC 0252): the console serves `ekos profile show --json`.

A fake `ekos` records its argv: the console runs exactly the one read with the table it was given,
refuses anything that is not a table name, and turns "no profile run yet" into a plain answer.
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
if args[:3] == ["profile", "show", "--json"]:
    if os.environ.get("FAKE_NO_RUN"):
        sys.stderr.write("Error: no profile run yet (ekos profile run)\n")
        sys.exit(1)
    print(json.dumps({
        "measured_at": "2026-11-12T10:00:00Z", "dead_after_months": 24, "grain": [], "usage": [],
        "columns": [{"database": "erp", "owner": "public", "table": "orders", "column": "status",
                     "tier": "p1", "coverage": {"non_null": 50, "literals": {"1": {"rows": 50}},
                     "unexplained": {"rows": 0},
                     "monthly": {"min_cell": 5, "literals": {"1": [
                        {"month": "2026-09", "rows": 40}, {"month": "2026-10", "rows": None}]}}}}],
    }))
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


def test_only_a_table_name_reaches_the_cli() -> None:
    _check_allowed(["profile", "show", "--json", "public.orders"])
    for argv in (
        ["profile", "show", "--json", "--help"],
        ["profile", "show", "--json", "../etc/passwd"],
        ["profile", "show", "--json", "orders", "extra"],
        ["profile", "run"],
    ):
        with pytest.raises(ReadProcError):
            _check_allowed(argv)


@_not_on_windows("the fake ekos is a #! script")
def test_a_reader_gets_the_monthly_counts_as_measured(setup: tuple[TestClient, Path]) -> None:
    c, log = setup
    r = c.get("/api/workspaces/w/profile?table=orders", headers=R)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["measured"] is True
    series = body["columns"][0]["coverage"]["monthly"]["literals"]["1"]
    assert series[1] == {"month": "2026-10", "rows": None}
    assert _calls(log) == [["profile", "show", "--json", "orders"]]


@_not_on_windows("the fake ekos is a #! script")
def test_before_any_run_the_answer_says_so(
    setup: tuple[TestClient, Path], monkeypatch: pytest.MonkeyPatch
) -> None:
    c, _ = setup
    monkeypatch.setenv("FAKE_NO_RUN", "1")
    r = c.get("/api/workspaces/w/profile?table=orders", headers=R)
    assert r.status_code == 200, r.text
    assert r.json()["measured"] is False
    assert "ekos profile run" in r.json()["reason"]


def test_a_name_that_is_not_a_table_is_refused(setup: tuple[TestClient, Path]) -> None:
    c, log = setup
    r = c.get("/api/workspaces/w/profile?table=--json", headers=R)
    assert r.status_code == 400
    assert _calls(log) == []
