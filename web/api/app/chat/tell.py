"""Tell mode (RFC 0253 §3.5): a person's statement → proposals the person may save. Nothing here
writes the ledger.

The model may look things up with the same read tools as an answer (to resolve "status 7 in the
orders table" to `orders.status`), then answers with JSON proposals. Each proposal must quote the
span of the person's own words it comes from — the RFC 0170 `semantics_llm.rs` discipline,
cite-or-drop: a proposal whose quote is missing, is not in the statement, or hedges ("probably")
is dropped and counted, never shown. What the person then saves is their own text.
"""

from __future__ import annotations

import json
import re
import uuid
from typing import Any

from .. import models

KINDS = ("meaning", "about", "answers")
MAX_QUOTE = 400
MAX_FIELD = 300
MAX_ITEMS = 12

_HEDGES = (" likely ", " probably ", " may ", " might ", " perhaps ", " possibly ", " i think ")


def system_prompt(project: str) -> str:
    return (
        f"A person is telling you something about the project '{project}'. Do not answer them. "
        "Work out what their statement is about, using the EKOS tools to find the exact "
        "objects (a table, a column, a code value such as `orders.status = 7`, an open "
        "question). Then reply with JSON only, no prose: "
        '{"proposals": [{"kind": "meaning", "about": "orders.status = 7", '
        '"meaning": "cancelled by the customer", "quote": "<their exact words>"}, '
        '{"kind": "about", "about": "<object id or exact name>", "quote": "…"}, '
        '{"kind": "answers", "question": "<open question id>", "quote": "…"}]}. '
        "`meaning` is for what a code value means; `about` for any other statement about an "
        "object; `answers` when they answer an open question. Every proposal MUST quote, word "
        "for word, the part of the person's message it comes from; a proposal without such a "
        "quote is discarded. Propose nothing they did not say. Do not guess: a proposal that "
        'says "likely" or "probably" is discarded.'
    )


# Straight, typographic and guillemet quotation marks, by code point.
_NO_QUOTES = dict.fromkeys((0x22, 0x27, 0x60, 0x201C, 0x201D, 0x2018, 0x2019, 0xAB, 0xBB))


def _norm(text: str) -> str:
    # Quotation marks of any style do not count: a model may change ' to a typographic one.
    return " ".join(text.translate(_NO_QUOTES).lower().split())


def _hedges(text: str) -> bool:
    t = f" {_norm(text)} "
    return any(h in t for h in _HEDGES)


def _json_body(reply: str) -> Any:
    """The JSON object in a reply: the whole text, a fenced block, or the outermost braces."""
    text = reply.strip()
    fenced = re.search(r"```(?:json)?\s*(.*?)```", text, re.S)
    if fenced:
        text = fenced.group(1).strip()
    start, end = text.find("{"), text.rfind("}")
    if start < 0 or end <= start:
        return None
    try:
        return json.loads(text[start : end + 1])
    except json.JSONDecodeError:
        return None


def _field(item: dict, name: str) -> str:
    value = item.get(name)
    if not isinstance(value, str):
        return ""
    value = value.strip()
    if len(value) > MAX_FIELD or any(c in value for c in "\x00\n\r"):
        return ""
    return value


def _one(item: Any, statement_norm: str) -> dict | None:
    """One proposal as kept, or `None` when it must be dropped."""
    if not isinstance(item, dict) or item.get("kind") not in KINDS:
        return None
    quote = item.get("quote")
    if not isinstance(quote, str) or not quote.strip() or len(quote) > MAX_QUOTE:
        return None
    if _norm(quote) not in statement_norm or len(_norm(quote)) < 3:
        return None
    kind = item["kind"]
    out: dict[str, str] = {"kind": kind, "quote": quote.strip()}
    if kind == "answers":
        question = _field(item, "question")
        if not re.fullmatch(r"[0-9a-f][0-9a-f-]{3,35}", question):
            return None
        out["question"] = question
        return out
    about = _field(item, "about")
    if not about:
        return None
    out["about"] = about
    if kind == "meaning":
        meaning = _field(item, "meaning")
        if not meaning or _hedges(meaning):
            return None
        out["meaning"] = meaning
    return out


def keep_cited(reply: str, statement: str) -> tuple[list[dict], int] | None:
    """The proposals worth showing, and how many were dropped; `None` when the reply is not the
    JSON asked for."""
    body = _json_body(reply)
    if not isinstance(body, dict) or not isinstance(body.get("proposals"), list):
        return None
    norm = _norm(statement)
    kept: list[dict] = []
    dropped = 0
    for item in body["proposals"]:
        one = _one(item, norm)
        if one is None or one in kept:
            dropped += 1
            continue
        if len(kept) < MAX_ITEMS:
            kept.append(one)
        else:
            dropped += 1
    return kept, dropped


def new_proposal(
    workspace_id: str,
    author: str,
    text: str,
    items: list[dict],
    *,
    channel: str = "console",
    channel_account: str = "",
    message_ref: str = "",
    permalink: str = "",
    answers: str = "",
    attachments: list[str] | None = None,
) -> models.Proposal:
    """Keep a card for its author to save or discard (the console database, not the ledger)."""
    row = models.Proposal(
        id=uuid.uuid4().hex,
        workspace_id=workspace_id,
        author=author,
        channel=channel,
        channel_account=channel_account,
        message_ref=message_ref or uuid.uuid4().hex[:16],
        permalink=permalink,
        text=text,
        items=items,
        answers=answers,
        attachments=attachments or [],
    )
    models.add(row)
    return row


def card_lines(items: list[dict]) -> list[str]:
    """How a card states each proposal, the same words in every channel."""
    lines = []
    for it in items:
        if it["kind"] == "meaning":
            lines.append(f'{it["about"]} means "{it["meaning"]}"  [from: "{it["quote"]}"]')
        elif it["kind"] == "answers":
            lines.append(f'answers question {it["question"][:8]}  [from: "{it["quote"]}"]')
        else:
            lines.append(f'about {it["about"]}  [from: "{it["quote"]}"]')
    return lines


async def propose(
    provider,
    project: str,
    statement: str,
    tools: list[dict],
    call_tool,
    *,
    max_calls: int,
) -> tuple[list[dict], int, list[str]]:
    """Run the model over one statement: (kept proposals, dropped count, tools it called)."""
    from . import agent

    messages: list[dict[str, Any]] = [{"role": "user", "content": statement}]
    called: list[str] = []
    final = ""
    async for event in agent.turn(
        provider, system_prompt(project), messages, tools, call_tool, max_calls=max_calls
    ):
        if event["type"] == "tool_call":
            called.append(str(event["name"]))
        elif event["type"] == "text":
            final = str(event["text"])
    result = keep_cited(final, statement)
    if result is None:
        return [], 0, called
    kept, dropped = result
    return kept, dropped, called
