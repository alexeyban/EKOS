"""Telegram (RFC 0253 §3.8): the Bot API over long polling, so no inbound endpoint is needed.

Transport only: `parse_update` turns an update into a `ChannelMessage` or a `ButtonClick`,
`send` posts a message with an inline keyboard, `download` fetches a file with `getFile`. Who the
sender is comes from the update itself, which only the Bot API can hand to this token's
`getUpdates` — the token is the credential, and it is never logged (the Bot API puts it in the
URL path, so request URLs are never logged either).

In a group the bot answers only what is addressed to it: a `/ekos` command, a mention, or a reply
to one of its own messages. Telegram cannot list a group's members, so the core treats a group's
common scope as unknown and answers there privately whenever an answer read the ledger.
"""

from __future__ import annotations

import asyncio
import logging
from typing import Any

import httpx

from .core import Attachment, ButtonClick, ChannelMessage, OutMessage, chunks

API = "https://api.telegram.org"
MAX_TEXT = 4000
log = logging.getLogger("ekos.channels.telegram")


class TelegramError(RuntimeError):
    pass


def _addressed(message: dict, bot_username: str, bot_id: int | None) -> bool:
    chat = message.get("chat") or {}
    if chat.get("type") == "private":
        return True
    text = str(message.get("text") or message.get("caption") or "")
    if text.lstrip().startswith("/ekos"):
        return True
    if bot_username and f"@{bot_username}".lower() in text.lower():
        return True
    replied = (message.get("reply_to_message") or {}).get("from") or {}
    return bot_id is not None and replied.get("id") == bot_id


def _attachments(message: dict) -> tuple[Attachment, ...]:
    out = []
    doc = message.get("document")
    if isinstance(doc, dict) and doc.get("file_id"):
        out.append(
            Attachment(
                name=str(doc.get("file_name") or "file"),
                media_type=str(doc.get("mime_type") or ""),
                size=int(doc.get("file_size") or 0),
                fetch=str(doc["file_id"]),
            )
        )
    photos = message.get("photo")
    if isinstance(photos, list) and photos:
        best = max(photos, key=lambda p: int(p.get("file_size") or 0))
        out.append(
            Attachment(
                name=f"photo-{message.get('message_id', 'x')}.jpg",
                media_type="image/jpeg",
                size=int(best.get("file_size") or 0),
                fetch=str(best.get("file_id")),
            )
        )
    return tuple(out)


def _strip_mention(text: str, bot_username: str) -> str:
    if bot_username:
        text = text.replace(f"@{bot_username}", "").replace(f"@{bot_username.lower()}", "")
    return text.strip()


def _parse_click(cb: dict) -> ButtonClick | None:
    """A button press (`callback_query`) on one of the bot's messages."""
    sender = cb.get("from") or {}
    msg = cb.get("message") or {}
    data = str(cb.get("data") or "")
    if sender.get("is_bot") or not data.startswith("b:"):
        return None
    return ButtonClick(
        channel="telegram",
        account=str(sender.get("id")),
        conversation=str((msg.get("chat") or {}).get("id", "")),
        ref=data[2:],
        message_ref=str(msg.get("message_id", "")),
        extra={"callback_query_id": str(cb.get("id", ""))},
    )


def parse_update(
    update: dict, *, bot_username: str = "", bot_id: int | None = None
) -> ChannelMessage | ButtonClick | None:
    """One `getUpdates` result → what the core handles, or `None` (not for the bot, or a bot)."""
    cb = update.get("callback_query")
    if isinstance(cb, dict):
        return _parse_click(cb)
    message = update.get("message")
    if not isinstance(message, dict):
        return None
    sender = message.get("from") or {}
    if sender.get("is_bot") or not _addressed(message, bot_username, bot_id):
        return None
    chat = message.get("chat") or {}
    text = _strip_mention(str(message.get("text") or message.get("caption") or ""), bot_username)
    return ChannelMessage(
        channel="telegram",
        account=str(sender.get("id")),
        conversation=str(chat.get("id")),
        message_ref=str(message.get("message_id", "")),
        text=text,
        direct=chat.get("type") == "private",
        attachments=_attachments(message),
        reply_to=str((message.get("reply_to_message") or {}).get("message_id", "") or ""),
        sent_at=str(message.get("date", "")),
    )


def render(out: OutMessage) -> list[dict[str, Any]]:
    """`sendMessage` bodies for one message; the buttons go on the last piece. A private reply
    in a group goes to the person's own chat with the bot (in Telegram a user id is that chat)."""
    chat_id = out.private_to or out.conversation
    pieces = chunks(out.text, MAX_TEXT)
    bodies: list[dict[str, Any]] = [{"chat_id": chat_id, "text": p} for p in pieces]
    if out.thread and not out.private_to:
        bodies[0]["reply_to_message_id"] = int(out.thread) if out.thread.isdigit() else out.thread
    if out.buttons:
        bodies[-1]["reply_markup"] = {
            "inline_keyboard": [
                [{"text": b.label, "callback_data": f"b:{b.ref}"} for b in out.buttons]
            ]
        }
    return bodies


class TelegramAdapter:
    name = "telegram"

    def __init__(self, token: str, client: httpx.AsyncClient | None = None) -> None:
        self._token = token
        self._client = client or httpx.AsyncClient(timeout=60.0)
        self.bot_username = ""
        self.bot_id: int | None = None
        self._offset = 0

    async def _call(self, method: str, body: dict | None = None) -> Any:
        try:
            r = await self._client.post(f"{API}/bot{self._token}/{method}", json=body or {})
        except httpx.HTTPError as exc:
            # The exception text holds the URL, and the URL holds the token.
            raise TelegramError(f"{method}: {type(exc).__name__}") from None
        data = r.json() if r.content else {}
        if not data.get("ok"):
            raise TelegramError(f"{method}: {data.get('description') or r.status_code}")
        return data.get("result")

    async def start(self) -> None:
        me = await self._call("getMe")
        self.bot_username = str(me.get("username") or "")
        self.bot_id = me.get("id")

    async def poll(self, wait: int = 30) -> list[ChannelMessage | ButtonClick]:
        """One long poll: what arrived, already parsed; the offset moves past every update."""
        updates = await self._call(
            "getUpdates",
            {
                "offset": self._offset,
                "timeout": wait,
                "allowed_updates": ["message", "callback_query"],
            },
        )
        out = []
        for u in updates or []:
            self._offset = max(self._offset, int(u.get("update_id", 0)) + 1)
            parsed = parse_update(u, bot_username=self.bot_username, bot_id=self.bot_id)
            if parsed is not None:
                out.append(parsed)
        return out

    async def send(self, out: OutMessage) -> str:
        ref = ""
        for body in render(out):
            result = await self._call("sendMessage", body)
            ref = str((result or {}).get("message_id", ""))
        return ref

    async def answer_click(self, click: ButtonClick, text: str) -> None:
        await self._call(
            "answerCallbackQuery",
            {"callback_query_id": click.extra.get("callback_query_id", ""), "text": text[:190]},
        )

    async def download(self, att: Attachment) -> bytes:
        info = await self._call("getFile", {"file_id": att.fetch})
        path = str((info or {}).get("file_path") or "")
        if not path:
            raise TelegramError("getFile: no path")
        try:
            r = await self._client.get(f"{API}/file/bot{self._token}/{path}")
        except httpx.HTTPError as exc:
            raise TelegramError(f"download: {type(exc).__name__}") from None
        if r.status_code != 200:
            raise TelegramError(f"download: {r.status_code}")
        return r.content

    # The adapter protocol awaits `members`; this adapter has nothing to await.
    async def members(self, msg: ChannelMessage) -> list[str] | None:  # NOSONAR
        return None  # the Bot API cannot list a group's members

    async def run(self, handle, stop: asyncio.Event) -> None:
        """Poll until `stop` is set; each event goes to `handle`. A failure waits and retries."""
        backoff = 1.0
        while not stop.is_set():
            try:
                if self.bot_id is None:
                    await self.start()
                for event in await self.poll():
                    await handle(self, event)
                backoff = 1.0
            except asyncio.CancelledError:
                raise
            except Exception as exc:
                log.warning("telegram poll failed: %s", exc)
                await asyncio.sleep(backoff)
                backoff = min(backoff * 2, 60.0)

    async def aclose(self) -> None:
        await self._client.aclose()
