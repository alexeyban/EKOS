"""Integrity view route (RFC 0233).

A fake `ekos` records its argv: the console must run exactly the one fixed read, never recompute
or pass anything a caller typed.
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
from app.routes.integrity import ARGV

R = {"Authorization": "Bearer r"}
W = {"Authorization": "Bearer w"}

FAKE = r"""#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ["FAKE_LOG"], "a") as f:
    f.write(json.dumps(args) + "\n")
if args[:1] == ["mcp"]:
    sys.exit(0)
if args[:2] == ["check", "refs"]:
    if os.environ.get("FAKE_NO_INDEX"):
        sys.stderr.write("Error: no integrity index — run `ekos commit`\n")
        sys.exit(1)
    print(json.dumps({
        "version": 2, "from_index": True, "computed_at": "2026-10-30T16:00:00Z",
        "metrics": {"resolution_rate": 0.98, "cycles": 1},
        "diagnostics": [
            {"kind": "UnresolvedReference", "severity": "error", "confidence": 0.9,
             "source": {"file": "sql/b.sql", "line": 328, "object_name": "budget__reject"},
             "target_spec": {"written": "budget_to_project"}, "reason": "no compiled relation",
             "fingerprint": "ab"},
            {"kind": "OrphanArtifact", "severity": "info", "confidence": 0.5,
             "source": {"object_name": "old_log"}, "target_spec": {"written": "old_log"},
             "reason": "unused", "fingerprint": "cd"}],
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


def test_only_the_fixed_read_is_allowed() -> None:
    _check_allowed(ARGV)
    for argv in (
        ["check", "refs", "--format", "json"],  # would recompute over the whole ledger
        [*ARGV, "--baseline", "/etc/passwd"],
        ["check", "refs", "--from-index", "--format", "sarif", "--reachability"],
    ):
        with pytest.raises(ReadProcError):
            _check_allowed(argv)


@_not_on_windows("the fake ekos is a #! script")
def test_a_reader_gets_the_index_report_as_is(setup: tuple[TestClient, Path]) -> None:
    c, log = setup
    r = c.get("/api/workspaces/w/integrity", headers=R)
    assert r.status_code == 200, r.text
    assert [d["fingerprint"] for d in r.json()["diagnostics"]] == ["ab", "cd"]
    assert _calls(log) == [ARGV]


@_not_on_windows("the fake ekos is a #! script")
def test_the_summary_counts_by_severity_and_drops_findings(
    setup: tuple[TestClient, Path],
) -> None:
    c, _ = setup
    r = c.get("/api/workspaces/w/integrity?summary=true", headers=R)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["by_severity"] == {"error": 1, "info": 1}
    assert "diagnostics" not in body
    assert body["metrics"]["cycles"] == 1


@_not_on_windows("the fake ekos is a #! script")
def test_no_index_yet_is_a_bad_gateway_with_the_cli_message(
    setup: tuple[TestClient, Path], monkeypatch: pytest.MonkeyPatch
) -> None:
    c, _ = setup
    monkeypatch.setenv("FAKE_NO_INDEX", "1")
    r = c.get("/api/workspaces/w/integrity", headers=R)
    assert r.status_code == 502
    assert "ekos commit" in r.json()["detail"]


def test_an_unknown_workspace_is_404(setup: tuple[TestClient, Path]) -> None:
    c, _ = setup
    assert c.get("/api/workspaces/nope/integrity", headers=R).status_code == 404
