"""Conflicts view routes (RFC 0174).

A fake `ekos` script records its argv, so these tests pin down exactly what the console runs,
with which role, and who it says decided.
"""

from __future__ import annotations

import json
import stat
import sys
from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app.conflicts_write import resolve_argv
from app.main import create_app
from app.readproc import ReadProcError, _check_allowed
from app.semantics_write import ReviewError


# Windows cannot run this test's assumption (see the reason); the job on Windows is informational.
def _not_on_windows(reason: str) -> pytest.MarkDecorator:
    return pytest.mark.skipif(sys.platform == "win32", reason=reason)


R = {"Authorization": "Bearer r"}
W = {"Authorization": "Bearer w"}
ID = "cc57a3dc-bd4c-507e-94fe-54f967a3594f"

FAKE = r"""#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ["FAKE_LOG"], "a") as f:
    f.write(json.dumps(args) + "\n")
if args[:1] == ["mcp"]:
    sys.exit(0)
if args[:2] == ["conflicts", "list"]:
    print(json.dumps([{"id": "__ID__", "name": "user_preference.columns", "status": "open",
                       "conflict_type": "duplicate_definition", "attribute": "columns",
                       "claims": [{"value": ["id"], "path": "a.sql", "line": 1, "source": "sql"},
                                  {"value": ["user_id"], "path": "b.sql", "line": 2,
                                   "source": "sql"}]}]))
elif args[:2] == ["conflicts", "resolve"]:
    if os.environ.get("FAKE_LOCKED"):
        sys.stderr.write("Error: cannot write: another writable process already holds the lock\n")
        sys.exit(1)
    if "--pick=9" in args:
        sys.stderr.write("Error: --pick must be 1..=2\n")
        sys.exit(1)
    print("user_preference.columns — resolved")
""".replace("__ID__", ID)


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


# ── pure ─────────────────────────────────────────────────────────────────────────────────────


def test_only_the_list_is_a_read() -> None:
    _check_allowed(["conflicts", "list", "--json", "--status", "open", "--type", "merge_loss"])
    for argv in (
        ["conflicts", "resolve", ID, "--pick=1"],
        ["conflicts", "list", "--json", "--out", "/etc/passwd"],
        ["conflicts", "show", ID],
    ):
        with pytest.raises(ReadProcError):
            _check_allowed(argv)


def test_resolve_argv_is_one_decision_named_by_id() -> None:
    assert resolve_argv(ID, "ann@x.io", pick=2, note="--yes") == [
        "conflicts",
        "resolve",
        ID,
        "--as=ann@x.io",
        "--pick=2",
        "--note=--yes",
    ]
    assert resolve_argv(ID, "ann", both_valid=True, note="two facets") == [
        "conflicts",
        "resolve",
        ID,
        "--as=ann",
        "--both-valid",
        "--note=two facets",
    ]
    for kwargs in (
        {},  # no decision
        {"pick": 1, "both_valid": True, "note": "n"},  # two decisions
        {"both_valid": True},  # both valid without a note
        {"pick": 0},
        {"pick": True},
    ):
        with pytest.raises(ReviewError):
            resolve_argv(ID, "ann", **kwargs)
    with pytest.raises(ReviewError):
        resolve_argv("user_preference.columns", "ann", pick=1)  # a name, not an id


# ── routes ───────────────────────────────────────────────────────────────────────────────────


@_not_on_windows("the fake ekos is a script run by its #! line")
def test_the_list_needs_only_the_read_role(setup) -> None:
    c, log = setup
    r = c.get("/api/workspaces/w/conflicts?status=open&type=duplicate_definition", headers=R)
    assert r.status_code == 200 and r.json()[0]["id"] == ID
    assert _calls(log) == [
        ["conflicts", "list", "--json", "--status", "open", "--type", "duplicate_definition"]
    ]
    assert c.get("/api/workspaces/w/conflicts?status=bogus", headers=R).status_code == 422
    assert c.get("/api/workspaces/w/conflicts").status_code == 401


@_not_on_windows("the fake ekos is a script run by its #! line")
def test_a_decision_needs_the_write_role_and_a_reviewer(setup) -> None:
    c, log = setup
    url = f"/api/workspaces/w/conflicts/{ID}/resolve"
    assert c.post(url, headers=R, json={"pick": 1, "reviewer": "Ann"}).status_code == 403
    assert c.post(url, headers=W, json={"pick": 1}).status_code == 400  # token mode: no name
    assert _calls(log) == []

    r = c.post(url, headers=W, json={"pick": 1, "reviewer": "Ann", "note": "the migration"})
    assert r.status_code == 200 and r.json()["reviewer"] == "token:Ann"
    assert _calls(log) == [
        ["conflicts", "resolve", ID, "--as=token:Ann", "--pick=1", "--note=the migration"]
    ]


@_not_on_windows("the fake ekos is a script run by its #! line")
def test_bad_decisions_and_cli_refusals_are_400_and_a_held_lock_is_409(
    setup, monkeypatch: pytest.MonkeyPatch
) -> None:
    c, log = setup
    url = f"/api/workspaces/w/conflicts/{ID}/resolve"
    both = {"both_valid": True, "reviewer": "Ann"}
    assert c.post(url, headers=W, json=both).status_code == 400  # no note
    assert c.post(url, headers=W, json={"reviewer": "Ann"}).status_code == 400  # no decision
    bad_id = "/api/workspaces/w/conflicts/not-an-id/resolve"
    assert c.post(bad_id, headers=W, json={"pick": 1, "reviewer": "Ann"}).status_code == 400
    assert _calls(log) == []

    r = c.post(url, headers=W, json={"pick": 9, "reviewer": "Ann"})
    assert r.status_code == 400 and "--pick must be" in r.json()["detail"]

    monkeypatch.setenv("FAKE_LOCKED", "1")
    assert c.post(url, headers=W, json={"pick": 1, "reviewer": "Ann"}).status_code == 409
