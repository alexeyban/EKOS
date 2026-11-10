"""RFC 0253/0254 in the console: the inbox, testimony, questions and proposal cards.

A fake `ekos` records its argv, so these tests pin down exactly what the console runs, with
which right, and whose name a write carries.
"""

from __future__ import annotations

import base64
import json
import stat
import sys
from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi import HTTPException
from fastapi.testclient import TestClient

from app import authz, models, people_write, proposals
from app.auth import Principal
from app.chat import tell
from app.main import create_app
from app.readproc import ReadProcError, _check_allowed
from app.routes import _people
from app.semantics_write import ReviewError


def _not_on_windows(reason: str) -> pytest.MarkDecorator:
    return pytest.mark.skipif(sys.platform == "win32", reason=reason)


R = {"Authorization": "Bearer r"}
W = {"Authorization": "Bearer w"}
QID = "0b6a1c2e-1111-5222-8333-944455556666"

FAKE = r"""#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ["FAKE_LOG"], "a") as f:
    f.write(json.dumps(args) + "\n")
if args[:1] == ["mcp"]:
    sys.exit(0)
if args[:2] == ["inbox", "list"]:
    print(json.dumps([{"id": "ab12cd34ef567890", "name": "spec.pdf", "status": "pending",
                       "size": 10, "received_at": "2026-11-10T00:00:00Z",
                       "senders": [{"by": "ann", "channel": "console"}]}]))
elif args[:2] == ["inbox", "add"]:
    path = args[-1]
    if not path.startswith("http"):
        data = open(path, "rb").read()
        assert data == b"hello", data
    print(json.dumps({"id": "ab12cd34ef567890", "name": "spec.pdf",
                      "status": "pending" if "--pending" in args else "accepted", "new": True}))
elif args[:2] in (["inbox", "accept"], ["inbox", "purge"]):
    print("done")
elif args[:2] == ["testimony", "add"]:
    print(json.dumps({"id": "77aa", "author": [a for a in args if a.startswith("--as=")][0][5:]}))
elif args[:2] == ["testimony", "list"]:
    print(json.dumps([{"id": "77aa", "author": "ann", "text": "Status 7 is cancelled"}]))
elif args[:2] == ["questions", "list"]:
    print(json.dumps([{"id": "__QID__", "type": "meaning_of_value", "status": "open",
                       "addressee": "token:Ann", "text": "What does status = 7 mean?"}]))
elif args[:2] == ["questions", "show"]:
    q = {"id": "__QID__", "type": "unexplained_values", "status": "open",
         "addressee": "token:Ann", "text": "?"}
    if "--reveal" in args:
        q["revealed"] = [{"value": "9", "rows": 120}]
    print(json.dumps(q))
elif args[:2] in (["questions", "decline"], ["questions", "reassign"], ["questions", "mute"]):
    print("ok")
""".replace("__QID__", QID)


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


# ── pure ─────────────────────────────────────────────────────────────────────


def test_the_reads_are_on_the_allowlist_and_nothing_else() -> None:
    for argv in (
        ["inbox", "list", "--json", "--status", "pending"],
        ["testimony", "list", "--json", "--about", "orders"],
        ["testimony", "list", "--json", "--all", "--author", "ann"],
        ["questions", "list", "--json", "--status", "open", "--type", "dead_code"],
        ["questions", "digest", "--json"],
        ["questions", "show", "--json", QID],
        ["questions", "show", "--json", "--reveal", QID[:8]],
    ):
        _check_allowed(argv)
    for argv in (
        ["inbox", "add", "x"],
        ["inbox", "accept", "ab12"],
        ["testimony", "add", "--text=x"],
        ["questions", "show", "--json", "--reveal", "--out", "/etc/passwd"],
        ["questions", "show", "--json", "../../x"],
        ["questions", "list", "--json", "--out", "x"],
        ["questions", "decline", QID],
    ):
        with pytest.raises(ReadProcError):
            _check_allowed(argv)


def test_testimony_argv_keeps_every_value_behind_its_flag() -> None:
    argv = people_write.testimony_add_argv(
        "--status 7 means cancelled",
        "ann@x.io",
        about=["orders.status = 7"],
        answers=QID,
        meaning="cancelled by the customer",
        channel="slack",
        message_ref="C1/123.4",
        on_behalf_of="Bob",
        attachments=["ab12cd34"],
    )
    assert argv == [
        "testimony", "add", "--text=--status 7 means cancelled", "--as=ann@x.io",
        "--channel=slack", "--about=orders.status = 7", f"--answers={QID}",
        "--meaning=cancelled by the customer", "--message-ref=C1/123.4",
        "--on-behalf-of=Bob", "--attach=ab12cd34", "--json",
    ]  # fmt: skip
    for bad in (
        lambda: people_write.testimony_add_argv("  ", "ann"),
        lambda: people_write.testimony_add_argv("x", "ann", channel="email"),
        lambda: people_write.testimony_add_argv("x", "ann", answers="not-an-id!"),
        lambda: people_write.testimony_add_argv("x", "ann", about=["a\nb"]),
        lambda: people_write.testimony_add_argv("x", "ann", attachments=["../x"]),
    ):
        with pytest.raises(ReviewError):
            bad()


def test_inbox_argv_puts_the_file_after_the_options() -> None:
    assert people_write.inbox_add_argv("/tmp/a.pdf", "ann", pending=True) == [
        "inbox", "add", "--as=ann", "--channel=console", "--pending", "--json", "--", "/tmp/a.pdf",
    ]  # fmt: skip
    with pytest.raises(ReviewError):
        people_write.inbox_add_argv("--help", "ann")
    with pytest.raises(ReviewError):
        people_write.inbox_accept_argv("../../etc", "ann")
    assert people_write.mute_argv("ad_*", "ann") == [
        "questions", "mute", "--as=ann", "--", "ad_*",
    ]  # fmt: skip
    with pytest.raises(ReviewError):
        people_write.mute_argv("--all", "ann")


def test_an_api_token_never_writes_as_a_person() -> None:
    token = Principal(kind="token", id="t1", subject="superset", tenant_id="default")
    assert not authz.TOKEN_ACTIONS & {"review", "admin"}
    with pytest.raises(HTTPException) as e:
        _people.person(token, _settings(), None)
    assert e.value.status_code == 403


def _settings():
    from app.settings import Settings

    return Settings(console_db=":memory:")


# ── cite-or-drop ─────────────────────────────────────────────────────────────

STATEMENT = "In the orders table status 7 is when the customer cancels; amounts are in cents."


def test_a_proposal_must_quote_the_persons_words() -> None:
    reply = json.dumps(
        {
            "proposals": [
                {"kind": "meaning", "about": "orders.status = 7",
                 "meaning": "cancelled by the customer",
                 "quote": "status 7 is when the customer cancels"},
                {"kind": "about", "about": "orders.amount", "quote": "amounts are in cents"},
                # No quote: dropped.
                {"kind": "about", "about": "orders.total"},
                # A quote the person never wrote: dropped.
                {"kind": "meaning", "about": "orders.status = 9", "meaning": "refunded",
                 "quote": "status 9 is a refund"},
                # A hedged meaning is a guess: dropped.
                {"kind": "meaning", "about": "orders.status = 7", "meaning": "probably a return",
                 "quote": "status 7"},
                # An unknown kind: dropped.
                {"kind": "decide", "about": "x", "quote": "orders"},
            ]
        }
    )  # fmt: skip
    kept, dropped = tell.keep_cited(f"Here you go:\n```json\n{reply}\n```", STATEMENT)
    assert [k["about"] for k in kept] == ["orders.status = 7", "orders.amount"]
    assert dropped == 4
    assert tell.keep_cited("not json", STATEMENT) is None


def _proposal(author: str, items: list[dict], **kw) -> models.Proposal:
    return models.Proposal(
        id="p1", workspace_id="w", author=author, text=STATEMENT, items=items,
        message_ref="m1", **kw,
    )  # fmt: skip


def test_save_writes_the_persons_text_one_testimony_per_meaning() -> None:
    items = [
        {"kind": "meaning", "about": "orders.status = 7", "meaning": "cancelled", "quote": "q"},
        {"kind": "meaning", "about": "orders.status = 8", "meaning": "shipped", "quote": "q"},
        {"kind": "about", "about": "orders.amount", "quote": "q"},
        {"kind": "answers", "question": QID, "quote": "q"},
    ]
    calls = proposals.save_argvs(_proposal("user:u1", items), "ann@x.io")
    assert len(calls) == 2
    assert calls[0][2] == f"--text={STATEMENT}" and "--as=ann@x.io" in calls[0]
    assert "--meaning=cancelled" in calls[0] and f"--answers={QID}" in calls[0]
    assert "--about=orders.amount" in calls[0] and "--about=orders.status = 7" in calls[0]
    assert "--message-ref=m1#2" in calls[1] and not any(a.startswith("--answers") for a in calls[1])
    # An edit replaces the text; the kept list narrows the proposals.
    edited = proposals.save_argvs(
        _proposal("user:u1", items), "ann@x.io", edited="My own words.", keep=[2]
    )
    assert edited == [
        ["testimony", "add", "--text=My own words.", "--as=ann@x.io", "--channel=console",
         "--about=orders.amount", "--message-ref=m1", "--json"]
    ]  # fmt: skip
    with pytest.raises(ReviewError):
        proposals.save_argvs(_proposal("user:u1", items), "ann", keep=[9])


# ── routes ───────────────────────────────────────────────────────────────────

FAKE_OS = _not_on_windows("the fake ekos is a script run by its #! line")


@FAKE_OS
def test_the_inbox_reads_and_a_readers_material_waits(setup) -> None:
    c, log = setup
    r = c.get("/api/workspaces/w/inbox?status=pending", headers=R)
    assert r.status_code == 200 and r.json()[0]["name"] == "spec.pdf"
    body = {"name": "../../spec.pdf", "content_base64": base64.b64encode(b"hello").decode()}
    # The reader sends: stored as pending, under the typed name in token mode.
    assert c.post("/api/workspaces/w/inbox", headers=R, json=body).status_code == 400  # no name
    r = c.post("/api/workspaces/w/inbox", headers=R, json={**body, "reviewer": "Ann"})
    assert r.status_code == 200 and r.json()["status"] == "pending"
    add = _calls(log)[-1]
    assert add[:5] == ["inbox", "add", "--as=token:Ann", "--channel=console", "--pending"]
    assert Path(add[-1]).name == "spec.pdf"  # the name cleaned of its directories
    assert not Path(add[-1]).exists()  # the temporary copy is gone
    # A reviewer's material is accepted at once; a link goes as it is.
    r = c.post(
        "/api/workspaces/w/inbox", headers=W, json={"url": "https://x.io/p", "reviewer": "Bo"}
    )
    assert r.status_code == 200 and "--pending" not in _calls(log)[-1]
    assert _calls(log)[-1][-1] == "https://x.io/p"
    assert (
        c.post(
            "/api/workspaces/w/inbox",
            headers=W,
            json={"url": "file:///etc/passwd", "reviewer": "B"},
        ).status_code
        == 400
    )


@FAKE_OS
def test_accept_needs_review_and_purge_needs_admin_and_the_id_typed(setup) -> None:
    c, log = setup
    url = "/api/workspaces/w/inbox/ab12cd34ef567890"
    assert c.post(url + "/accept", headers=R, json={"reviewer": "Ann"}).status_code == 403
    r = c.post(url + "/accept", headers=W, json={"reviewer": "Ann"})
    assert r.status_code == 200 and _calls(log)[-1] == [
        "inbox", "accept", "ab12cd34ef567890", "--as=token:Ann",
    ]  # fmt: skip
    assert c.post(url + "/purge", headers=W, json={"reviewer": "Ann"}).status_code == 409
    r = c.post(url + "/purge", headers=W, json={"reviewer": "Ann", "confirm": "ab12cd34ef567890"})
    assert r.status_code == 200 and _calls(log)[-1][:2] == ["inbox", "purge"]


@FAKE_OS
def test_testimony_is_the_signed_in_persons_own(setup) -> None:
    c, log = setup
    url = "/api/workspaces/w/testimony"
    assert c.get(url + "?about=orders", headers=R).json()[0]["id"] == "77aa"
    assert _calls(log)[-1] == ["testimony", "list", "--json", "--about", "orders"]
    body = {"text": "Status 7 is cancelled", "about": ["orders.status = 7"], "reviewer": "Ann"}
    assert c.post(url, headers=R, json=body).status_code == 403  # a reader does not record
    r = c.post(url, headers=W, json=body)
    assert r.status_code == 200 and r.json()["by"] == "token:Ann"
    assert _calls(log)[-1][:4] == [
        "testimony", "add", "--text=Status 7 is cancelled", "--as=token:Ann",
    ]  # fmt: skip


@FAKE_OS
def test_questions_list_show_reveal_and_answer_for_someone(setup) -> None:
    c, log = setup
    base = "/api/workspaces/w/questions"
    r = c.get(base + "?status=all&type=dead_code&table=orders&limit=5", headers=R)
    assert r.status_code == 200
    assert _calls(log)[-1] == [
        "questions", "list", "--json", "--status", "all", "--type", "dead_code",
        "--table", "orders", "--limit", "5",
    ]  # fmt: skip
    assert c.get(base + "?type=bogus", headers=R).status_code == 422
    assert c.get(f"{base}/{QID}", headers=R).json()["id"] == QID
    r = c.get(f"{base}/{QID}/reveal", headers=R)
    assert r.json()["revealed"][0]["rows"] == 120
    assert _calls(log)[-1] == ["questions", "show", "--json", "--reveal", QID]
    # Who looked is audited; what they saw is not.
    audit = [a for a in models.list_audit("default") if a.action == "questions: reveal"]
    assert audit and "120" not in audit[0].detail
    # An answer heard in a meeting is the operator's testimony, naming who said it.
    answer = {"text": "It is cancelled", "said_by": "Bob", "reviewer": "Ann"}
    assert c.post(f"{base}/{QID}/answer", headers=R, json=answer).status_code == 403
    r = c.post(f"{base}/{QID}/answer", headers=W, json=answer)
    assert r.status_code == 200
    call = _calls(log)[-1]
    assert "--as=token:Ann" in call and "--on-behalf-of=Bob" in call
    assert f"--answers={QID}" in call


@FAKE_OS
def test_not_me_only_by_the_person_it_is_with_and_reassign_needs_operate(setup) -> None:
    c, log = setup
    base = f"/api/workspaces/w/questions/{QID}"
    r = c.post(base + "/decline", headers=R, json={"reviewer": "Bob"})
    assert r.status_code == 403
    r = c.post(base + "/decline", headers=R, json={"reviewer": "Ann"})
    assert r.status_code == 200 and _calls(log)[-1] == [
        "questions", "decline", QID, "--as=token:Ann",
    ]  # fmt: skip
    assert c.post(base + "/reassign", headers=R, json={"to": "bob@x.io"}).status_code == 403
    r = c.post(base + "/reassign", headers=W, json={"to": "bob@x.io"})
    assert r.status_code == 200 and _calls(log)[-1][-1] == "--to=bob@x.io"
    r = c.post(
        "/api/workspaces/w/questions/mute", headers=R, json={"pattern": "ad_*", "reviewer": "Ann"}
    )
    assert r.status_code == 200 and _calls(log)[-1][-1] == "ad_*"


@FAKE_OS
def test_only_the_author_saves_a_card(setup) -> None:
    c, log = setup
    items = [{"kind": "about", "about": "orders.amount", "quote": "amounts are in cents"}]
    models.add(_proposal("user:somebody-else", items))
    r = c.post("/api/workspaces/w/proposals/p1/save", headers=W, json={"reviewer": "Ann"})
    assert r.status_code == 404  # not this caller's card
    models.add(
        models.Proposal(
            id="p2", workspace_id="w", author="static:write", text=STATEMENT, items=items
        )
    )
    assert c.post("/api/workspaces/w/proposals/p2/save", headers=R, json={}).status_code == 403
    r = c.post(
        "/api/workspaces/w/proposals/p2/save",
        headers=W,
        json={"reviewer": "Ann", "text": "Amounts are in cents."},
    )
    assert r.status_code == 200
    assert _calls(log)[-1][:4] == [
        "testimony", "add", "--text=Amounts are in cents.", "--as=token:Ann",
    ]  # fmt: skip
    assert models.get_proposal("p2").status == "saved"
    # Saved once: a second click changes nothing.
    r = c.post("/api/workspaces/w/proposals/p2/save", headers=W, json={"reviewer": "Ann"})
    assert r.status_code == 409


@FAKE_OS
def test_nothing_is_taken_in_a_public_deployment(setup, monkeypatch: pytest.MonkeyPatch) -> None:
    c, _ = setup
    monkeypatch.setattr(c.app.state.settings, "moderation_enabled", True)
    body = {"url": "https://x.io/p", "reviewer": "Ann"}
    assert c.post("/api/workspaces/w/inbox", headers=W, json=body).status_code == 403
