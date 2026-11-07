"""Annotations routes (RFC 0247).

A fake `ekos` script records its argv, so these tests pin down exactly what the console runs, with
which role, who it says wrote, and that a person's text can never become an option.
"""

from __future__ import annotations

import json
import stat
import sys
from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app.annotations_write import act_argv, add_argv, target_ok
from app.main import create_app
from app.readproc import ReadProcError, _check_allowed
from app.semantics_write import ReviewError


def _not_on_windows(reason: str) -> pytest.MarkDecorator:
    return pytest.mark.skipif(sys.platform == "win32", reason=reason)


R = {"Authorization": "Bearer r"}
W = {"Authorization": "Bearer w"}
ID = "cc57a3dc-bd4c-507e-94fe-54f967a3594f"
TARGET = "11111111-2222-4333-8444-555555555555"

FAKE = r"""#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ["FAKE_LOG"], "a") as f:
    f.write(json.dumps(args) + "\n")
if args[:1] == ["mcp"]:
    sys.exit(0)
if args[:2] == ["annotate", "list"]:
    print(json.dumps([{"id": "__ID__", "target": "__T__", "kind": "comment", "text": "in cents",
                       "rule": None, "term": None, "author": "ann", "scope": "personal",
                       "approved_by": None, "state": "active", "updated_at": "t"}]))
elif args[:2] == ["annotate", "add"]:
    print("annotation __ID__ (comment)")
elif args[:2] == ["annotate", "withdraw"] and "--as=token:Bob" in args:
    sys.stderr.write("Error: only ann can withdraw this annotation\n")
    sys.exit(1)
elif args[:1] == ["annotate"]:
    if os.environ.get("FAKE_LOCKED"):
        sys.stderr.write("Error: cannot write: another writable process already holds the lock\n")
        sys.exit(1)
    print("ok")
""".replace("__ID__", ID).replace("__T__", TARGET)


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
    _check_allowed(["annotate", "list", "--json"])
    _check_allowed(["annotate", "list", "--json", "--target", TARGET])
    for argv in (
        ["annotate", "add", "--as=x", "--", TARGET, "comment", "t"],
        ["annotate", "withdraw", ID],
        ["annotate", "approve", ID],
        ["annotate", "resolve", "--rule", "r.json", "--apply"],
        ["annotate", "list", "--json", "--out", "/etc/passwd"],
    ):
        with pytest.raises(ReadProcError):
            _check_allowed(argv)


def test_a_target_is_an_entity_id_or_the_project() -> None:
    assert target_ok("project") == "project"
    assert target_ok(TARGET) == TARGET
    for bad in ("sales", "--all", "project; rm", "", TARGET.upper() + "x"):
        with pytest.raises(ReviewError):
            target_ok(bad)


def test_add_argv_keeps_a_persons_text_out_of_the_options() -> None:
    argv = add_argv(TARGET, "comment", "--as=root --apply", "ann@x.io", shared=True)
    assert argv == [
        "annotate",
        "add",
        "--as=ann@x.io",
        "--shared",
        "--",
        TARGET,
        "comment",
        "--as=root --apply",
    ]
    # Everything a person typed sits after the `--`; nothing before it came from their text.
    assert argv.index("--") < argv.index("--as=root --apply")
    rule = {"rule": "fiscal_year_start", "month": 4}
    syn = add_argv(TARGET, "synonym", "turnover means invoices", "ann", term="turnover")
    assert "--term=turnover" in syn and syn[-3:] == [TARGET, "synonym", "turnover means invoices"]
    ins = add_argv("project", "instruction", "FY starts in April", "ann", rule=rule)
    assert '--rule={"rule":"fiscal_year_start","month":4}' in ins


def test_add_argv_refuses_what_the_cli_would_have_to_refuse() -> None:
    bad = [
        ("sales", "comment", "x", {}),  # a name, not an id
        (TARGET, "wiki", "x", {}),  # unknown kind
        (TARGET, "comment", "  ", {}),  # no text
        (TARGET, "comment", "x\x00y", {}),  # NUL
        (TARGET, "comment", "x" * 2001, {}),  # too long
        (TARGET, "comment", "x", {"rule": {"rule": "fiscal_year_start", "month": 4}}),
        (TARGET, "synonym", "x", {}),  # no word
        (TARGET, "comment", "x", {"term": "word"}),  # a term on a non-synonym
        (TARGET, "instruction", "x", {"rule": {"rule": "run_sql", "sql": "drop table t"}}),
        (TARGET, "instruction", "x", {"rule": "fiscal_year_start"}),  # not an object
    ]
    for target, kind, text, kw in bad:
        with pytest.raises(ReviewError):
            add_argv(target, kind, text, "ann", **kw)


def test_act_argv_is_one_action_on_one_full_id() -> None:
    assert act_argv("approve", ID, "bob") == ["annotate", "approve", ID, "--as=bob"]
    for action, ident in (("delete", ID), ("approve", "cc57a3dc"), ("confirm", "--all")):
        with pytest.raises(ReviewError):
            act_argv(action, ident, "bob")


# ── routes ───────────────────────────────────────────────────────────────────────────────────


@_not_on_windows("the fake ekos is a script run by its #! line")
def test_the_list_needs_only_the_read_role_and_a_valid_target(setup) -> None:
    c, log = setup
    r = c.get("/api/workspaces/w/annotations", headers=R)
    assert r.status_code == 200 and r.json()[0]["id"] == ID
    assert c.get(f"/api/workspaces/w/annotations?target={TARGET}", headers=R).status_code == 200
    assert _calls(log) == [
        ["annotate", "list", "--json"],
        ["annotate", "list", "--json", "--target", TARGET],
    ]
    assert c.get("/api/workspaces/w/annotations?target=--all", headers=R).status_code == 400
    assert c.get("/api/workspaces/w/annotations").status_code == 401
    assert len(_calls(log)) == 2, "a refused target never reached the CLI"


@_not_on_windows("the fake ekos is a script run by its #! line")
def test_writing_needs_the_write_role_and_a_name_in_token_mode(setup) -> None:
    c, log = setup
    body = {"target": TARGET, "kind": "comment", "text": "in cents", "reviewer": "Ann"}
    assert c.post("/api/workspaces/w/annotations", headers=R, json=body).status_code == 403
    no_name = {k: v for k, v in body.items() if k != "reviewer"}
    assert c.post("/api/workspaces/w/annotations", headers=W, json=no_name).status_code == 400
    assert _calls(log) == []

    r = c.post("/api/workspaces/w/annotations", headers=W, json=body)
    assert r.status_code == 200 and r.json()["author"] == "token:Ann"
    assert _calls(log) == [
        ["annotate", "add", "--as=token:Ann", "--", TARGET, "comment", "in cents"]
    ]


@_not_on_windows("the fake ekos is a script run by its #! line")
def test_a_hostile_note_stays_data(setup) -> None:
    c, log = setup
    body = {"target": TARGET, "kind": "comment", "text": "--shared --apply", "reviewer": "Ann"}
    assert c.post("/api/workspaces/w/annotations", headers=W, json=body).status_code == 200
    argv = _calls(log)[0]
    assert argv[-3:] == [TARGET, "comment", "--shared --apply"]
    assert argv[argv.index("--") + 1 :] == [TARGET, "comment", "--shared --apply"]
    assert "--shared" not in argv[: argv.index("--")]


@_not_on_windows("the fake ekos is a script run by its #! line")
def test_withdraw_confirm_and_approve_run_the_matching_command(setup) -> None:
    c, log = setup
    base = f"/api/workspaces/w/annotations/{ID}"
    for action in ("withdraw", "approve", "confirm"):
        assert c.post(f"{base}/{action}", headers=R, json={"reviewer": "Ann"}).status_code == 403
        assert c.post(f"{base}/{action}", headers=W, json={"reviewer": "Ann"}).status_code == 200
    assert _calls(log) == [
        ["annotate", "withdraw", ID, "--as=token:Ann"],
        ["annotate", "approve", ID, "--as=token:Ann"],
        ["annotate", "confirm", ID, "--as=token:Ann"],
    ]
    bad = "/api/workspaces/w/annotations/not-an-id/approve"
    assert c.post(bad, headers=W, json={"reviewer": "Ann"}).status_code == 400


@_not_on_windows("the fake ekos is a script run by its #! line")
def test_cli_refusals_are_400_and_a_held_lock_is_409(
    setup, monkeypatch: pytest.MonkeyPatch
) -> None:
    c, _log = setup
    url = f"/api/workspaces/w/annotations/{ID}/withdraw"
    r = c.post(url, headers=W, json={"reviewer": "Bob"})
    assert r.status_code == 400 and "only ann can withdraw" in r.json()["detail"]
    monkeypatch.setenv("FAKE_LOCKED", "1")
    assert c.post(url, headers=W, json={"reviewer": "Ann"}).status_code == 409
