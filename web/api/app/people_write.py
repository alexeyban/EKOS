"""People as a source (RFC 0253) and the questions they are asked (RFC 0254): the console's write
seam for the inbox, testimony and question routing.

Every command here is a CLI command run as an argv list, never a shell, through
`semantics_write`'s runner (which builds the command line with `home.command` and gives the
process its tenant's environment). Who a write is attributed to is never taken from a request
body when the console knows the person: the caller passes `semantics_write.reviewer_for`'s answer,
and the human-only ones (`testimony add|withdraw`, `inbox accept|purge`) are reached only from
routes that need the `review` or `admin` right — which an API token never has (RFC 0180).

This module is **not** imported by the chat agent or the channel agents (a test scans them): a
model can propose, only a person's own action reaches these functions.
"""

from __future__ import annotations

import json
import re
from typing import Any

from . import semantics_write
from .semantics_write import ReviewError

# An inbox item id: the first hex characters of the content's SHA-256 (the CLI uses 16).
_INBOX_ID = re.compile(r"^[0-9a-f]{4,64}$")
# A question or testimony id, or a prefix of one: a UUID's characters.
_OBJECT_ID = re.compile(r"^[0-9a-f][0-9a-f-]{3,35}$")
_CHANNELS = ("console", "telegram", "slack", "teams")
_MAX_TEXT = 4000
_MAX_ABOUT = 20
_MAX_SHORT = 300
_PATTERN = re.compile(r"^[\w.*$][\w.*$-]{0,119}$")


def _short(field: str, value: str | None) -> str | None:
    if value is None:
        return None
    value = value.strip()
    if not value:
        return None
    if len(value) > _MAX_SHORT:
        raise ReviewError(f"{field} is longer than {_MAX_SHORT} characters")
    if any(c in value for c in "\x00\n\r"):
        raise ReviewError(f"{field} contains a control character")
    return value


def _channel(channel: str) -> str:
    if channel not in _CHANNELS:
        raise ReviewError(f"channel is one of {', '.join(_CHANNELS)}")
    return channel


def inbox_id(value: str) -> str:
    if not _INBOX_ID.match(value or ""):
        raise ReviewError("an inbox item is named by its id (hex)")
    return value


def object_id(value: str) -> str:
    if not _OBJECT_ID.match(value or ""):
        raise ReviewError("an id is the hex id the list shows (or its first characters)")
    return value


# ── inbox ────────────────────────────────────────────────────────────────────


def inbox_add_argv(
    what: str,
    by: str,
    *,
    channel: str = "console",
    permalink: str | None = None,
    pending: bool = False,
) -> list[str]:
    """`ekos inbox add --as=<by> … --json -- <file|url>`. The file is one the console wrote to
    a private temporary path; a link must be http(s) (the CLI refuses internal addresses)."""
    if not what or "\x00" in what or len(what) > 4096:
        raise ReviewError("nothing to store")
    if what.startswith("-"):
        raise ReviewError("a path or link cannot start with '-'")
    argv = ["inbox", "add", f"--as={by}", f"--channel={_channel(channel)}"]
    link = _short("permalink", permalink)
    if link:
        argv.append(f"--permalink={link}")
    if pending:
        argv.append("--pending")
    return [*argv, "--json", "--", what]


def is_link(text: str) -> bool:
    return bool(re.fullmatch(r"https?://\S{3,2000}", text.strip()))


def inbox_accept_argv(item: str, by: str) -> list[str]:
    return ["inbox", "accept", inbox_id(item), f"--as={by}"]


def inbox_purge_argv(item: str, by: str) -> list[str]:
    return ["inbox", "purge", inbox_id(item), f"--as={by}"]


# ── testimony ────────────────────────────────────────────────────────────────


def testimony_add_argv(
    text: str,
    by: str,
    *,
    about: list[str] | None = None,
    answers: str | None = None,
    meaning: str | None = None,
    channel: str = "console",
    message_ref: str | None = None,
    permalink: str | None = None,
    on_behalf_of: str | None = None,
    attachments: list[str] | None = None,
) -> list[str]:
    """`ekos testimony add --text=… --as=<by> … --json`. `text` is the person's own words (or
    their edit of them): never a model's."""
    text = (text or "").strip()
    if not text:
        raise ReviewError("a testimony needs the person's text")
    if len(text) > _MAX_TEXT:
        raise ReviewError(f"the text is longer than {_MAX_TEXT} characters")
    if "\x00" in text:
        raise ReviewError("the text contains a NUL byte")
    about = about or []
    if len(about) > _MAX_ABOUT:
        raise ReviewError(f"at most {_MAX_ABOUT} subjects")
    argv = ["testimony", "add", f"--text={text}", f"--as={by}", f"--channel={_channel(channel)}"]
    for a in about:
        a = _short("about", a)
        if a:
            argv.append(f"--about={a}")
    if answers:
        argv.append(f"--answers={object_id(answers)}")
    meaning = _short("meaning", meaning)
    if meaning:
        argv.append(f"--meaning={meaning}")
    for flag, value in (
        ("--message-ref", _short("message ref", message_ref)),
        ("--permalink", _short("permalink", permalink)),
        ("--on-behalf-of", _short("on behalf of", on_behalf_of)),
    ):
        if value:
            argv.append(f"{flag}={value}")
    for item in attachments or []:
        argv.append(f"--attach={inbox_id(item)}")
    argv.append("--json")
    return argv


def testimony_withdraw_argv(tid: str, by: str) -> list[str]:
    return ["testimony", "withdraw", object_id(tid), f"--as={by}"]


# ── question routing ─────────────────────────────────────────────────────────


def reassign_argv(qid: str, to: str) -> list[str]:
    to = _short("to", to)
    if not to:
        raise ReviewError("say who the question goes to")
    return ["questions", "reassign", object_id(qid), f"--to={to}"]


def decline_argv(qid: str, by: str) -> list[str]:
    return ["questions", "decline", object_id(qid), f"--as={by}"]


def mute_argv(pattern: str, by: str) -> list[str]:
    if not _PATTERN.match(pattern or ""):
        raise ReviewError("a table pattern: letters, digits, _ . $ - and * only")
    return ["questions", "mute", f"--as={by}", "--", pattern]


# ── running ──────────────────────────────────────────────────────────────────


async def run(ekos_bin: str, workspace_path: str, argv: list[str]) -> str:
    """Run one write; its printed output. Raises `semantics_write.LedgerBusy` / `CliFailed`."""
    return await semantics_write.review(ekos_bin, workspace_path, argv, time_limit=120.0)


async def run_json(ekos_bin: str, workspace_path: str, argv: list[str]) -> dict[str, Any]:
    out = await run(ekos_bin, workspace_path, argv)
    try:
        value = json.loads(out)
    except json.JSONDecodeError:
        return {"message": out}
    return value if isinstance(value, dict) else {"result": value}
