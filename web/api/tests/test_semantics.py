"""Business semantics review + LinkML viewer/editor routes (RFC 0170).

A fake `ekos` script stands in for the CLI: it records its argv and prints a canned payload per
subcommand, so these tests pin down exactly what the console runs and who it says is reviewing.
"""

from __future__ import annotations

import json
import stat
from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app.auth import Principal
from app.main import create_app
from app.readproc import ReadProcError, _check_allowed
from app.semantics_write import ReviewError, bulk_argv, review_argv, reviewer_for

R = {"Authorization": "Bearer r"}
W = {"Authorization": "Bearer w"}
ID = "3aec3aa7-3fc0-51fd-b90f-a91baa49d3c1"

FAKE = r"""#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ["FAKE_LOG"], "a") as f:
    f.write(json.dumps(args) + "\n")
if args[:1] == ["mcp"]:
    sys.exit(0)
writes = args[0] in ("semantics", "import") and args[1] not in ("list", "show", "gaps")
if os.environ.get("FAKE_LOCKED") and writes and "--dry-run" not in args:
    sys.stderr.write("Error: cannot write: another writable process already holds the lock\n")
    sys.exit(1)
if args[:2] == ["semantics", "list"]:
    print(json.dumps([{"id": "__ID__", "kind": "BusinessConcept", "name": "PartsNotObsolete",
                       "properties": {"status": "hypothesis"}}]))
elif args[:2] == ["semantics", "show"]:
    print(json.dumps({"id": args[-1], "kind": "BusinessConcept", "evidence": [], "links": []}))
elif args[:2] == ["semantics", "gaps"]:
    print("[]")
elif args[:2] == ["export", "linkml"] and "--json" not in args:
    print("name: t\nclasses: {}\n")
elif args[:2] == ["export", "linkml"]:
    if "confirmed" in args:
        sys.stderr.write("Error: nothing to export with --status Confirmed: 3 hypothesis\n")
        sys.exit(1)
    print(json.dumps({"name": "t", "classes": {}}))
elif args[:2] == ["import", "linkml"]:
    text = open(args[2]).read()
    print(json.dumps({"decisions": [{"id": "c1", "action": "edit"}] if "edited" in text else [],
                      "warnings": [], "errors": [], "applied": 0 if "--dry-run" in args else 1}))
elif args[0] == "semantics":
    print(f"BusinessConcept x — {args[1]}ed")
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
    """The CLI calls the routes made — not the supervisor's `mcp serve` launches."""
    calls = [json.loads(line) for line in log.read_text().splitlines() if line]
    return [c for c in calls if c[:1] != ["mcp"]]


# ── pure ─────────────────────────────────────────────────────────────────────────────────────


def test_reads_are_on_the_allowlist_and_ids_must_be_uuids() -> None:
    _check_allowed(["semantics", "list", "--json", "--kind", "concept", "--status", "hypothesis"])
    _check_allowed(["semantics", "gaps", "--json"])
    _check_allowed(["export", "linkml", "--json", "--status", "all"])
    _check_allowed(["semantics", "show", "--json", ID])
    for argv in (
        ["semantics", "show", "--json", "PartsNotObsolete"],
        ["semantics", "show", "--json", ID, "--extra"],
        ["semantics", "confirm", ID],
        ["import", "linkml", "/tmp/x"],
        ["export", "linkml", "--json", "--out", "/etc/passwd"],
    ):
        with pytest.raises(ReadProcError):
            _check_allowed(argv)


def test_review_argv_keeps_every_value_inside_its_flag() -> None:
    argv = review_argv("edit", ID, "ann@x.io", name="--yes", description="d")
    assert argv == ["semantics", "edit", ID, "--as=ann@x.io", "--name=--yes", "--description=d"]
    with pytest.raises(ReviewError):
        review_argv("reject", ID, "ann")  # no note
    with pytest.raises(ReviewError):
        review_argv("edit", ID, "ann")  # nothing to edit
    with pytest.raises(ReviewError):
        review_argv("confirm", ID, "ann", label="x")
    with pytest.raises(ReviewError):
        review_argv("confirm", "../../etc", "ann")


def test_token_mode_records_a_marked_claim_and_oidc_an_identity() -> None:
    p = Principal(subject="token", role="write")
    assert reviewer_for(p, False, "Ann Smith") == "token:Ann Smith"
    with pytest.raises(ReviewError):
        reviewer_for(p, False, None)
    with pytest.raises(ReviewError):
        reviewer_for(p, False, "x; rm -rf /")
    o = Principal(subject="sub-1", email="ann@x.io", role="write")
    assert reviewer_for(o, True, "ignored") == "ann@x.io"


# ── routes ───────────────────────────────────────────────────────────────────────────────────


def test_reads_need_only_the_read_role(setup) -> None:
    c, log = setup
    r = c.get("/api/workspaces/w/semantics/items?kind=concept&status=hypothesis", headers=R)
    assert r.status_code == 200 and r.json()[0]["name"] == "PartsNotObsolete"
    assert c.get(f"/api/workspaces/w/semantics/items/{ID}", headers=R).status_code == 200
    assert c.get("/api/workspaces/w/semantics/items/not-a-uuid", headers=R).status_code == 400
    assert c.get("/api/workspaces/w/semantics/gaps", headers=R).json() == []
    assert c.get("/api/workspaces/w/semantics/linkml", headers=R).json()["empty"] is False
    y = c.get("/api/workspaces/w/semantics/linkml/yaml", headers=R).json()
    assert y["yaml"].startswith("name: t")
    assert ["export", "linkml", "--status", "all"] in _calls(log), "--yaml never reaches the CLI"
    empty = c.get("/api/workspaces/w/semantics/linkml?status=confirmed", headers=R).json()
    assert empty["empty"] is True and "hypothesis" in empty["reason"]
    assert _calls(log)[0] == [
        "semantics",
        "list",
        "--json",
        "--kind",
        "concept",
        "--status",
        "hypothesis",
    ]


def test_a_decision_needs_the_write_role_and_records_who(setup) -> None:
    c, log = setup
    url = f"/api/workspaces/w/semantics/items/{ID}/review"
    body = {"action": "confirm", "reviewer": "Ann"}
    assert c.post(url, headers=R, json=body).status_code == 403
    assert c.post(url, headers=W, json={"action": "confirm"}).status_code == 400  # who?
    r = c.post(url, headers=W, json=body)
    assert r.status_code == 200, r.text
    assert r.json()["reviewer"] == "token:Ann"
    assert _calls(log)[-1] == ["semantics", "confirm", ID, "--as=token:Ann"]
    bad = c.post(url, headers=W, json={"action": "reject", "reviewer": "Ann"})
    assert bad.status_code == 400 and "note" in bad.json()["detail"]


def test_a_busy_ledger_is_a_conflict(setup, monkeypatch: pytest.MonkeyPatch) -> None:
    c, _ = setup
    monkeypatch.setenv("FAKE_LOCKED", "1")
    r = c.post(
        f"/api/workspaces/w/semantics/items/{ID}/review",
        headers=W,
        json={"action": "confirm", "reviewer": "Ann"},
    )
    assert r.status_code == 409


def test_linkml_validate_is_a_read_and_import_is_a_write(setup) -> None:
    c, log = setup
    v = c.post(
        "/api/workspaces/w/semantics/linkml/validate", headers=R, json={"yaml": "name: edited"}
    )
    assert v.status_code == 200 and v.json()["decisions"][0]["action"] == "edit"
    assert "--dry-run" in _calls(log)[-1]
    url = "/api/workspaces/w/semantics/linkml/import"
    assert c.post(url, headers=R, json={"yaml": "name: edited"}).status_code == 403
    a = c.post(url, headers=W, json={"yaml": "name: edited", "reviewer": "Ann"})
    assert a.status_code == 200 and a.json()["applied"] == 1
    call = _calls(log)[-1]
    assert call[:2] == ["import", "linkml"] and "--as=token:Ann" in call
    assert not Path(call[2]).exists(), "the temp file is removed"


def test_a_client_side_route_reloads_into_the_app(tmp_path: Path) -> None:
    """A reload of `/w/<id>/semantics` serves the UI, not a 404 (the React router owns it)."""
    import asyncio

    from app.main import _SpaFiles

    (tmp_path / "index.html").write_text("<html>app</html>")
    files = _SpaFiles(directory=tmp_path, html=True)
    scope = {"type": "http", "method": "GET", "headers": []}
    resp = asyncio.run(files.get_response("w/lsmb/semantics", scope))
    assert resp.status_code == 200
    from starlette.exceptions import HTTPException as StarletteHTTPException

    for missing in ("assets/missing.js", "api/nope"):
        with pytest.raises(StarletteHTTPException):
            asyncio.run(files.get_response(missing, scope))


ID2 = "5d04c82a-bbf0-5307-a0c3-f27beda79347"


def test_bulk_argv_is_one_all_or_nothing_cli_call() -> None:
    assert bulk_argv("confirm", [ID, ID2], "ann", note=None) == [
        "semantics",
        "confirm",
        ID,
        ID2,
        "--as=ann",
    ]
    for bad in (
        lambda: bulk_argv("confirm", [], "ann", note=None),
        lambda: bulk_argv("confirm", [ID, ID], "ann", note=None),
        lambda: bulk_argv("confirm", [ID, "--all"], "ann", note=None),
        lambda: bulk_argv("reject", [ID], "ann", note=" "),
    ):
        with pytest.raises(ReviewError):
            bad()


def test_bulk_review_route(setup) -> None:
    c, log = setup
    url = "/api/workspaces/w/semantics/review-bulk"
    body = {"action": "confirm", "ids": [ID, ID2], "reviewer": "Ann"}
    assert c.post(url, headers=R, json=body).status_code == 403
    r = c.post(url, headers=W, json=body)
    assert r.status_code == 200 and r.json()["count"] == 2, r.text
    assert _calls(log)[-1] == ["semantics", "confirm", ID, ID2, "--as=token:Ann"]
    assert c.post(url, headers=W, json={**body, "action": "edit"}).status_code == 422
