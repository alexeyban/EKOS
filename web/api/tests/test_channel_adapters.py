"""RFC 0253 §3.8: the three channel adapters against recorded request/response shapes.

No network: every HTTP call goes to an `httpx.MockTransport` that answers from the fixtures in
`tests/fixtures/channels/` and records what was sent. The fixtures follow the platforms'
documented payloads (Telegram Bot API, Slack Events API / Block Kit / Socket Mode, Bot Framework
activities and Adaptive Cards).
"""

from __future__ import annotations

import json
import time
from pathlib import Path

import httpx
import pytest

from app.channels import slack, teams, telegram
from app.channels.core import (
    UNLINKED,
    Attachment,
    Button,
    ButtonClick,
    ChannelMessage,
    OutMessage,
    answer_privately,
    chunks,
    command,
)

FIX = Path(__file__).parent / "fixtures" / "channels"


def _fx(*parts: str):
    return json.loads((FIX.joinpath(*parts)).read_text(encoding="utf-8"))


class Recorder:
    """A transport that answers by URL and keeps every request."""

    def __init__(self, answer) -> None:
        self.requests: list[httpx.Request] = []
        self._answer = answer

    def __call__(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        return self._answer(request)

    def client(self) -> httpx.AsyncClient:
        return httpx.AsyncClient(transport=httpx.MockTransport(self))

    def bodies(self) -> list[dict]:
        return [json.loads(r.content) for r in self.requests if r.content]


# ── core ─────────────────────────────────────────────────────────────────────


def test_commands_and_chunks() -> None:
    assert command("/ekos link") == ("link", "")
    assert command("/ekos@ekos_test_bot tell status 7 is cancelled") == (
        "tell",
        "status 7 is cancelled",
    )
    assert command("/ekos") == ("help", "")
    assert command("/ekos bogus x") == ("help", "x")
    assert command("what is orders?") is None
    # Cut at line ends; a line longer than the limit is cut inside.
    assert chunks("a\nbb\nccc\n", 4) == ["a\n", "bb\n", "ccc\n"]
    assert chunks("x" * 10, 3) == ["xxx", "xxx", "xxx", "x"]
    assert chunks("", 3) == [""]


def test_a_group_answer_goes_privately_unless_every_member_sees_it_all() -> None:
    # A direct message, or an answer that read nothing, is never a leak.
    assert not answer_privately(direct=True, used_knowledge=True, member_scopes=None)
    assert not answer_privately(direct=False, used_knowledge=False, member_scopes=None)
    # Every member is a linked person who sees the whole project: the thread.
    assert not answer_privately(direct=False, used_knowledge=True, member_scopes=[None, None])
    # One restricted member, one unlinked one, or members unknown: privately.
    restricted = {"default": "deny", "rules": []}
    assert answer_privately(direct=False, used_knowledge=True, member_scopes=[None, restricted])
    assert answer_privately(direct=False, used_knowledge=True, member_scopes=[None, UNLINKED])
    assert answer_privately(direct=False, used_knowledge=True, member_scopes=None)


# ── Telegram ─────────────────────────────────────────────────────────────────


def test_telegram_takes_only_what_is_addressed_to_the_bot() -> None:
    updates = _fx("telegram", "getUpdates.json")["result"]
    parsed = [telegram.parse_update(u, bot_username="ekos_test_bot", bot_id=9000) for u in updates]
    dm, group_chatter, mention, document, click, other_bot = parsed
    assert isinstance(dm, ChannelMessage) and dm.direct and dm.account == "7001"
    assert dm.text == "what is the orders table?"
    assert group_chatter is None  # not addressed to the bot
    assert other_bot is None  # bots are ignored
    assert isinstance(mention, ChannelMessage) and not mention.direct
    assert mention.text == "what does status 7 mean?" and mention.conversation == "-100555"
    assert isinstance(document, ChannelMessage)
    assert document.attachments == (
        Attachment("invoice-spec.pdf", "application/pdf", 5, "BQACAgIAAxkBAAIBc2Zk"),
    )
    assert isinstance(click, ButtonClick) and click.ref == "REF" and click.account == "7001"
    assert click.extra["callback_query_id"] == "4382bfdwdsb323b2d9"


def test_telegram_renders_an_inline_keyboard_and_private_replies_go_to_the_person() -> None:
    out = OutMessage("-100555", "card", [Button("Save", "r1"), Button("Discard", "r2")], "7001")
    (body,) = telegram.render(out)
    assert body["chat_id"] == "7001"  # a private reply in a group: the person's own chat
    assert body["reply_markup"] == {
        "inline_keyboard": [
            [
                {"text": "Save", "callback_data": "b:r1"},
                {"text": "Discard", "callback_data": "b:r2"},
            ]
        ]
    }
    long = telegram.render(OutMessage("1", "x" * 9000, [Button("Save", "r")]))
    assert len(long) == 3 and "reply_markup" in long[-1] and "reply_markup" not in long[0]


async def test_telegram_polls_downloads_and_never_shows_its_token() -> None:
    resp = _fx("telegram", "responses.json")
    updates = _fx("telegram", "getUpdates.json")

    def answer(req: httpx.Request) -> httpx.Response:
        path = req.url.path
        if path.endswith("/getMe"):
            return httpx.Response(200, json=resp["getMe"])
        if path.endswith("/getUpdates"):
            return httpx.Response(200, json=updates)
        if path.endswith("/getFile"):
            return httpx.Response(200, json=resp["getFile"])
        if "/file/bot" in path:
            return httpx.Response(200, content=b"hello")
        if path.endswith("/sendMessage"):
            return httpx.Response(200, json=resp["sendMessage"])
        return httpx.Response(401, json=resp["unauthorized"])

    rec = Recorder(answer)
    a = telegram.TelegramAdapter("123:SECRET-TOKEN", client=rec.client())
    await a.start()
    assert a.bot_username == "ekos_test_bot"
    events = await a.poll()
    assert len(events) == 4  # chatter and the other bot left out
    assert rec.bodies()[-1]["offset"] == 0
    await a.poll()
    assert rec.bodies()[-1]["offset"] == 507  # past every update seen
    data = await a.download(Attachment("x.pdf", fetch="BQACAgIAAxkBAAIBc2Zk"))
    assert data == b"hello"
    assert rec.requests[-1].url.path == "/file/bot123:SECRET-TOKEN/documents/file_3.pdf"
    assert await a.send(OutMessage("7001", "hi")) == "21"
    with pytest.raises(telegram.TelegramError) as e:
        await a._call("deleteWebhook")
    assert "SECRET-TOKEN" not in str(e.value)

    def broken(req: httpx.Request) -> httpx.Response:
        raise httpx.ConnectError(f"cannot reach {req.url}")

    b = telegram.TelegramAdapter("123:SECRET-TOKEN", client=Recorder(broken).client())
    with pytest.raises(telegram.TelegramError) as e:
        await b.start()
    assert "SECRET-TOKEN" not in str(e.value)


# ── Slack ────────────────────────────────────────────────────────────────────


def test_slack_signature_is_checked_and_a_replay_refused() -> None:
    s = _fx("slack", "requests.json")["signed"]
    body, ts, secret, sig = s["body"].encode(), s["timestamp"], s["secret"], s["signature"]
    now = int(ts) + 10
    assert slack.verify(secret, ts, body, sig, now=now)
    assert not slack.verify(secret, ts, body + b" ", sig, now=now)  # body changed
    assert not slack.verify("another-secret", ts, body, sig, now=now)
    assert not slack.verify(secret, ts, body, sig, now=int(ts) + 301)  # too old: a replay
    assert not slack.verify(secret, "", body, sig, now=now)
    assert not slack.verify("", ts, body, sig, now=now)


def test_slack_events_and_button_presses() -> None:
    r = _fx("slack", "requests.json")
    dm = slack.parse_event(r["event_dm"], bot_user="U0BOT")
    assert isinstance(dm, ChannelMessage) and dm.direct and dm.account == "U0ANN"
    assert dm.message_ref == "D0001/1762790400.000100"
    mention = slack.parse_event(r["event_mention"], bot_user="U0BOT")
    assert mention is not None and not mention.direct
    assert mention.text == "what does status 7 mean?"
    # Over the Events API alone the bot's id is unknown: the leading mention still goes.
    unknown_bot = slack.parse_event(r["event_mention"])
    assert unknown_bot is not None and unknown_bot.text == "what does status 7 mean?"
    assert slack.parse_event(r["event_channel_plain"], bot_user="U0BOT") is None
    assert slack.parse_event(r["event_bot"], bot_user="U0BOT") is None
    file = slack.parse_event(r["event_file"], bot_user="U0BOT")
    assert file is not None and file.attachments[0].name == "invoice-spec.pdf"
    form = ("payload=" + httpx.QueryParams({"x": json.dumps(r["block_actions"])})["x"]).encode()
    click = slack.parse_action(slack.form_payload(form.replace(b" ", b"+")))
    assert isinstance(click, ButtonClick) and click.ref == "REF" and click.account == "U0ANN"


def test_slack_renders_block_kit_and_ephemeral_messages() -> None:
    out = OutMessage("C0FIN", "card", [Button("Save", "r1")], private_to="U0ANN", thread="1.2")
    ((method, body),) = slack.render(out)
    assert method == "chat.postEphemeral" and body["user"] == "U0ANN"
    assert body["thread_ts"] == "1.2"
    assert body["blocks"][1]["elements"][0]["value"] == "b:r1"
    ((method, _),) = slack.render(OutMessage("C0FIN", "answer"))
    assert method == "chat.postMessage"


async def test_slack_calls_download_and_members() -> None:
    resp = _fx("slack", "responses.json")

    def answer(req: httpx.Request) -> httpx.Response:
        if req.url.host == "files.slack.com":
            assert req.headers["authorization"] == "Bearer xoxb-bot"
            return httpx.Response(
                200, content=b"hello", headers={"content-type": "application/pdf"}
            )
        method = req.url.path.rsplit("/", 1)[-1]
        return httpx.Response(200, json=resp.get(method, resp["not_authed"]))

    rec = Recorder(answer)
    a = slack.SlackAdapter("xoxb-bot", signing_secret="s", client=rec.client())
    await a.start()
    assert a.bot_user == "U0BOT"
    msg = ChannelMessage("slack", "U0ANN", "C0FIN", "C0FIN/1", direct=False)
    assert await a.members(msg) == ["U0ANN", "U0BOB"]  # the bot is not a member who reads
    url = "https://files.slack.com/files-pri/T0001-F0001/download/invoice-spec.pdf"
    assert await a.download(Attachment("invoice-spec.pdf", fetch=url)) == b"hello"
    with pytest.raises(slack.SlackError):
        await a.download(Attachment("x", fetch="https://evil.example/x"))
    assert await a.send(OutMessage("D0001", "hi")) == "1762790410.000600"


async def test_slack_socket_mode_acknowledges_each_envelope() -> None:
    frames = _fx("slack", "requests.json")["socket_frames"]
    sent: list[str] = []
    handled: list[object] = []

    class Conn:
        def __init__(self, url: str) -> None:
            assert url.startswith("wss://")

        async def __aenter__(self):
            return self

        async def __aexit__(self, *exc):
            return False

        def __aiter__(self):
            return self._frames()

        async def _frames(self):
            for f in frames:
                yield f

        async def send(self, text: str) -> None:
            sent.append(text)

    resp = _fx("slack", "responses.json")

    def answer(req: httpx.Request) -> httpx.Response:
        method = req.url.path.rsplit("/", 1)[-1]
        if method == "apps.connections.open":
            assert req.headers["authorization"] == "Bearer xapp-app"
        return httpx.Response(200, json=resp[method])

    import asyncio

    stop = asyncio.Event()

    async def handle(adapter, event) -> None:
        handled.append(event)
        if len(handled) == 2:
            stop.set()

    a = slack.SlackAdapter("xoxb-bot", app_token="xapp-app", client=Recorder(answer).client())
    await asyncio.wait_for(a.run(handle, stop, connect=Conn), timeout=5)
    assert [json.loads(s) for s in sent] == [{"envelope_id": "e1"}, {"envelope_id": "e2"}]
    assert isinstance(handled[0], ChannelMessage) and isinstance(handled[1], ButtonClick)


# ── Teams ────────────────────────────────────────────────────────────────────


def test_teams_activities() -> None:
    acts = _fx("teams", "activities.json")
    dm = teams.parse_activity(acts["message_personal"])
    assert isinstance(dm, ChannelMessage) and dm.direct and dm.account == "aad-ann-0001"
    assert dm.extra["service_url"] == "https://smba.trafficmanager.net/emea/"
    ch = teams.parse_activity(acts["message_channel"])
    assert isinstance(ch, ChannelMessage) and not ch.direct
    assert ch.text == "what does status 7 mean?" and ch.thread == "1762790400002"
    f = teams.parse_activity(acts["message_file"])
    assert f is not None and f.attachments[0].fetch.endswith("token=abc")
    click = teams.parse_activity(acts["invoke_action"])
    assert isinstance(click, ButtonClick) and click.ref == "REF"
    assert click.account == "aad-ann-0001"
    card = teams.render(OutMessage("c", "card", [Button("Save", "r1")]))[0]["attachments"][0]
    assert card["content"]["actions"][0] == {
        "type": "Action.Execute", "title": "Save", "verb": "ekos", "data": {"ekos": "r1"},
    }  # fmt: skip
    assert teams.invoke_response("Saved.")["value"] == "Saved."


def _signed(key, claims: dict) -> str:
    from joserfc import jwt

    return jwt.encode({"alg": "RS256", "kid": "k1"}, claims, key)


@pytest.fixture
def bot_framework():
    """The Bot Framework's published keys (a key made here) and its token endpoint."""
    from joserfc.jwk import RSAKey

    key = RSAKey.generate_key(2048, parameters={"kid": "k1"})
    resp = _fx("teams", "responses.json")
    jwks = {"keys": [key.as_dict(private=False)]}

    def answer(req: httpx.Request) -> httpx.Response:
        url = str(req.url)
        if url == teams.OPENID:
            return httpx.Response(200, json=resp["openid"])
        if url == resp["openid"]["jwks_uri"]:
            return httpx.Response(200, json=jwks)
        if url == teams.TOKEN_URL:
            form = dict(httpx.QueryParams(req.content.decode()))
            assert form["client_secret"] == "app-secret" and form["scope"] == teams.SCOPE
            return httpx.Response(200, json=resp["token"])
        if url.endswith("/v3/conversations"):
            return httpx.Response(200, json=resp["create_conversation"])
        if url.endswith("/members"):
            return httpx.Response(200, json=resp["members"])
        if "/activities" in url:
            return httpx.Response(200, json=resp["send"])
        return httpx.Response(404)

    rec = Recorder(answer)
    return key, rec, teams.TeamsAdapter("app-0001", "app-secret", client=rec.client())


async def test_teams_checks_the_bot_framework_token(bot_framework) -> None:
    key, _, a = bot_framework
    act = _fx("teams", "activities.json")["message_personal"]
    now = int(time.time())
    good = {
        "iss": teams.ISSUER, "aud": "app-0001", "exp": now + 600, "nbf": now - 10,
        "serviceurl": act["serviceUrl"],
    }  # fmt: skip
    assert (await a.verify_token("Bearer " + _signed(key, good), act))["aud"] == "app-0001"
    from joserfc.jwk import RSAKey

    forger = RSAKey.generate_key(2048, parameters={"kid": "k1"})
    for bad_auth in (
        "",
        "Bearer not-a-jwt",
        "Bearer " + _signed(forger, good),  # signed by someone else
        "Bearer " + _signed(key, {**good, "aud": "another-bot"}),
        "Bearer " + _signed(key, {**good, "iss": "https://evil.example"}),
        "Bearer " + _signed(key, {**good, "exp": now - 3600}),
        "Bearer " + _signed(key, {**good, "serviceurl": "https://evil.example/"}),
    ):
        with pytest.raises(teams.TeamsAuthError):
            await a.verify_token(bad_auth, act)


async def test_teams_replies_and_opens_a_personal_chat_for_a_private_answer(bot_framework) -> None:
    _, rec, a = bot_framework
    ch = teams.parse_activity(_fx("teams", "activities.json")["message_channel"])
    assert isinstance(ch, ChannelMessage)
    await a.send(OutMessage(ch.conversation, "answer", thread=ch.thread, extra=ch.extra))
    last = rec.requests[-1]
    assert str(last.url).endswith(f"/v3/conversations/{ch.conversation}/activities/{ch.thread}")
    assert last.headers["authorization"].startswith("Bearer eyJ")
    await a.send(
        OutMessage(
            ch.conversation, "private", private_to=ch.account, thread=ch.thread, extra=ch.extra
        )
    )
    create, post = rec.requests[-2], rec.requests[-1]
    assert str(create.url).endswith("/v3/conversations")
    assert json.loads(create.content)["members"] == [{"id": "29:1ann-bot-framework-id"}]
    assert str(post.url).endswith("/v3/conversations/a:1personal-ann/activities")
    assert await a.members(ch) == ["aad-ann-0001", "aad-bob-0002"]
