"""Chat with a configured model using the project's EKOS tools (RFC 0187).

The tools are the project's MCP tools that need only `read` and that this person may call
through the gateway's own check (role, and for a restricted reader the scoped tools only); they
are called through `ScopedMcp` with the person's read scope and audited as the gateway does.
No tool that writes or decides a review is ever offered to the model.

RFC 0253 adds tell mode: `POST …/chat/tell` turns a statement into a card of proposals, each
quoting the person's words (cite-or-drop). The model writes nothing; the card's Save is a
separate route for the author only (`routes/testimony.py`).
"""

from __future__ import annotations

import json
import re
import uuid
from collections.abc import AsyncIterator

from fastapi import APIRouter, Depends, HTTPException, Request
from fastapi.responses import StreamingResponse
from pydantic import BaseModel

from .. import authz, models
from ..auth import Principal
from ..chat import agent, providers, tell, tools
from ..deps import mcp_for_workspace, require_workspace
from ..mcp_client import ScopedMcp
from ..settings import Settings, get_settings
from ._responses import NOT_FOUND, UNPROCESSABLE
from .mcp_gateway import _Caller

router = APIRouter(tags=["chat"], dependencies=[Depends(authz.not_read_only)])

CONFLICT = {"description": "No chat model, or a paid model while metered calls are off"}


class ChatIn(BaseModel):
    message: str
    conversation_id: str | None = None


def _owner(p: Principal) -> str:
    return f"{p.kind}:{p.id}"


@router.get("/chat/model")
async def chat_model(
    _: Principal = Depends(authz.authenticated), settings: Settings = Depends(get_settings)
) -> dict:
    return {
        "configured": bool(settings.chat_provider),
        "provider": settings.chat_provider,
        "model": settings.chat_model,
        "metered": bool(settings.chat_provider)
        and providers.is_metered(settings.chat_provider, settings.chat_base_url),
    }


def _mine(cid: str, ws: models.Workspace, principal: Principal) -> models.Conversation:
    conv = models.get_conversation(cid)
    if conv is None or conv.workspace_id != ws.id or conv.owner != _owner(principal):
        raise HTTPException(status_code=404, detail="no such conversation")
    return conv


@router.get(
    "/workspaces/{workspace_id}/chat", dependencies=[Depends(authz.allow("read", scoped=True))]
)
async def list_chats(
    ws: models.Workspace = Depends(require_workspace),
    principal: Principal = Depends(authz.authenticated),
) -> list[dict]:
    return [
        {"id": c.id, "title": c.title, "updated_at": c.updated_at.isoformat()}
        for c in models.list_conversations(ws.id, _owner(principal))
    ]


@router.get(
    "/workspaces/{workspace_id}/chat/{cid}",
    dependencies=[Depends(authz.allow("read", scoped=True))],
    responses={404: NOT_FOUND},
)
async def get_chat(
    cid: str,
    ws: models.Workspace = Depends(require_workspace),
    principal: Principal = Depends(authz.authenticated),
) -> dict:
    conv = _mine(cid, ws, principal)
    return {"id": conv.id, "title": conv.title, "messages": models.chat_messages(conv.id)}


@router.delete(
    "/workspaces/{workspace_id}/chat/{cid}",
    dependencies=[Depends(authz.allow("read", scoped=True))],
    responses={404: NOT_FOUND},
)
async def delete_chat(
    cid: str,
    ws: models.Workspace = Depends(require_workspace),
    principal: Principal = Depends(authz.authenticated),
) -> dict:
    models.delete_conversation(_mine(cid, ws, principal).id)
    return {"ok": True}


def _sse(event: dict) -> str:
    return f"data: {json.dumps(event)}\n\n"


def _provider_for(settings: Settings, ws: models.Workspace):
    """The configured chat provider, or 409 when it is paid per call and the workspace's tenant
    has metered calls off, or when the settings do not make a provider."""
    problem = tools.provider_problem(settings, ws)
    if problem:
        raise HTTPException(status_code=409, detail=problem)
    try:
        return providers.from_settings(settings)
    except providers.ProviderError as exc:
        raise HTTPException(status_code=409, detail=str(exc)) from exc


def _conversation_for(
    conversation_id: str | None, message: str, ws: models.Workspace, principal: Principal
) -> models.Conversation:
    """The caller's own conversation by id, or a new one titled by the message."""
    if conversation_id:
        return _mine(conversation_id, ws, principal)
    conv = models.Conversation(
        id=uuid.uuid4().hex, workspace_id=ws.id, owner=_owner(principal), title=message[:80]
    )
    models.add(conv)
    return conv


async def _tools_for(
    request: Request, principal: Principal, ws: models.Workspace, backend: ScopedMcp
) -> tuple[list[dict], tools.CallTool]:
    """What the model is offered and how its calls are made: the gateway's own check of who may
    call which tool; the client is the scoped one."""
    caller = _Caller(principal, ws, request.app.state.supervisor)
    offered = await tools.offered(backend, caller.may)
    names = {str(t["name"]) for t in offered}
    return tools.spec(offered), tools.caller(principal, ws, backend, names)


@router.post(
    "/workspaces/{workspace_id}/chat",
    dependencies=[Depends(authz.allow("read", scoped=True))],
    responses={404: NOT_FOUND, 409: CONFLICT, 422: UNPROCESSABLE},
)
async def chat(
    body: ChatIn,
    request: Request,
    ws: models.Workspace = Depends(require_workspace),
    principal: Principal = Depends(authz.authenticated),
    settings: Settings = Depends(get_settings),
    backend: ScopedMcp = Depends(mcp_for_workspace),
) -> StreamingResponse:
    if not settings.chat_provider:
        raise HTTPException(
            status_code=409,
            detail="no chat model is configured; an owner sets one in Settings → Chat",
        )
    message = body.message.strip()
    if not message:
        raise HTTPException(status_code=422, detail="an empty message")
    provider = _provider_for(settings, ws)
    conv = _conversation_for(body.conversation_id, message, ws, principal)
    spec, call_tool = await _tools_for(request, principal, ws, backend)

    history = models.chat_messages(conv.id)
    messages = [*history, {"role": "user", "content": message}]
    start = len(history)

    async def events() -> AsyncIterator[str]:
        yield _sse({"type": "conversation", "id": conv.id, "title": conv.title})
        try:
            async for event in agent.turn(
                provider,
                agent.system_prompt(ws.name),
                messages,
                spec,
                call_tool,
                max_calls=settings.chat_max_tool_calls,
            ):
                yield _sse(event)
        except providers.ProviderError as exc:
            yield _sse({"type": "error", "message": str(exc)})
        finally:
            models.add_chat_messages(conv.id, messages[start:])
        yield _sse({"type": "done"})

    return StreamingResponse(events(), media_type="text/event-stream")


class TellIn(BaseModel):
    message: str
    # Set when the statement answers an open question (RFC 0254 §3.3).
    answers: str | None = None


_QUESTION_ID = re.compile(r"[0-9a-f][0-9a-f-]{3,35}")


@router.post(
    "/workspaces/{workspace_id}/chat/tell",
    dependencies=[Depends(authz.allow("read", scoped=True))],
    responses={409: CONFLICT, 422: UNPROCESSABLE},
)
async def tell_mode(
    body: TellIn,
    request: Request,
    ws: models.Workspace = Depends(require_workspace),
    principal: Principal = Depends(authz.authenticated),
    settings: Settings = Depends(get_settings),
    backend: ScopedMcp = Depends(mcp_for_workspace),
) -> dict:
    """RFC 0253 §3.5: a statement → a card of proposals, each quoting the person's words. The
    model reads with the same read tools as an answer and writes nothing; the card waits for the
    author's Save."""
    if not settings.chat_provider:
        raise HTTPException(status_code=409, detail="no chat model is configured")
    if principal.kind == "token":
        raise HTTPException(status_code=403, detail="a statement is a person's, not a token's")
    statement = body.message.strip()
    if not statement or len(statement) > 4000:
        raise HTTPException(status_code=422, detail="a statement is 1 to 4000 characters")
    answers = (body.answers or "").strip()
    if answers and not _QUESTION_ID.fullmatch(answers):
        raise HTTPException(status_code=422, detail="answers: a question id")
    provider = _provider_for(settings, ws)
    spec, call_tool = await _tools_for(request, principal, ws, backend)
    try:
        kept, dropped, called = await tell.propose(
            provider, ws.name, statement, spec, call_tool, max_calls=settings.chat_max_tool_calls
        )
    except providers.ProviderError as exc:
        raise HTTPException(status_code=502, detail=str(exc)) from exc
    card = tell.new_proposal(ws.id, _owner(principal), statement, kept, answers=answers)
    return {
        "proposal": card.id,
        "text": statement,
        "items": kept,
        "lines": tell.card_lines(kept),
        "dropped": dropped,
        "tools": called,
        "answers": answers or None,
    }
