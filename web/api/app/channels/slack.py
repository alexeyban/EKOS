"""Slack (RFC 0253 §3.8): Socket Mode (no inbound endpoint) or the Events API, Block Kit buttons.

* **Events API / interactivity**: Slack signs every request; `verify` checks the
  `X-Slack-Signature` (HMAC-SHA256 of `v0:<timestamp>:<body>` with the signing secret) and that
  the timestamp is within five minutes, so a captured request cannot be replayed later.
* **Socket Mode**: `apps.connections.open` with the app-level token gives a WebSocket URL; each
  envelope is acknowledged with its `envelope_id` (`socket_ack`). Its events and button presses
  are the same payloads as over HTTP, so both paths share `parse_event` / `parse_action`.

Who pressed a button is `user.id` in Slack's signed payload. Files are downloaded from
`url_private_download` with the bot token. A private answer in a channel is an ephemeral message.
"""

from __future__ import annotations

import asyncio
import hashlib
import hmac
import json
import logging
import re
import time
from collections.abc import Awaitable, Callable
from typing import Any
from urllib.parse import parse_qs

import httpx

from .core import Attachment, ButtonClick, ChannelMessage, OutMessage, chunks

API = "https://slack.com/api"
MAX_TEXT = 3000
REPLAY_WINDOW = 300
log = logging.getLogger("ekos.channels.slack")


class SlackError(RuntimeError):
    pass


def signature(secret: str, timestamp: str, body: bytes) -> str:
    base = b"v0:" + timestamp.encode() + b":" + body
    return "v0=" + hmac.new(secret.encode(), base, hashlib.sha256).hexdigest()


def verify(
    secret: str, timestamp: str, body: bytes, given: str, *, now: float | None = None
) -> bool:
    """Whether a request is Slack's: a fresh timestamp and a matching signature."""
    if not secret or not timestamp or not given:
        return False
    try:
        ts = int(timestamp)
    except ValueError:
        return False
    if abs((now if now is not None else time.time()) - ts) > REPLAY_WINDOW:
        return False
    return hmac.compare_digest(signature(secret, timestamp, body), given)


def _files(event: dict) -> tuple[Attachment, ...]:
    return tuple(
        Attachment(
            name=str(f.get("name") or "file"),
            media_type=str(f.get("mimetype") or ""),
            size=int(f.get("size") or 0),
            fetch=str(f.get("url_private_download") or ""),
        )
        for f in event.get("files") or []
        if isinstance(f, dict) and f.get("url_private_download")
    )


def _text(event: dict, bot_user: str) -> str:
    text = str(event.get("text") or "")
    if bot_user:
        text = text.replace(f"<@{bot_user}>", "")
    elif event.get("type") == "app_mention":
        # Over the Events API alone the bot's own id is not known; a mention starts the text.
        text = re.sub(r"^\s*<@[A-Z0-9]+>", "", text)
    return text.strip()


def parse_event(payload: dict, *, bot_user: str = "") -> ChannelMessage | None:
    """An `event_callback` → a message for the core, or `None` (a bot, an edit, not addressed)."""
    if payload.get("type") != "event_callback":
        return None
    event = payload.get("event") or {}
    if event.get("bot_id") or event.get("subtype") not in (None, "file_share"):
        return None
    kind = event.get("type")
    direct = event.get("channel_type") == "im"
    # In a channel only an @mention (`app_mention`) is addressed to the bot; a plain message
    # there is someone else's conversation.
    if kind == "message" and not direct:
        return None
    if kind not in ("message", "app_mention"):
        return None
    channel_id = str(event.get("channel") or "")
    ts = str(event.get("ts") or "")
    return ChannelMessage(
        channel="slack",
        account=str(event.get("user") or ""),
        conversation=channel_id,
        message_ref=f"{channel_id}/{ts}",
        text=_text(event, bot_user),
        direct=direct,
        attachments=_files(event),
        thread=str(event.get("thread_ts") or ts),
        sent_at=ts,
        extra={"team": str(payload.get("team_id") or "")},
    )


def parse_action(payload: dict) -> ButtonClick | None:
    """A `block_actions` interaction → a button press, or `None`."""
    if payload.get("type") != "block_actions":
        return None
    actions = payload.get("actions") or []
    if not actions:
        return None
    value = str(actions[0].get("value") or "")
    if not value.startswith("b:"):
        return None
    return ButtonClick(
        channel="slack",
        account=str((payload.get("user") or {}).get("id") or ""),
        conversation=str((payload.get("channel") or {}).get("id") or ""),
        ref=value[2:],
        message_ref=str((payload.get("container") or {}).get("message_ts") or ""),
        extra={"response_url": str(payload.get("response_url") or "")},
    )


def form_payload(body: bytes) -> dict:
    """Interactivity posts `payload=<json>` form-encoded."""
    raw = parse_qs(body.decode("utf-8", "replace")).get("payload", [""])[0]
    try:
        value = json.loads(raw)
    except json.JSONDecodeError:
        return {}
    return value if isinstance(value, dict) else {}


def blocks(out: OutMessage, text: str) -> list[dict]:
    body: list[dict] = [{"type": "section", "text": {"type": "mrkdwn", "text": text}}]
    if out.buttons:
        body.append(
            {
                "type": "actions",
                "elements": [
                    {
                        "type": "button",
                        "text": {"type": "plain_text", "text": b.label},
                        "action_id": f"ekos-{n}",
                        "value": f"b:{b.ref}",
                    }
                    for n, b in enumerate(out.buttons)
                ],
            }
        )
    return body


def render(out: OutMessage) -> list[tuple[str, dict[str, Any]]]:
    """(method, body) pairs for one message: `chat.postEphemeral` when private in a channel,
    `chat.postMessage` otherwise (to a person's id it opens their direct message)."""
    pieces = chunks(out.text, MAX_TEXT)
    calls: list[tuple[str, dict[str, Any]]] = []
    for n, piece in enumerate(pieces):
        body: dict[str, Any] = {"channel": out.conversation, "text": piece}
        if n == len(pieces) - 1:
            body["blocks"] = blocks(out, piece)
        if out.thread:
            body["thread_ts"] = out.thread
        if out.private_to:
            body["user"] = out.private_to
            calls.append(("chat.postEphemeral", body))
        else:
            calls.append(("chat.postMessage", body))
    return calls


def socket_ack(frame: str) -> tuple[dict | None, str, dict]:
    """A Socket Mode frame → (the acknowledgement to send back, its type, its payload)."""
    try:
        env = json.loads(frame)
    except json.JSONDecodeError:
        return None, "", {}
    kind = str(env.get("type") or "")
    ack = {"envelope_id": env["envelope_id"]} if env.get("envelope_id") else None
    return ack, kind, env.get("payload") or {}


class SlackAdapter:
    name = "slack"

    def __init__(
        self,
        bot_token: str,
        *,
        signing_secret: str = "",
        app_token: str = "",
        client: httpx.AsyncClient | None = None,
    ) -> None:
        self._bot = bot_token
        self.signing_secret = signing_secret
        self._app = app_token
        self._client = client or httpx.AsyncClient(timeout=30.0)
        self.bot_user = ""

    async def _call(self, method: str, body: dict, *, token: str | None = None) -> dict:
        try:
            r = await self._client.post(
                f"{API}/{method}",
                json=body,
                headers={"Authorization": f"Bearer {token or self._bot}"},
            )
        except httpx.HTTPError as exc:
            raise SlackError(f"{method}: {type(exc).__name__}") from None
        data = r.json() if r.content else {}
        if not data.get("ok"):
            raise SlackError(f"{method}: {data.get('error') or r.status_code}")
        return data

    async def start(self) -> None:
        me = await self._call("auth.test", {})
        self.bot_user = str(me.get("user_id") or "")

    async def send(self, out: OutMessage) -> str:
        ref = ""
        for method, body in render(out):
            data = await self._call(method, body)
            ref = str(data.get("ts") or data.get("message_ts") or "")
        return ref

    async def answer_click(self, click: ButtonClick, text: str) -> None:
        await self.send(
            OutMessage(conversation=click.conversation, text=text, private_to=click.account)
        )

    async def download(self, att: Attachment) -> bytes:
        if not att.fetch.startswith("https://files.slack.com/"):
            raise SlackError("a file is downloaded from files.slack.com only")
        try:
            r = await self._client.get(att.fetch, headers={"Authorization": f"Bearer {self._bot}"})
        except httpx.HTTPError as exc:
            raise SlackError(f"download: {type(exc).__name__}") from None
        if r.status_code != 200 or r.headers.get("content-type", "").startswith("text/html"):
            # Without the files:read scope Slack answers with its sign-in page.
            raise SlackError(f"download: {r.status_code} (does the app have files:read?)")
        return r.content

    async def members(self, msg: ChannelMessage) -> list[str] | None:
        if msg.direct:
            return [msg.account]
        out: list[str] = []
        cursor = ""
        for _ in range(20):
            body = {"channel": msg.conversation, "limit": 200}
            if cursor:
                body["cursor"] = cursor
            data = await self._call("conversations.members", body)
            out += [str(m) for m in data.get("members") or [] if str(m) != self.bot_user]
            cursor = str((data.get("response_metadata") or {}).get("next_cursor") or "")
            if not cursor:
                return out
        return None  # too large to know: treated as unknown

    async def run(
        self,
        handle: Callable[[Any, Any], Awaitable[None]],
        stop: asyncio.Event,
        connect: Callable[[str], Any] | None = None,
    ) -> None:
        """Socket Mode until `stop`: open a connection, acknowledge each envelope at once, then
        hand its event or button press to `handle`."""
        if connect is None:
            import websockets

            connect = websockets.connect
        backoff = 1.0
        while not stop.is_set():
            try:
                if not self.bot_user:
                    await self.start()
                url = (await self._call("apps.connections.open", {}, token=self._app))["url"]
                async with connect(url) as ws:
                    backoff = 1.0
                    async for frame in ws:
                        if await self._on_frame(ws, frame, handle):
                            break
            except asyncio.CancelledError:
                raise
            except Exception as exc:
                log.warning("slack socket mode: %s", type(exc).__name__)
                await asyncio.sleep(backoff)
                backoff = min(backoff * 2, 60.0)

    async def _on_frame(self, ws, frame, handle) -> bool:
        """Handle one frame; True when Slack asks for a reconnect."""
        ack, kind, payload = socket_ack(frame if isinstance(frame, str) else frame.decode())
        if ack is not None:
            await ws.send(json.dumps(ack))
        if kind == "disconnect":
            return True
        event = (
            parse_event(payload, bot_user=self.bot_user)
            if kind == "events_api"
            else parse_action(payload)
            if kind == "interactive"
            else None
        )
        if event is not None:
            await handle(self, event)
        return False

    async def aclose(self) -> None:
        await self._client.aclose()
