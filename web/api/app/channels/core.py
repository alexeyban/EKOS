"""The channel-agnostic conversation core (RFC 0253 §3.1): what every adapter turns its events
into, what it is asked to send, and the rules that are the same in every channel.

An adapter only translates transport (receive events, download files, post replies, render
buttons). It never decides who may do what, what a model sees, or where an answer goes — that is
here and in `service.py`, once for every channel.

Nothing in this module calls a model or writes anything.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

CHANNELS = ("telegram", "slack", "teams")


@dataclass(frozen=True)
class Attachment:
    """A file sent to the bot. `fetch` is the adapter's own handle (a Telegram file id, a Slack
    private download URL, a Teams content URL); only the adapter that made it reads it."""

    name: str
    media_type: str = ""
    size: int = 0
    fetch: str = ""


@dataclass(frozen=True)
class ChannelMessage:
    channel: str
    # The sender's account in the channel (Slack user id, Teams AAD object id or Bot Framework
    # id, Telegram user id).
    account: str
    # Where it was said: a chat, channel or conversation id.
    conversation: str
    message_ref: str
    text: str = ""
    # True for a one-to-one chat with the bot.
    direct: bool = True
    attachments: tuple[Attachment, ...] = ()
    permalink: str = ""
    reply_to: str = ""
    # Slack thread / Teams reply chain to answer in; empty: the conversation itself.
    thread: str = ""
    sent_at: str = ""
    # Adapter-specific routing a reply needs (Teams `serviceUrl`, tenant).
    extra: dict[str, str] = field(default_factory=dict)


@dataclass(frozen=True)
class ButtonClick:
    """A press of a button the console sent. `ref` names a `ButtonRef` row; `account` is the
    channel's own, signed statement of who pressed it."""

    channel: str
    account: str
    conversation: str
    ref: str
    message_ref: str = ""
    # Teams answers an `invoke` in its HTTP response; others reply with a message.
    extra: dict[str, str] = field(default_factory=dict)


@dataclass(frozen=True)
class Button:
    label: str
    ref: str  # a `ButtonRef` id


@dataclass
class OutMessage:
    """One message to send. `private_to` set: only that account sees it (a direct message in
    Telegram, an ephemeral message in Slack, a personal chat in Teams)."""

    conversation: str
    text: str
    buttons: list[Button] = field(default_factory=list)
    private_to: str = ""
    thread: str = ""
    extra: dict[str, str] = field(default_factory=dict)


# ── commands ─────────────────────────────────────────────────────────────────

_COMMAND = re.compile(r"^\s*/ekos(?:@\w+)?\s*(\w+)?\s*(.*)$", re.S)
COMMANDS = ("link", "project", "tell", "answer", "edit", "withdraw", "help")


def command(text: str) -> tuple[str, str] | None:
    """`/ekos <verb> <rest>` → (verb, rest); a bare `/ekos` is `help`. `None` for anything else
    (a question for the model)."""
    m = _COMMAND.match(text or "")
    if not m:
        return None
    verb = (m.group(1) or "help").lower()
    return (verb if verb in COMMANDS else "help"), m.group(2).strip()


HELP = (
    "Ask me anything about the project. Also:\n"
    "/ekos tell <what you know> — I show what I understood; you save it in your own words\n"
    "/ekos answer <question id> <your answer> — answer a question I asked you\n"
    "/ekos edit <card id> <your corrected text> — correct a card before saving it\n"
    "/ekos withdraw <testimony id> — withdraw something you said\n"
    "/ekos project <name> — the project for direct messages\n"
    "Send a file or a link to add it to the project's inbox."
)


def link_instructions(console_hint: str = "") -> str:
    where = f" ({console_hint})" if console_hint else ""
    return (
        "This account is not linked to an EKOS person yet, so I can do nothing for it. "
        f"Write `/ekos link`, then enter the code I send on the console's Channels page{where} "
        "while signed in."
    )


# ── who may see an answer (RFC 0253 §3.2, group chats) ──────────────────────

UNLINKED = "unlinked"


def answer_privately(
    *, direct: bool, used_knowledge: bool, member_scopes: list[object] | None
) -> bool:
    """Whether an answer must go only to the person who asked.

    The thread may see an answer only when it used nothing outside what **every** member may
    read: their common scope. `member_scopes` holds each member's read scope (`None` = the whole
    project, a dict = restricted, `UNLINKED` = no EKOS person and so no scope at all), or is
    `None` when the channel cannot say who its members are. An answer that read nothing from the
    ledger shows nothing anyone could be refused.
    """
    if direct or not used_knowledge:
        return False
    if member_scopes is None:
        return True
    return any(s is not None for s in member_scopes)


def chunks(text: str, limit: int) -> list[str]:
    """`text` cut at line ends into pieces a channel accepts."""
    out, cur = [], ""
    for line in text.splitlines(keepends=True):
        while len(line) > limit:
            if cur:
                out.append(cur)
                cur = ""
            out.append(line[:limit])
            line = line[limit:]
        if len(cur) + len(line) > limit:
            out.append(cur)
            cur = ""
        cur += line
    if cur or not out:
        out.append(cur)
    return out
