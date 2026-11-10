"""RFC 0253/0254 through a channel: linking, the project, intake, tell → card → Save by the author
only, the group-chat rule, the inbound endpoints and the questions digest.

The adapter here is a fake that records what would be sent; the transports themselves are tested
against recorded shapes in `test_channel_adapters.py`. A fake `ekos` records argv. No model is
called: a scripted provider answers.
"""

from __future__ import annotations

import json
import stat
import sys
import time
from collections.abc import Iterator
from pathlib import Path
from typing import Any

import pytest
from fastapi.testclient import TestClient

from app import models
from app.auth import Principal
from app.channels import actions, service, slack
from app.channels.core import Attachment, ButtonClick, ChannelMessage, OutMessage
from app.chat import providers
from app.main import create_app

FAKE_OS = pytest.mark.skipif(sys.platform == "win32", reason="the fake ekos runs by its #! line")
OWNER = {"Authorization": "Bearer w"}
QID = "0b6a1c2e-1111-5222-8333-944455556666"

FAKE = r"""#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ["FAKE_LOG"], "a") as f:
    f.write(json.dumps(args) + "\n")
if args[:2] == ["inbox", "add"]:
    print(json.dumps({"id": "ab12cd34ef567890", "name": "invoice-spec.pdf",
                      "status": "pending" if "--pending" in args else "accepted"}))
elif args[:2] == ["testimony", "add"]:
    print(json.dumps({"id": "77aa0000"}))
elif args[:2] == ["questions", "digest"]:
    print(json.dumps({"per_person": {
        "ann@x.io": [{"id": "__QID__", "text": "What does orders.status = 7 mean?"}],
        "nobody@x.io": [{"id": "__QID__", "text": "?"}]}, "unrouted": 0, "cap": 5}))
elif args[:2] == ["questions", "decline"]:
    print("ok")
""".replace("__QID__", QID)


class FakeAdapter:
    name = "telegram"

    def __init__(self, members: list[str] | None = None) -> None:
        self.sent: list[OutMessage] = []
        self.clicks: list[str] = []
        self._members = members

    async def send(self, out: OutMessage) -> str:
        self.sent.append(out)
        return str(len(self.sent))

    async def answer_click(self, click, text: str) -> None:
        self.clicks.append(text)

    async def download(self, att: Attachment) -> bytes:
        return b"hello"

    async def members(self, msg: ChannelMessage) -> list[str] | None:
        return self._members

    async def aclose(self) -> None:
        pass


class FakeMcp:
    async def list_tools(self):
        return [
            {"name": "ekos_search", "description": "s", "inputSchema": {"type": "object"}},
            {"name": "ekos_identity_review", "description": "r", "inputSchema": {}},
        ]

    async def call_tool_raw(self, name, arguments=None, *, scope=None):
        return {"content": [{"type": "text", "text": "orders is a table"}]}


class Supervisor:
    async def acquire(self, ws):
        return FakeMcp()


class Scripted:
    name = "scripted"

    def __init__(self, replies: list[providers.Reply]) -> None:
        self.replies = list(replies)
        self.tools: list[set[str]] = []

    async def complete(self, system, messages, tools):
        self.tools.append({t["name"] for t in tools})
        return self.replies.pop(0)


def _r(text="", calls=()):
    return providers.Reply(text, [providers.ToolCall(*c) for c in calls], {})


@pytest.fixture
def env(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None
) -> Iterator[dict[str, Any]]:
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
    monkeypatch.setenv("EKOS_CONSOLE_CHAT_PROVIDER", "ollama")
    monkeypatch.setenv("EKOS_BIN", str(fake))
    ws = tmp_path / "ws"
    ws.mkdir()
    (ws / "ekos.toml").write_text("[observe]\n")
    (ws / ".ekos").mkdir()
    with TestClient(create_app()) as c:
        models.add_workspace(models.Workspace(id="w", name="Shop", path=str(ws)))
        for uid, email, role in (("u1", "ann@x.io", "reviewer"), ("u2", "bob@x.io", "viewer")):
            models.add(models.User(id=uid, subject=f"s-{uid}", email=email))
            models.add(
                models.RoleBinding(
                    id=f"b-{uid}", tenant_id="default", principal_kind="user",
                    principal_id=uid, role=role, workspace_id="w",
                )
            )  # fmt: skip
        settings = c.app.state.settings
        svc = service.ChannelService(settings, Supervisor(), {"telegram": FakeAdapter()})
        yield {"client": c, "log": log, "svc": svc, "settings": settings, "mp": monkeypatch}


def _calls(log: Path) -> list[list[str]]:
    return [json.loads(line) for line in log.read_text().splitlines() if line]


def _ann() -> Principal:
    return Principal(kind="user", id="u1", subject="s-u1", email="ann@x.io")


def _link(env, account: str, principal: Principal, default_ws: str = "w") -> None:
    code = actions.new_link_code("telegram", account, env["settings"])
    actions.complete_link(principal, code)
    acc = models.get_channel_account("telegram", account)
    assert acc is not None
    acc.default_workspace = default_ws
    models.save_channel_account(acc)


def _dm(account: str, text: str, **kw) -> ChannelMessage:
    return ChannelMessage("telegram", account, account, f"m-{time.time_ns()}", text, **kw)


# ── linking ──────────────────────────────────────────────────────────────────


async def test_an_unlinked_account_gets_only_the_link_instructions(env) -> None:
    svc, adapter = env["svc"], env["svc"].adapters["telegram"]
    model = Scripted([])
    env["mp"].setattr(providers, "from_settings", lambda s: model)
    (out,) = await svc.on_message(adapter, _dm("7001", "what is the orders table?"))
    assert "not linked" in out.text and model.tools == []  # no model, no reads
    (out,) = await svc.on_message(adapter, _dm("7001", "/ekos tell status 7 is cancelled"))
    assert "not linked" in out.text
    (out,) = await svc.on_message(adapter, _dm("7001", "/ekos link"))
    code = out.text.split("Your code: ")[1].split()[0]
    assert len(code) == 8
    stored = [r.code_hash for r in _all(models.LinkCode)]
    assert code not in stored  # only its hash is kept


def _all(table):
    from sqlmodel import select

    with models.session() as s:
        return list(s.exec(select(table)).all())


def test_a_code_links_once_for_a_signed_in_person_only(env) -> None:
    c = env["client"]
    code = actions.new_link_code("telegram", "7001", env["settings"])
    # The development write token is not a person.
    r = c.post("/api/channels/link", headers=OWNER, json={"code": code})
    assert r.status_code == 400 and "signed-in person" in r.json()["detail"]
    account = actions.complete_link(_ann(), code)
    assert account.label == "ann@x.io" and account.principal_id == "u1"
    with pytest.raises(actions.ActionRefused):
        actions.complete_link(_ann(), code)  # used up
    expired = actions.new_link_code("telegram", "7002", env["settings"])
    with models.session() as s:
        row = s.exec(__import__("sqlmodel").select(models.LinkCode)).first()
        row.expires_at = row.expires_at.replace(year=2000)
        s.add(row)
        s.commit()
    with pytest.raises(actions.ActionRefused, match="expired"):
        actions.complete_link(_ann(), expired)


# ── asking ───────────────────────────────────────────────────────────────────


async def test_a_linked_person_asks_with_read_tools_only(env) -> None:
    svc, adapter = env["svc"], env["svc"].adapters["telegram"]
    _link(env, "7001", _ann())
    model = Scripted([_r(calls=[("a", "ekos_search", {"query": "orders"})]), _r("A table [t1].")])
    env["mp"].setattr(providers, "from_settings", lambda s: model)
    (out,) = await svc.on_message(adapter, _dm("7001", "what is orders?"))
    assert out.text == "A table [t1]." and not out.private_to
    assert model.tools[0] == {"ekos_search"}  # never a review tool
    conv = _all(models.Conversation)[0]
    assert conv.channel == "telegram" and conv.owner == "user:u1"


async def test_a_group_answer_is_private_when_a_member_could_not_read_it(env) -> None:
    svc = env["svc"]
    _link(env, "7001", _ann())
    models.add(
        models.ChannelBinding(id="telegram:-100", channel="telegram", conversation="-100",
                              workspace_id="w")
    )  # fmt: skip
    group = ChannelMessage("telegram", "7001", "-100", "m1", "what is orders?", direct=False)

    def model():
        return Scripted([_r(calls=[("a", "ekos_search", {})]), _r("A table.")])

    # Every member is linked and sees the whole project: the thread.
    svc.adapters["telegram"] = FakeAdapter(members=["7001"])
    env["mp"].setattr(providers, "from_settings", lambda s: model())
    (out,) = await svc.on_message(svc.adapters["telegram"], group)
    assert not out.private_to
    # A member nobody linked: only the asker sees it.
    svc.adapters["telegram"] = FakeAdapter(members=["7001", "9999"])
    (out,) = await svc.on_message(svc.adapters["telegram"], group)
    assert out.private_to == "7001"
    # Members unknown (Telegram cannot list them): privately too.
    svc.adapters["telegram"] = FakeAdapter(members=None)
    (out,) = await svc.on_message(svc.adapters["telegram"], group)
    assert out.private_to == "7001"


# ── telling and saving ───────────────────────────────────────────────────────

STATEMENT = "status 7 in orders is when the customer cancels"


def _tell_model(env) -> None:
    reply = {
        "proposals": [
            {"kind": "meaning", "about": "orders.status = 7",
             "meaning": "cancelled by the customer",
             "quote": STATEMENT},
            {"kind": "about", "about": "orders", "quote": "words nobody said"},
        ]
    }  # fmt: skip
    env["mp"].setattr(providers, "from_settings", lambda s: Scripted([_r(json.dumps(reply))]))


@FAKE_OS
async def test_tell_makes_a_card_only_its_author_can_save(env) -> None:
    svc, adapter = env["svc"], env["svc"].adapters["telegram"]
    _link(env, "7001", _ann())
    _link(env, "7002", Principal(kind="user", id="u2", subject="s-u2", email="bob@x.io"))
    _tell_model(env)
    (card,) = await svc.on_message(adapter, _dm("7001", f"/ekos tell {STATEMENT}"))
    assert "cancelled by the customer" in card.text and "1 proposal(s) dropped" in card.text
    assert [b.label for b in card.buttons] == ["Save", "Edit", "Discard"]
    save = card.buttons[0].ref
    # Bob presses Ann's button: not counted, nothing written.
    click = ButtonClick("telegram", "7002", "7002", save)
    assert "for the person it was sent to" in await svc.on_click(adapter, click)
    assert not any(c[:2] == ["testimony", "add"] for c in _calls(env["log"]))
    # Ann presses it: her words, attributed to her.
    text = await svc.on_click(adapter, ButtonClick("telegram", "7001", "7001", save))
    assert text == "Saved as what ann@x.io said."
    (call,) = [c for c in _calls(env["log"]) if c[:2] == ["testimony", "add"]]
    assert call[2] == f"--text={STATEMENT}" and "--as=ann@x.io" in call
    assert "--channel=telegram" in call and "--meaning=cancelled by the customer" in call
    # A second press changes nothing.
    assert "already saved" in await svc.on_click(
        adapter, ButtonClick("telegram", "7001", "7001", save)
    )


@FAKE_OS
async def test_an_edit_replaces_the_words_saved(env) -> None:
    svc, adapter = env["svc"], env["svc"].adapters["telegram"]
    _link(env, "7001", _ann())
    _tell_model(env)
    (card,) = await svc.on_message(adapter, _dm("7001", f"/ekos tell {STATEMENT}"))
    pid = card.text.split("(card ")[1][:8]
    (again,) = await svc.on_message(adapter, _dm("7001", f"/ekos edit {pid} Status 7: cancelled."))
    await svc.on_click(adapter, ButtonClick("telegram", "7001", "7001", again.buttons[0].ref))
    (call,) = [c for c in _calls(env["log"]) if c[:2] == ["testimony", "add"]]
    assert call[2] == "--text=Status 7: cancelled."


async def test_an_answer_to_a_question_card_names_the_question(env) -> None:
    svc, adapter = env["svc"], env["svc"].adapters["telegram"]
    _link(env, "7001", _ann())
    _tell_model(env)
    (card,) = await svc.on_message(adapter, _dm("7001", f"/ekos answer {QID[:8]} {STATEMENT}"))
    assert card.buttons
    (row,) = [p for p in _all(models.Proposal)]
    assert row.answers == QID[:8] and row.text == STATEMENT
    (out,) = await svc.on_message(adapter, _dm("7001", "/ekos answer ../x it is"))
    assert "/ekos answer <question id>" in out.text


async def test_a_reader_cannot_record_testimony(env) -> None:
    svc, adapter = env["svc"], env["svc"].adapters["telegram"]
    _link(env, "7002", Principal(kind="user", id="u2", subject="s-u2", email="bob@x.io"))
    model = Scripted([])
    env["mp"].setattr(providers, "from_settings", lambda s: model)
    (out,) = await svc.on_message(adapter, _dm("7002", f"/ekos tell {STATEMENT}"))
    assert "reviewer role" in out.text and model.tools == []


# ── intake ───────────────────────────────────────────────────────────────────


@FAKE_OS
async def test_a_file_from_a_reader_waits_in_the_inbox(env) -> None:
    svc, adapter = env["svc"], env["svc"].adapters["telegram"]
    _link(env, "7002", Principal(kind="user", id="u2", subject="s-u2", email="bob@x.io"))
    msg = _dm("7002", "", attachments=(Attachment("invoice-spec.pdf", fetch="f1"),))
    (out,) = await svc.on_message(adapter, msg)
    assert "waits for a reviewer" in out.text
    (call,) = [c for c in _calls(env["log"]) if c[:2] == ["inbox", "add"]]
    assert "--as=bob@x.io" in call and "--channel=telegram" in call and "--pending" in call


async def test_nothing_is_taken_in_a_public_deployment(env) -> None:
    svc, adapter = env["svc"], env["svc"].adapters["telegram"]
    _link(env, "7001", _ann())
    env["mp"].setattr(env["settings"], "moderation_enabled", True)
    msg = _dm("7001", "", attachments=(Attachment("x.pdf", fetch="f1"),))
    (out,) = await svc.on_message(adapter, msg)
    assert "does not take material" in out.text


# ── projects ─────────────────────────────────────────────────────────────────


async def test_a_project_is_chosen_for_direct_messages(env) -> None:
    svc, adapter = env["svc"], env["svc"].adapters["telegram"]
    _link(env, "7001", _ann(), default_ws="")
    env["mp"].setattr(providers, "from_settings", lambda s: Scripted([]))
    (out,) = await svc.on_message(adapter, _dm("7001", "what is orders?"))
    assert "Choose a project" in out.text and "w" in out.text
    (out,) = await svc.on_message(adapter, _dm("7001", "/ekos project nope"))
    assert "No project" in out.text
    (out,) = await svc.on_message(adapter, _dm("7001", "/ekos project w"))
    assert "Shop" in out.text
    assert models.get_channel_account("telegram", "7001").default_workspace == "w"


# ── digest ───────────────────────────────────────────────────────────────────


@FAKE_OS
async def test_the_digest_goes_to_linked_people_with_not_me(env) -> None:
    svc, adapter = env["svc"], env["svc"].adapters["telegram"]
    _link(env, "7001", _ann())
    acc = models.get_channel_account("telegram", "7001")
    acc.dm_ref = "7001"
    models.save_channel_account(acc)
    result = await svc.digest()
    assert result == {"sent": 1, "unreachable": ["nobody@x.io"]}
    header, question = adapter.sent
    assert header.conversation == "7001" and "Shop" in header.text
    assert [b.label for b in question.buttons] == ["Answer", "Not me"]
    text = await svc.on_click(
        adapter, ButtonClick("telegram", "7001", "7001", question.buttons[1].ref)
    )
    assert "next person" in text
    assert _calls(env["log"])[-1] == ["questions", "decline", QID, "--as=ann@x.io"]


async def test_no_digest_from_a_public_deployment(env) -> None:
    env["mp"].setattr(env["settings"], "read_only", True)
    result = await env["svc"].digest()
    assert result["sent"] == 0 and "public" in result["skipped"]
    assert env["svc"].adapters["telegram"].sent == []


# ── service and inbound routes ───────────────────────────────────────────────


def test_the_service_starts_only_when_a_channel_is_configured(env, memory_keyring: dict) -> None:
    settings = env["settings"]
    assert service.build(settings, Supervisor()) is None
    env["mp"].setattr(settings, "channels_telegram_token_env", "EKOS_TG_TOKEN")
    assert service.build(settings, Supervisor()) is None  # named, not present
    memory_keyring[("ekos", "env:EKOS_TG_TOKEN")] = "123:abc"
    svc = service.build(settings, Supervisor())
    assert svc is not None and set(svc.adapters) == {"telegram"}
    assert service.configured(settings) == {"telegram": True, "slack": False, "teams": False}
    assert service.digest_trigger("") is None and service.digest_trigger("not cron") is None
    assert service.digest_trigger("0 8 * * 1-5") is not None


def test_owner_binds_a_channel_and_people_see_their_links(env) -> None:
    c = env["client"]
    r = c.post(
        "/api/channels/bindings",
        headers=OWNER,
        json={"channel": "slack", "conversation": "C0FIN", "workspace_id": "w"},
    )
    assert r.status_code == 200
    reader = {"Authorization": "Bearer r"}
    assert (
        c.post(
            "/api/channels/bindings",
            headers=reader,
            json={"channel": "slack", "conversation": "C1", "workspace_id": "w"},
        ).status_code
        == 403
    )
    overview = c.get("/api/channels", headers=OWNER).json()
    assert overview["bindings"][0]["conversation"] == "C0FIN"
    assert overview["configured"] == {"telegram": False, "slack": False, "teams": False}
    assert c.get("/api/channels/me", headers=OWNER).json() == []
    assert c.delete("/api/channels/bindings/slack:C0FIN", headers=OWNER).status_code == 200


def test_slack_events_need_slacks_signature(env) -> None:
    c = env["client"]
    reqs = json.loads(
        (Path(__file__).parent / "fixtures/channels/slack/requests.json").read_text("utf-8")
    )
    adapter = slack.SlackAdapter("xoxb", signing_secret="sec")
    svc = service.ChannelService(env["settings"], Supervisor(), {"slack": adapter})
    c.app.state.channels = svc
    body = json.dumps(reqs["url_verification"]).encode()
    ts = str(int(time.time()))
    headers = {
        "x-slack-request-timestamp": ts,
        "x-slack-signature": slack.signature("sec", ts, body),
        "content-type": "application/json",
    }
    r = c.post("/api/channels/slack/events", content=body, headers=headers)
    assert r.status_code == 200 and r.json()["challenge"].startswith("3eZb")
    bad = {**headers, "x-slack-signature": slack.signature("other", ts, body)}
    assert c.post("/api/channels/slack/events", content=body, headers=bad).status_code == 401
    old = str(int(time.time()) - 3600)
    stale = {**headers, "x-slack-request-timestamp": old,
             "x-slack-signature": slack.signature("sec", old, body)}  # fmt: skip
    assert c.post("/api/channels/slack/events", content=body, headers=stale).status_code == 401
    c.app.state.channels = None
    assert c.post("/api/channels/slack/events", content=body, headers=headers).status_code == 404


def test_teams_needs_the_bot_frameworks_token(env) -> None:
    from app.channels import teams

    c = env["client"]
    adapter = teams.TeamsAdapter("app-0001", "secret")
    c.app.state.channels = service.ChannelService(env["settings"], Supervisor(), {"teams": adapter})
    act = json.loads(
        (Path(__file__).parent / "fixtures/channels/teams/activities.json").read_text("utf-8")
    )["message_personal"]
    r = c.post("/api/channels/teams/messages", json=act)
    assert r.status_code == 401
    r = c.post("/api/channels/teams/messages", json=act, headers={"Authorization": "Bearer x"})
    assert r.status_code == 401
    c.app.state.channels = None


def _mock_client(answer):
    import httpx

    return httpx.AsyncClient(transport=httpx.MockTransport(answer))


def test_a_signed_teams_button_press_is_answered_in_the_response(env) -> None:
    """End to end through the route: the Bot Framework's token checked, the press attributed to
    the account in the signed activity, the answer in the invoke response."""
    import httpx
    from joserfc import jwt
    from joserfc.jwk import RSAKey

    from app.channels import teams

    c = env["client"]
    key = RSAKey.generate_key(2048, parameters={"kid": "k1"})
    fx = Path(__file__).parent / "fixtures/channels/teams"
    resp = json.loads((fx / "responses.json").read_text("utf-8"))
    invoke = json.loads((fx / "activities.json").read_text("utf-8"))["invoke_action"]

    def answer(req: httpx.Request) -> httpx.Response:
        if str(req.url) == teams.OPENID:
            return httpx.Response(200, json=resp["openid"])
        return httpx.Response(200, json={"keys": [key.as_dict(private=False)]})

    adapter = teams.TeamsAdapter("app-0001", "secret", client=_mock_client(answer))
    c.app.state.channels = service.ChannelService(env["settings"], Supervisor(), {"teams": adapter})
    # The card was sent to Bob's account; the press comes from Ann's.
    models.add(
        models.ButtonRef(id="REF", channel="teams", account="aad-bob-0002", workspace_id="w",
                         payload={"action": "discard", "proposal": "p"})
    )  # fmt: skip
    now = int(time.time())
    token = jwt.encode(
        {"alg": "RS256", "kid": "k1"},
        {"iss": teams.ISSUER, "aud": "app-0001", "exp": now + 600, "nbf": now - 5},
        key,
    )
    r = c.post(
        "/api/channels/teams/messages", json=invoke, headers={"Authorization": f"Bearer {token}"}
    )
    assert r.status_code == 200
    assert r.json()["value"] == "this button is for the person it was sent to"
    c.app.state.channels = None


def test_a_signed_slack_message_from_an_unlinked_account_gets_the_instructions(env) -> None:
    import httpx

    c = env["client"]
    sent: list[dict] = []

    def answer(req: httpx.Request) -> httpx.Response:
        sent.append(json.loads(req.content))
        return httpx.Response(200, json={"ok": True, "ts": "1"})

    adapter = slack.SlackAdapter("xoxb", signing_secret="sec", client=_mock_client(answer))
    svc = service.ChannelService(env["settings"], Supervisor(), {"slack": adapter})
    c.app.state.channels = svc
    reqs = json.loads(
        (Path(__file__).parent / "fixtures/channels/slack/requests.json").read_text("utf-8")
    )
    body = json.dumps(reqs["event_dm"]).encode()
    ts = str(int(time.time()))
    headers = {
        "x-slack-request-timestamp": ts,
        "x-slack-signature": slack.signature("sec", ts, body),
    }
    assert c.post("/api/channels/slack/events", content=body, headers=headers).status_code == 200
    c.portal.call(svc.drain)
    assert sent and "not linked" in sent[0]["text"] and sent[0]["channel"] == "D0001"
    c.app.state.channels = None
