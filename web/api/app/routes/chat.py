"""Chat with a configured model using the project's EKOS tools (RFC 0187).

The tools are the project's MCP tools that need only `read` and that this person may call
through the gateway's own check (role, and for a restricted reader the scoped tools only); they
are called through `ScopedMcp` with the person's read scope and audited as the gateway does.
No tool that writes or decides a review is ever offered to the model.
"""

from __future__ import annotations

import json
import uuid
from collections.abc import AsyncIterator
from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Request
from fastapi.responses import StreamingResponse
from pydantic import BaseModel

from .. import authz, mcp_policy, models
from ..auth import Principal
from ..chat import agent, providers
from ..deps import mcp_for_workspace, require_workspace
from ..mcp_client import McpToolError, ScopedMcp
from ..settings import Settings, get_settings
from ._responses import NOT_FOUND
from .mcp_gateway import _Caller

router = APIRouter(tags=["chat"])

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


def _text_of(result: Any) -> tuple[str, bool]:
    """An MCP `tools/call` result as text, and whether it is an error."""
    if not isinstance(result, dict):
        return json.dumps(result), False
    parts = [
        c.get("text", "")
        for c in result.get("content", [])
        if isinstance(c, dict) and c.get("type") == "text"
    ]
    return "\n".join(parts) or json.dumps(result), bool(result.get("isError"))


def _sse(event: dict) -> str:
    return f"data: {json.dumps(event)}\n\n"


@router.post(
    "/workspaces/{workspace_id}/chat",
    dependencies=[Depends(authz.allow("read", scoped=True))],
    responses={404: NOT_FOUND, 409: CONFLICT},
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
    if providers.is_metered(settings.chat_provider, settings.chat_base_url):
        tenant = models.get_tenant(ws.tenant_id)
        if tenant is None or not tenant.allow_metered:
            raise HTTPException(
                status_code=409,
                detail=f"{settings.chat_provider} is paid per call and metered calls are off for "
                f"tenant {ws.tenant_id}; a platform owner turns them on in Admin",
            )
    try:
        provider = providers.from_settings(settings)
    except providers.ProviderError as exc:
        raise HTTPException(status_code=409, detail=str(exc)) from exc

    if body.conversation_id:
        conv = _mine(body.conversation_id, ws, principal)
    else:
        conv = models.Conversation(
            id=uuid.uuid4().hex, workspace_id=ws.id, owner=_owner(principal), title=message[:80]
        )
        models.add(conv)

    # The gateway's own check of who may call which tool; the client is the scoped one.
    caller = _Caller(principal, ws, request.app.state.supervisor)
    offered = [
        t
        for t in await backend.list_tools()
        if mcp_policy.action_for(str(t.get("name", ""))) == "read"
        and caller.may(str(t.get("name", "")))
    ]
    names = {str(t["name"]) for t in offered}
    spec = [
        {
            "name": str(t["name"]),
            "description": str(t.get("description", "")),
            "input_schema": t.get("inputSchema") or {"type": "object", "properties": {}},
        }
        for t in offered
    ]

    async def call_tool(name: str, arguments: dict[str, Any]) -> tuple[str, bool]:
        allowed = name in names
        authz.audit(
            principal,
            f"chat: {name}",
            "allowed" if allowed else "denied",
            tenant_id=ws.tenant_id,
            workspace_id=ws.id,
        )
        if not allowed:
            return f"{name} is not available in this chat", True
        try:
            return _text_of(await backend.call_tool_raw(name, arguments))
        except McpToolError as exc:
            return str(exc), True

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
