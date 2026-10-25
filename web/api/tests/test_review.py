"""Workflow bar and review routes (RFC 0213 Phase 3).

A fake `ekos` script records its argv, so these tests pin down exactly what the console runs,
with which right, and who it says decided.
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
from app.review_write import approve_argv, decide_argv, revoke_argv
from app.semantics_write import ReviewError


def _not_on_windows(reason: str) -> pytest.MarkDecorator:
    return pytest.mark.skipif(sys.platform == "win32", reason=reason)


R = {"Authorization": "Bearer r"}
W = {"Authorization": "Bearer w"}
D = "a" * 64

FAKE = r"""#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ["FAKE_LOG"], "a") as f:
    f.write(json.dumps(args) + "\n")
if args[:1] == ["mcp"]:
    sys.exit(0)
if args[:2] == ["workflow", "status"]:
    print(json.dumps({"steps": [{"n": 1, "id": "analyse", "title": "Analyse sources",
                                 "state": "done", "detail": "fresh"}]}))
elif args[:2] == ["review", "list"]:
    print(json.dumps({"subjects": [{"subject": "scope", "items": 2, "decided": 0,
                                    "outdated": 0, "blocking": 0, "digest": "d",
                                    "signoff": {"state": "none"}}]}))
elif args[:2] == ["review", "show"]:
    print(json.dumps({"subject": args[2], "items": 1, "rows": []}))
elif args[:1] in (["review"], ["signoff"]):
    if "--digest=" + "b" * 64 in args:
        sys.stderr.write("Error: the review changed since it was shown\n")
        sys.exit(1)
    print("recorded")
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


# ── pure ─────────────────────────────────────────────────────────────────────────────────────


def test_review_reads_are_fixed_shapes() -> None:
    _check_allowed(["workflow", "status", "--json"])
    _check_allowed(["review", "list", "--json"])
    _check_allowed(["review", "show", "pii", "--json", "--undecided", "--limit", "50"])
    for argv in (
        ["review", "show", "../x", "--json"],
        ["review", "show", "pii"],
        ["review", "show", "pii", "--json", "--limit", "-1"],
        ["review", "show", "pii", "--json", "--as", "x"],
        ["review", "scope", "--in", "--", "T"],
        ["signoff", "approve", "pii", f"--digest={D}"],
    ):
        with pytest.raises(ReadProcError):
            _check_allowed(argv)


def test_decision_argv_puts_items_after_the_end_of_options() -> None:
    assert decide_argv("scope", ["T_REPORT", "--yes"], "ann", verdict="in", reason="needed") == [
        "review",
        "scope",
        "--in",
        "--reason=needed",
        "--as=ann",
        "--",
        "T_REPORT",
        "--yes",
    ]
    assert decide_argv("pii", ["C_BPARTNER.EMAIL"], "ann", verdict="confirm", mask="hash") == [
        "review",
        "pii",
        "--confirm",
        "--mask=hash",
        "--as=ann",
        "--",
        "C_BPARTNER.EMAIL",
    ]
    assert decide_argv("identity", ["a = b"], "ann", verdict="reject", reason="different") == [
        "review",
        "identity",
        "--reject",
        "--note=different",
        "--as=ann",
        "--",
        "a = b",
    ]
    assert decide_argv(
        "write-behaviour",
        ["C_ORDER"],
        "ann",
        verdict="override",
        overrides={"update_class": "tracked"},
        reason="audited",
    ) == [
        "review",
        "write-behaviour",
        "--set=update_class=tracked",
        "--reason=audited",
        "--as=ann",
        "--",
        "C_ORDER",
    ]
    for subject, kwargs in (
        ("conflicts", {"verdict": "confirm"}),  # decided on its own page
        ("glossary", {"verdict": "confirm"}),
        ("scope", {"verdict": "maybe"}),
        ("pii", {"verdict": "reject", "mask": "hash"}),
        ("pii", {"verdict": "confirm", "mask": "plain"}),
        ("identity", {"verdict": "reject"}),  # no reason
        ("write-behaviour", {"verdict": "x", "overrides": {"owner": "x"}, "reason": "r"}),
        ("write-behaviour", {"verdict": "x", "overrides": {"change_log": "true"}}),  # no reason
    ):
        with pytest.raises(ReviewError):
            decide_argv(subject, ["T"], "ann", **kwargs)
    for targets in ([], ["a", "a"], ["a\nb"], [""], ["x" * 301]):
        with pytest.raises(ReviewError):
            decide_argv("scope", targets, "ann", verdict="in")


def test_signoff_argv_needs_the_shown_digest_and_revoke_a_reason() -> None:
    assert approve_argv("pii", D, "ann", note="ok") == [
        "signoff",
        "approve",
        "pii",
        f"--digest={D}",
        "--as=ann",
        "--note=ok",
    ]
    for digest in ("", "A" * 64, "a" * 63, "--yes"):
        with pytest.raises(ReviewError):
            approve_argv("pii", digest, "ann", note=None)
    with pytest.raises(ReviewError):
        approve_argv("everything", D, "ann", note=None)
    assert revoke_argv("pii", "redo", "ann") == [
        "signoff",
        "revoke",
        "pii",
        "--reason=redo",
        "--as=ann",
    ]
    with pytest.raises(ReviewError):
        revoke_argv("pii", " ", "ann")


# ── routes ───────────────────────────────────────────────────────────────────────────────────


@_not_on_windows("the fake ekos is a script run by its #! line")
def test_the_bar_and_the_review_need_only_the_read_right(setup) -> None:
    c, log = setup
    assert c.get("/api/workspaces/w/workflow", headers=R).json()["steps"][0]["state"] == "done"
    assert c.get("/api/workspaces/w/review", headers=R).json()["subjects"][0]["items"] == 2
    r = c.get("/api/workspaces/w/review/pii?undecided=true&limit=20", headers=R)
    assert r.status_code == 200 and r.json()["subject"] == "pii"
    assert c.get("/api/workspaces/w/review/everything", headers=R).status_code == 422
    assert c.get("/api/workspaces/w/workflow").status_code == 401
    assert _calls(log) == [
        ["workflow", "status", "--json"],
        ["review", "list", "--json"],
        ["review", "show", "pii", "--json", "--undecided", "--limit", "20"],
    ]


@_not_on_windows("the fake ekos is a script run by its #! line")
def test_decisions_and_signoff_need_the_review_right_and_a_reviewer(setup) -> None:
    c, log = setup
    decide = "/api/workspaces/w/review/scope/decide"
    body = {"targets": ["T_REPORT"], "verdict": "in", "reviewer": "Ann"}
    assert c.post(decide, headers=R, json=body).status_code == 403
    assert c.post(decide, headers=W, json={**body, "reviewer": None}).status_code == 400
    assert _calls(log) == []
    r = c.post(decide, headers=W, json=body)
    assert r.status_code == 200 and r.json()["reviewer"] == "token:Ann"

    sign = "/api/workspaces/w/review/pii/signoff"
    ok = {"digest": D, "confirm": "pii", "reviewer": "Ann"}
    assert c.post(sign, headers=R, json=ok).status_code == 403
    assert c.post(sign, headers=W, json={**ok, "confirm": "scope"}).status_code == 400
    assert c.post(sign, headers=W, json=ok).status_code == 200
    stale = c.post(sign, headers=W, json={**ok, "digest": "b" * 64})
    assert stale.status_code == 400 and "changed since" in stale.json()["detail"]
    revoke = "/api/workspaces/w/review/pii/revoke"
    assert c.post(revoke, headers=W, json={"reason": "redo", "reviewer": "Ann"}).status_code == 200
    assert _calls(log) == [
        ["review", "scope", "--in", "--as=token:Ann", "--", "T_REPORT"],
        ["signoff", "approve", "pii", f"--digest={D}", "--as=token:Ann"],
        ["signoff", "approve", "pii", f"--digest={'b' * 64}", "--as=token:Ann"],
        ["signoff", "revoke", "pii", "--reason=redo", "--as=token:Ann"],
    ]
