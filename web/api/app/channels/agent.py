"""Ask and tell from a channel (RFC 0253 §3.1): RFC 0187's agent loop and tell mode, run for the
linked person with their read scope. The model is offered the read tools the person may call and
nothing else; it writes nothing. A tell ends in a proposal card that only the person can save.
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass

from .. import models
from ..auth import Principal
from ..chat import agent, providers, tell, tools
from ..mcp_client import ScopedMcp
from ..routes.mcp_gateway import _Caller
from ..settings import Settings

# How much of a channel conversation the model sees again.
HISTORY = 20


@dataclass
class Answer:
    text: str
    # Whether the answer read anything from the ledger (the group-chat rule needs to know).
    used_knowledge: bool


class ChannelModelError(RuntimeError):
    """No model, or a paid one the tenant has not allowed."""


def model_for(settings: Settings, ws: models.Workspace):
    if not settings.chat_provider:
        raise ChannelModelError("no chat model is configured for this console")
    problem = tools.provider_problem(settings, ws)
    if problem:
        raise ChannelModelError(problem)
    try:
        return providers.from_settings(settings)
    except providers.ProviderError as exc:
        raise ChannelModelError(str(exc)) from exc


async def _offer(
    principal: Principal, ws: models.Workspace, supervisor
) -> tuple[list[dict], tools.CallTool]:
    caller = _Caller(principal, ws, supervisor)
    backend: ScopedMcp = await caller.backend()
    offered = await tools.offered(backend, caller.may)
    names = {str(t["name"]) for t in offered}
    return tools.spec(offered), tools.caller(principal, ws, backend, names, label="channel")


def conversation_id(channel: str, conversation: str, account: str, ws_id: str) -> str:
    raw = f"{channel}\x00{conversation}\x00{account}\x00{ws_id}".encode()
    return hashlib.sha256(raw).hexdigest()[:32]


def _conversation(
    channel: str, conversation: str, account: str, principal: Principal, ws: models.Workspace,
    title: str,
) -> models.Conversation:  # fmt: skip
    cid = conversation_id(channel, conversation, account, ws.id)
    conv = models.get_conversation(cid)
    if conv is None:
        conv = models.Conversation(
            id=cid,
            workspace_id=ws.id,
            owner=f"{principal.kind}:{principal.id}",
            title=title[:80],
            channel=channel,
        )
        models.add(conv)
    return conv


async def ask(
    settings: Settings,
    supervisor,
    principal: Principal,
    ws: models.Workspace,
    *,
    channel: str,
    conversation: str,
    account: str,
    question: str,
) -> Answer:
    provider = model_for(settings, ws)
    spec, call_tool = await _offer(principal, ws, supervisor)
    conv = _conversation(channel, conversation, account, principal, ws, question)
    history = models.chat_messages(conv.id)[-HISTORY:]
    # A cut history must not start with a tool result whose call was cut off.
    while history and history[0].get("role") != "user":
        history.pop(0)
    messages = [*history, {"role": "user", "content": question}]
    start = len(history)
    used = False
    text = ""
    try:
        async for event in agent.turn(
            provider,
            agent.system_prompt(ws.name),
            messages,
            spec,
            call_tool,
            max_calls=settings.chat_max_tool_calls,
        ):
            if event["type"] == "tool_call":
                used = True
            elif event["type"] == "text":
                text = str(event["text"])
    except providers.ProviderError as exc:
        raise ChannelModelError(str(exc)) from exc
    finally:
        models.add_chat_messages(conv.id, messages[start:])
    return Answer(text or "(no answer)", used)


async def propose(
    settings: Settings,
    supervisor,
    principal: Principal,
    ws: models.Workspace,
    *,
    statement: str,
) -> tuple[list[dict], int]:
    """A statement's proposals (cite-or-drop) and how many were dropped."""
    provider = model_for(settings, ws)
    spec, call_tool = await _offer(principal, ws, supervisor)
    try:
        kept, dropped, _ = await tell.propose(
            provider, ws.name, statement, spec, call_tool, max_calls=settings.chat_max_tool_calls
        )
    except providers.ProviderError as exc:
        raise ChannelModelError(str(exc)) from exc
    return kept, dropped
