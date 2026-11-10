"""The EKOS tools a chat model is offered, and how its calls reach them (RFC 0187), shared by the
console chat and every channel (RFC 0253).

Only tools whose MCP policy action is `read` and that the caller may call through the gateway's
own check are offered; a metered tool never is. Calls go through the scoped client (the
caller's read scope on every call) and are audited without their arguments.
"""

from __future__ import annotations

import json
from collections.abc import Awaitable, Callable
from typing import Any

from .. import authz, mcp_policy, models
from ..auth import Principal
from ..mcp_client import McpToolError, ScopedMcp

CallTool = Callable[[str, dict[str, Any]], Awaitable[tuple[str, bool]]]


async def offered(backend: ScopedMcp, may: Callable[[str], bool]) -> list[dict]:
    """The server's tools the model may be offered: read tools the caller may call."""
    return [
        t
        for t in await backend.list_tools()
        if mcp_policy.action_for(str(t.get("name", ""))) == "read"
        and str(t.get("name", "")) not in mcp_policy.METERED_TOOLS
        and may(str(t.get("name", "")))
    ]


def spec(tools: list[dict]) -> list[dict]:
    """The offered tools in the providers' shape."""
    return [
        {
            "name": str(t["name"]),
            "description": str(t.get("description", "")),
            "input_schema": t.get("inputSchema") or {"type": "object", "properties": {}},
        }
        for t in tools
    ]


def text_of(result: Any) -> tuple[str, bool]:
    """An MCP `tools/call` result as text, and whether it is an error."""
    if not isinstance(result, dict):
        return json.dumps(result), False
    parts = [
        c.get("text", "")
        for c in result.get("content", [])
        if isinstance(c, dict) and c.get("type") == "text"
    ]
    return "\n".join(parts) or json.dumps(result), bool(result.get("isError"))


def provider_problem(settings, ws: models.Workspace) -> str | None:
    """Why the configured chat model may not be used for this workspace: it is paid per call
    and the workspace's tenant has metered calls off (RFC 0183)."""
    from . import providers

    if providers.is_metered(settings.chat_provider, settings.chat_base_url):
        tenant = models.get_tenant(ws.tenant_id)
        if tenant is None or not tenant.allow_metered:
            return (
                f"{settings.chat_provider} is paid per call and metered calls are off for "
                f"tenant {ws.tenant_id}; a platform owner turns them on in Admin"
            )
    return None


def caller(
    principal: Principal,
    ws: models.Workspace,
    backend: ScopedMcp,
    names: set[str],
    *,
    label: str = "chat",
) -> CallTool:
    """A `call_tool` for `agent.turn` that refuses any tool not offered."""

    async def call_tool(name: str, arguments: dict[str, Any]) -> tuple[str, bool]:
        allowed = name in names
        authz.audit(
            principal,
            f"{label}: {name}",
            "allowed" if allowed else "denied",
            tenant_id=ws.tenant_id,
            workspace_id=ws.id,
        )
        if not allowed:
            return f"{name} is not available in this chat", True
        try:
            return text_of(await backend.call_tool_raw(name, arguments))
        except McpToolError as exc:
            return str(exc), True

    return call_tool
