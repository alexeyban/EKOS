"""Saving a proposal card (RFC 0253 §3.6): the author's own action, never the model's.

`save_argvs` turns a card the author kept (and perhaps corrected) into `ekos testimony add`
calls attributed to that author. The text saved is the person's: what they wrote, or their edit
of it. The proposals only say what the statement is about, what code value it gives a meaning
to, and which question it answers — each one the person saw on the card with the words it was
taken from.

Reached from the console's Save route and the channels' Save button, both of which first check
that the click is the statement's author's. Not imported by any agent code (a test scans it).
"""

from __future__ import annotations

from . import models, people_write
from .auth import Principal
from .people_write import ReviewError


def author_key(principal: Principal) -> str:
    """How a proposal names its author: the same key a conversation uses."""
    return f"{principal.kind}:{principal.id}"


def _kept(p: models.Proposal, keep: list[int] | None) -> list[dict]:
    if keep is None:
        return list(p.items)
    bad = [i for i in keep if not 0 <= i < len(p.items)]
    if bad:
        raise ReviewError(f"no proposal at position {bad[0]}")
    return [p.items[i] for i in sorted(set(keep))]


def save_argvs(
    p: models.Proposal, by: str, *, edited: str | None = None, keep: list[int] | None = None
) -> list[list[str]]:
    """The writes for one Save: one testimony per code-value meaning kept (a testimony gives at
    most one meaning), else one testimony about everything kept."""
    text = (edited or "").strip() or p.text
    items = _kept(p, keep)
    about = [i["about"] for i in items if i["kind"] == "about"]
    meanings = [i for i in items if i["kind"] == "meaning"]
    answered = [i["question"] for i in items if i["kind"] == "answers"]
    answers = answered[0] if answered else (p.answers or None)
    common = {
        "channel": p.channel,
        "permalink": p.permalink or None,
        "attachments": list(p.attachments or []),
    }
    if not meanings:
        return [
            people_write.testimony_add_argv(
                text, by, about=about, answers=answers, message_ref=p.message_ref, **common
            )
        ]
    out = []
    for n, m in enumerate(meanings):
        ref = p.message_ref if len(meanings) == 1 else f"{p.message_ref}#{n + 1}"
        out.append(
            people_write.testimony_add_argv(
                text,
                by,
                about=[*about, m["about"]],
                answers=answers if n == 0 else None,
                meaning=m["meaning"],
                message_ref=ref,
                **common,
            )
        )
    return out
