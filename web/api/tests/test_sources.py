"""Sources and databases from the console (RFC 0223).

A fake `ekos` records its argv and environment, so these tests pin what the console runs, that a
password reaches only the keychain and the child's environment, and who may do what.
"""

from __future__ import annotations

import json
import stat
import sys
from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app import keychain, models
from app.main import create_app
from app.semantics_write import ReviewError
from app.sources_write import database_add_argv, secret_name, source_add_argv


def _not_on_windows(reason: str) -> pytest.MarkDecorator:
    return pytest.mark.skipif(sys.platform == "win32", reason=reason)


R = {"Authorization": "Bearer r"}
W = {"Authorization": "Bearer w"}
PASSWORD = "s3cret-Pa55-for-erp"

FAKE = r"""#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ["FAKE_LOG"], "a") as f:
    f.write(json.dumps({"argv": args, "pw": os.environ.get("EKOS_DB_DEFAULT_WS_ERP")}) + "\n")
state = os.environ["FAKE_STATE"]
if args[:1] == ["mcp"]:
    sys.exit(0)
if args[:2] == ["source", "list"]:
    print(open(state).read() if os.path.exists(state) else "[]")
elif args[:2] == ["source", "add"]:
    cur = json.load(open(state)) if os.path.exists(state) else []
    cur.append({"name": args[2], "kind": "git", "path": args[3][7:], "exists": True, "git": True})
    json.dump(cur, open(state, "w"))
    print("Added source " + args[2])
elif args[:2] == ["database", "list"]:
    print(json.dumps({"databases": [{"name": "erp", "kind": "oracle", "connect": "h:1521/S",
        "user": "ekos_ro", "password_env": "EKOS_DB_DEFAULT_WS_ERP", "password_set": True,
        "owners": ["ERP"], "profile": "p0", "dictionary": None}]}))
elif args[:2] == ["database", "test"]:
    print(json.dumps({"database": args[2], "ok": True, "census_vs_compiled": []}))
else:
    print("ok")
"""


@pytest.fixture
def setup(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None
) -> Iterator[tuple[TestClient, Path, Path]]:
    fake = tmp_path / "ekos"
    fake.write_text(FAKE)
    fake.chmod(fake.stat().st_mode | stat.S_IEXEC)
    log = tmp_path / "argv.log"
    log.write_text("")
    monkeypatch.setenv("FAKE_LOG", str(log))
    monkeypatch.setenv("FAKE_STATE", str(tmp_path / "sources.json"))
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "r")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_BIN", str(fake))
    ws = tmp_path / "ws"
    ws.mkdir()
    code = tmp_path / "code"
    code.mkdir()
    (ws / "ekos.toml").write_text(f'[workspace]\nroot = "{code}"\n')
    (ws / ".ekos").mkdir()
    with TestClient(create_app()) as c:
        c.post("/api/workspaces", headers=W, json={"id": "w", "name": "W", "path": str(ws)})
        yield c, log, tmp_path


def _calls(log: Path) -> list[dict]:
    calls = [json.loads(line) for line in log.read_text().splitlines() if line]
    return [c for c in calls if c["argv"][:1] != ["mcp"]]


# ── pure ─────────────────────────────────────────────────────────────────────────────────────


def test_argv_builders_take_names_only() -> None:
    assert secret_name("default", "my-shop", "erp") == "EKOS_DB_DEFAULT_MY_SHOP_ERP"
    argv = database_add_argv(
        "erp", "db.local:1521/XEPDB1", "ekos_ro", "EKOS_DB_X", ["ERP"], "p2", "idempiere"
    )
    assert argv == [
        "database",
        "add",
        "erp",
        "--connect=db.local:1521/XEPDB1",
        "--user=ekos_ro",
        "--password-env=EKOS_DB_X",
        "--profile=p2",
        "--owner=ERP",
        "--dictionary=idempiere",
    ]
    for name, connect, user, owners in (
        ("Erp", "h:1/S", "u", ["E"]),
        ("erp", "h:1521", "u", ["E"]),
        ("erp", "h:1/S", "u; drop", ["E"]),
        ("erp", "h:1/S", "u", ["E X"]),
        ("erp", "h:1/S", "u", []),
    ):
        with pytest.raises(ReviewError):
            database_add_argv(name, connect, user, "V", owners, "p0", None)
    assert source_add_argv("docs", "/srv/docs", "git") == [
        "source",
        "add",
        "docs",
        "--path=/srv/docs",
        "--kind=git",
    ]


# ── routes ───────────────────────────────────────────────────────────────────────────────────


def test_an_api_token_cannot_enter_a_credential() -> None:
    from fastapi import HTTPException

    from app.auth import Principal
    from app.routes.sources import _no_token

    with pytest.raises(HTTPException) as e:
        _no_token(Principal(kind="token", id="t1", subject="agent", tenant_id="default"))
    assert e.value.status_code == 403
    _no_token(Principal(kind="user", id="u1", subject="ann"))


@_not_on_windows("the fake ekos is a script run by its #! line")
def test_a_person_adds_a_database_and_its_password_reaches_only_the_keychain(setup) -> None:
    c, log, _ = setup
    body = {
        "name": "erp",
        "connect": "h:1521/S",
        "user": "ekos_ro",
        "password": PASSWORD,
        "owners": ["ERP"],
        "profile": "p2",
        "dictionary": "idempiere",
    }
    r = c.post("/api/workspaces/w/databases", headers=W, json=body)
    assert r.status_code == 201, r.text
    assert PASSWORD not in r.text
    assert keychain.get_env("EKOS_DB_DEFAULT_WS_ERP") == PASSWORD
    assert models.get_tenant("default").source_env == ["EKOS_DB_DEFAULT_WS_ERP"]
    call = _calls(log)[-1]
    assert PASSWORD not in json.dumps(call["argv"])
    assert "--password-env=EKOS_DB_DEFAULT_WS_ERP" in call["argv"]
    # The listing says a password is stored, never what it is.
    listed = c.get("/api/workspaces/w/sources", headers=R).json()
    assert listed["databases"][0]["password_stored"] is True
    assert PASSWORD not in json.dumps(listed)
    # The test runs with the password in its environment.
    t = c.post("/api/workspaces/w/databases/erp/test", headers=W)
    assert t.status_code == 200 and t.json()["ok"] is True
    assert _calls(log)[-1]["pw"] == PASSWORD
    # Removing needs admin; it deletes the password and the grant.
    assert c.delete("/api/workspaces/w/databases/erp", headers=R).status_code == 403
    assert c.delete("/api/workspaces/w/databases/erp", headers=W).status_code == 200
    assert keychain.get_env("EKOS_DB_DEFAULT_WS_ERP") is None
    assert models.get_tenant("default").source_env == []


@_not_on_windows("the fake ekos is a script run by its #! line")
def test_the_first_named_source_names_the_project_root_too(setup) -> None:
    c, log, tmp = setup
    docs = tmp / "docs"
    docs.mkdir()
    assert (
        c.post(
            "/api/workspaces/w/sources", headers=R, json={"name": "docs", "path": str(docs)}
        ).status_code
        == 403
    )
    r = c.post(
        "/api/workspaces/w/sources",
        headers=W,
        json={"name": "docs", "path": str(docs), "kind": "git"},
    )
    assert r.status_code == 201, r.text
    adds = [x["argv"] for x in _calls(log) if x["argv"][:2] == ["source", "add"]]
    assert adds == [
        ["source", "add", "code", f"--path={tmp / 'code'}"],
        ["source", "add", "docs", f"--path={docs}", "--kind=git"],
    ]
    # The second one does not add the root again.
    other = tmp / "other"
    other.mkdir()
    c.post("/api/workspaces/w/sources", headers=W, json={"name": "other", "path": str(other)})
    adds = [x["argv"][2] for x in _calls(log) if x["argv"][:2] == ["source", "add"]]
    assert adds == ["code", "docs", "other"]


@_not_on_windows("the fake ekos is a script run by its #! line")
def test_a_source_outside_the_tenants_roots_is_refused(
    setup, monkeypatch: pytest.MonkeyPatch
) -> None:
    c, log, tmp = setup
    models.update_tenant("default", source_roots=[str(tmp / "code")])
    r = c.post("/api/workspaces/w/sources", headers=W, json={"name": "etc", "path": "/etc"})
    assert r.status_code == 400 and "outside" in r.json()["detail"]
    assert not [x for x in _calls(log) if x["argv"][:2] == ["source", "add"]]


@_not_on_windows("the fake ekos is a script run by its #! line")
def test_without_a_keychain_adding_a_database_says_what_to_do(setup, monkeypatch) -> None:
    c, log, _ = setup
    monkeypatch.setenv("EKOS_CONSOLE_KEYCHAIN", "off")
    body = {
        "name": "erp",
        "connect": "h:1521/S",
        "user": "u",
        "password": PASSWORD,
        "owners": ["E"],
    }
    r = c.post("/api/workspaces/w/databases", headers=W, json=body)
    assert r.status_code == 400 and "--password-env" in r.json()["detail"]
    assert not [x for x in _calls(log) if x["argv"][:2] == ["database", "add"]]
