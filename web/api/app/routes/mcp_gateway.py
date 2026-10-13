"""MCP gateway (RFC 0182): `POST /mcp/{tenant}/{project}`.

MCP Streamable HTTP for agents, in front of the project's own `ekos mcp serve`: the caller is
authenticated (an API token, RFC 0180), each tool call is checked against their role, given
their read scope (RFC 0181), forwarded, and recorded in the audit log.

The forwarded call is built from the tool name and arguments alone. Nothing else the client
sent reaches the server, so a caller cannot supply or widen a scope.
"""

from __future__ import annotations

import json
from importlib.metadata import PackageNotFoundError, version
from typing import Any

from fastapi import APIRouter, Depends, HTTPException, Request
from fastapi.responses import JSONResponse, Response

from .. import authz, mcp_policy, models
from ..auth import Principal
from ..deps import get_supervisor
from ..mcp_client import McpError, McpToolError, ScopedMcp
from ..supervisor import McpSupervisor, _NotReady
from .workspaces import workspace_id_for

router = APIRouter(prefix="/mcp", tags=["mcp"])

DEFAULT_PROTOCOL = "2025-06-18"
# A tool call is a name and a few arguments; an EKL query is the longest of them.
MAX_BODY = 1 << 20
MAX_BATCH = 50

PARSE_ERROR = -32700
INVALID_REQUEST = -32600
METHOD_NOT_FOUND = -32601
INVALID_PARAMS = -32602
INTERNAL_ERROR = -32603


def _gateway_version() -> str:
    try:
        return version("ekos-console-api")
    except PackageNotFoundError:
        return "0.0.0+dev"


def _error(msg_id: Any, code: int, message: str) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": msg_id, "error": {"code": code, "message": message}}


def _result(msg_id: Any, result: Any) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": msg_id, "result": result}


def _refusal(text: str) -> dict[str, Any]:
    """A tool result that says no, in the shape `ekos mcp serve` uses for a failed tool."""
    return {"content": [{"type": "text", "text": text}], "isError": True}


class _Caller:
    """One request's caller, their project, and what they may do there."""

    def __init__(self, principal: Principal, ws: models.Workspace, supervisor: McpSupervisor):
        self.principal = principal
        self.ws = ws
        self.supervisor = supervisor
        self.scope = authz.scope_for(principal, ws)

    @property
    def restricted(self) -> bool:
        return self.scope is not None

    def may(self, tool: str) -> bool:
        if self.restricted and tool not in mcp_policy.SCOPED_TOOLS:
            return False
        return authz.authorize(
            self.principal, mcp_policy.action_for(tool), self.ws.tenant_id, self.ws.id
        )

    async def backend(self) -> ScopedMcp:
        return ScopedMcp(await self.supervisor.acquire(self.ws), self.scope)


async def _tools_list(caller: _Caller) -> dict[str, Any]:
    tools = await (await caller.backend()).list_tools()
    return {"tools": [t for t in tools if caller.may(str(t.get("name", "")))]}


async def _tools_call(caller: _Caller, params: Any) -> dict[str, Any]:
    name = params.get("name") if isinstance(params, dict) else None
    arguments = params.get("arguments") if isinstance(params, dict) else None
    if not isinstance(name, str) or not name:
        raise ValueError("`name` is the tool to call")
    if arguments is not None and not isinstance(arguments, dict):
        raise ValueError("`arguments` is an object")

    allowed = caller.may(name)
    # Every call is recorded, reads too. Not the arguments: a query holds what the caller is
    # looking for.
    authz.audit(
        caller.principal,
        f"mcp: {name}",
        "allowed" if allowed else "denied",
        tenant_id=caller.ws.tenant_id,
        workspace_id=caller.ws.id,
    )
    if not allowed:
        if caller.restricted and name not in mcp_policy.SCOPED_TOOLS:
            return _refusal(
                f"{name} is not available to a reader with restricted access to this project"
            )
        return _refusal(f"{name} needs the right to {mcp_policy.action_for(name)} this project")
    try:
        # Built from the name and arguments only: whatever else the client sent, `_meta`
        # included, stops here. The scope is the one computed from the caller's bindings.
        return await (await caller.backend()).call_tool_raw(name, arguments or {})
    except McpToolError as exc:
        return _refusal(str(exc))


async def _handle(caller: _Caller, message: Any) -> dict[str, Any] | None:
    """One JSON-RPC message → its response, or `None` for a notification."""
    if not isinstance(message, dict) or message.get("jsonrpc") != "2.0":
        return _error(None, INVALID_REQUEST, "not a JSON-RPC 2.0 message")
    method = message.get("method")
    if "id" not in message:
        return None  # a notification: accepted, nothing to answer, nothing forwarded
    msg_id = message["id"]
    params = message.get("params") or {}
    try:
        if method == "initialize":
            requested = params.get("protocolVersion") if isinstance(params, dict) else None
            return _result(
                msg_id,
                {
                    "protocolVersion": requested
                    if isinstance(requested, str)
                    else DEFAULT_PROTOCOL,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "ekos-gateway", "version": _gateway_version()},
                },
            )
        if method == "ping":
            return _result(msg_id, {})
        if method == "tools/list":
            return _result(msg_id, await _tools_list(caller))
        if method == "tools/call":
            return _result(msg_id, await _tools_call(caller, params))
    except ValueError as exc:
        return _error(msg_id, INVALID_PARAMS, str(exc))
    except _NotReady as exc:
        return _error(msg_id, INTERNAL_ERROR, f"the project's server is not ready: {exc}")
    except (McpError, ConnectionError, OSError, TimeoutError) as exc:
        return _error(msg_id, INTERNAL_ERROR, f"the project's server failed: {exc}")
    return _error(msg_id, METHOD_NOT_FOUND, f"method not found: {method}")


def _encode(request: Request, responses: list[dict[str, Any]], batch: bool) -> Response:
    if not responses:
        return Response(status_code=202)
    # RFC 0143: a client that lists `text/event-stream` gets the answer as one finished stream
    # (ChatGPT requires it); a bare `*/*` gets JSON.
    if "text/event-stream" in request.headers.get("accept", ""):
        body = "".join(
            f"event: message\ndata: {json.dumps(r, separators=(',', ':'))}\n\n" for r in responses
        )
        return Response(body, media_type="text/event-stream", headers={"Cache-Control": "no-cache"})
    return JSONResponse(responses if batch else responses[0])


@router.api_route("/{tenant_id}/{project}", methods=["GET", "DELETE"], include_in_schema=False)
async def no_stream(tenant_id: str, project: str) -> Response:
    """MCP: `405` on `GET` is the defined answer for "no server-initiated stream here", and on
    `DELETE` for "no session to end". Said to anyone: it tells nothing about any project."""
    return Response(status_code=405, headers={"Allow": "POST"})


@router.post("/{tenant_id}/{project}")
async def mcp(
    tenant_id: str,
    project: str,
    request: Request,
    principal: Principal = Depends(authz.authenticated),
    supervisor: McpSupervisor = Depends(get_supervisor),
) -> Response:
    # 404 for a project that does not exist or that this caller has no role in: the same.
    ws = authz.workspace_for(principal, workspace_id_for(tenant_id, project))
    if ws.tenant_id != tenant_id:
        raise HTTPException(status_code=404, detail=f"unknown workspace {project!r}")

    raw = await request.body()
    if len(raw) > MAX_BODY:
        raise HTTPException(status_code=413, detail="request body too large")
    try:
        payload = json.loads(raw)
    except (ValueError, UnicodeDecodeError):
        return JSONResponse(_error(None, PARSE_ERROR, "the body is not JSON"), status_code=400)

    batch = isinstance(payload, list)
    messages = payload if batch else [payload]
    if not messages or len(messages) > MAX_BATCH:
        return JSONResponse(
            _error(None, INVALID_REQUEST, f"a batch holds 1 to {MAX_BATCH} messages"),
            status_code=400,
        )

    caller = _Caller(principal, ws, supervisor)
    responses = []
    for message in messages:
        response = await _handle(caller, message)
        if response is not None:
            responses.append(response)
    return _encode(request, responses, batch)
