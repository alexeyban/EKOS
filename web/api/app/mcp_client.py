"""Asyncio NDJSON/TCP MCP client for `ekos mcp serve --tcp` (RFC 0128 §2).

The console reads a compiled workspace only through this client. The transport is raw
newline-delimited JSON-RPC 2.0 over a TCP socket (RFC 0115); the official `mcp` SDK targets
stdio and Streamable HTTP, so this is a small purpose-built client instead.
"""

from __future__ import annotations

import asyncio
import json
from typing import Any


class McpError(RuntimeError):
    """A JSON-RPC error object came back for a request."""


class McpToolError(RuntimeError):
    """A `tools/call` succeeded at the protocol level but the tool reported `isError: true`."""


class EkosMcpClient:
    """One connection to one `ekos mcp serve --tcp`. Concurrent callers are serialised on an
    internal `asyncio.Lock` (one request/response on the wire at a time). The
    :class:`~app.supervisor.McpSupervisor` owns one client per workspace."""

    def __init__(self, host: str, port: int, token: str | None = None, *, timeout: float = 30.0):
        self._host = host
        self._port = port
        self._token = token or None
        self._timeout = timeout
        self._reader: asyncio.StreamReader | None = None
        self._writer: asyncio.StreamWriter | None = None
        self._next_id = 0
        # One outstanding request at a time per connection — concurrent callers serialize here
        # rather than racing to read each other's response line.
        self._io_lock = asyncio.Lock()

    # ── lifecycle ────────────────────────────────────────────────────────────

    async def connect(self) -> None:
        """Open the socket; run the MCP handshake (`initialize` + `notifications/initialized`)."""
        self._reader, self._writer = await asyncio.wait_for(
            asyncio.open_connection(self._host, self._port), self._timeout
        )
        params: dict[str, Any] = {"protocolVersion": "2025-06-18", "capabilities": {}}
        if self._token:
            params["_meta"] = {"token": self._token}
        await self._request("initialize", params)
        await self._notify("notifications/initialized")

    async def aclose(self) -> None:
        if self._writer is not None:
            self._writer.close()
            try:
                await self._writer.wait_closed()
            except (ConnectionError, OSError):
                pass
        self._reader = self._writer = None

    async def __aenter__(self) -> EkosMcpClient:
        await self.connect()
        return self

    async def __aexit__(self, *_exc: object) -> None:
        await self.aclose()

    # ── public API ───────────────────────────────────────────────────────────

    async def list_tools(self) -> list[dict[str, Any]]:
        result = await self._request_with_retry("tools/list", {})
        return list(result.get("tools", []))

    async def call_tool(self, name: str, arguments: dict[str, Any] | None = None) -> Any:
        """Call an MCP tool and return its parsed result.

        This server returns tool output as ``{"content": [{"type": "text", "text": "<json>"}],
        "isError": bool}`` (``mcp.rs::tool_ok``). The text payload is itself a JSON document; it
        is parsed and returned. ``isError: true`` raises :class:`McpToolError`.
        """
        result = await self._request_with_retry(
            "tools/call", {"name": name, "arguments": arguments or {}}
        )
        blocks = result.get("content") or []
        text = next((b.get("text", "") for b in blocks if b.get("type") == "text"), "")
        if result.get("isError"):
            raise McpToolError(text or f"tool {name} reported an error")
        try:
            return json.loads(text)
        except json.JSONDecodeError:
            return text

    # ── transport ────────────────────────────────────────────────────────────

    async def _request_with_retry(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        async with self._io_lock:
            try:
                return await self._request(method, params)
            except (ConnectionError, asyncio.IncompleteReadError, OSError):
                await self.aclose()
                await self.connect()
                return await self._request(method, params)

    async def _request(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        self._next_id += 1
        req_id = self._next_id
        await self._send({"jsonrpc": "2.0", "id": req_id, "method": method, "params": params})
        while True:
            msg = await self._recv()
            if msg.get("id") != req_id:
                continue  # notifications / out-of-band; ignore
            if "error" in msg:
                err = msg["error"]
                raise McpError(f"{err.get('code')}: {err.get('message')}")
            return msg.get("result", {})

    async def _notify(self, method: str) -> None:
        await self._send({"jsonrpc": "2.0", "method": method})

    async def _send(self, obj: dict[str, Any]) -> None:
        if self._writer is None:
            raise ConnectionError("client is not connected")
        self._writer.write((json.dumps(obj) + "\n").encode())
        await self._writer.drain()

    async def _recv(self) -> dict[str, Any]:
        if self._reader is None:
            raise ConnectionError("client is not connected")
        line = await asyncio.wait_for(self._reader.readline(), self._timeout)
        if not line:
            raise asyncio.IncompleteReadError(b"", None)
        return json.loads(line)
