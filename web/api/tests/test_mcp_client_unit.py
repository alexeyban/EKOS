"""Framing / handshake / id-monotonicity against a scripted in-process MCP server — no `ekos`
binary needed."""

from __future__ import annotations

import asyncio
import json

import pytest

from app.mcp_client import EkosMcpClient, McpError, McpToolError


class FakeMcpServer:
    """A minimal NDJSON JSON-RPC server that records requests and replies from a script."""

    def __init__(self, *, require_token: str | None = None):
        self.require_token = require_token
        self.requests: list[dict] = []
        self._server: asyncio.AbstractServer | None = None
        self.port = 0

    async def start(self) -> None:
        self._server = await asyncio.start_server(self._handle, "127.0.0.1", 0)
        self.port = self._server.sockets[0].getsockname()[1]

    async def stop(self) -> None:
        if self._server is not None:
            self._server.close()
            await self._server.wait_closed()

    async def _handle(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        authed = self.require_token is None
        while True:
            line = await reader.readline()
            if not line:
                break
            msg = json.loads(line)
            self.requests.append(msg)
            method, mid = msg.get("method"), msg.get("id")

            if not authed:
                token = msg.get("params", {}).get("_meta", {}).get("token")
                if method != "initialize" or token != self.require_token:
                    writer.write(
                        json.dumps(
                            {
                                "jsonrpc": "2.0",
                                "id": None,
                                "error": {"code": -32001, "message": "unauthorized"},
                            }
                        ).encode()
                        + b"\n"
                    )
                    await writer.drain()
                    break
                authed = True

            if method == "initialize":
                reply = {"protocolVersion": "2025-06-18", "serverInfo": {"name": "fake"}}
            elif method == "notifications/initialized":
                continue  # notification, no reply
            elif method == "tools/list":
                reply = {"tools": [{"name": "ekos_graph_export"}, {"name": "ekos_status"}]}
            elif method == "tools/call":
                name = msg["params"]["name"]
                if name == "boom":
                    reply = {"content": [{"type": "text", "text": "kaboom"}], "isError": True}
                else:
                    reply = {
                        "content": [{"type": "text", "text": json.dumps({"echo": name})}],
                        "isError": False,
                    }
            else:
                writer.write(
                    json.dumps(
                        {
                            "jsonrpc": "2.0",
                            "id": mid,
                            "error": {"code": -32601, "message": "method not found"},
                        }
                    ).encode()
                    + b"\n"
                )
                await writer.drain()
                continue

            writer.write(
                json.dumps({"jsonrpc": "2.0", "id": mid, "result": reply}).encode() + b"\n"
            )
            await writer.drain()


async def test_handshake_and_tool_calls_round_trip():
    server = FakeMcpServer()
    await server.start()
    try:
        async with EkosMcpClient("127.0.0.1", server.port) as client:
            tools = await client.list_tools()
            assert {t["name"] for t in tools} == {"ekos_graph_export", "ekos_status"}
            assert await client.call_tool("ekos_status") == {"echo": "ekos_status"}
    finally:
        await server.stop()

    methods = [r["method"] for r in server.requests]
    assert methods[:2] == ["initialize", "notifications/initialized"]
    ids = [
        r["id"] for r in server.requests if "id" in r and r["method"] != "notifications/initialized"
    ]
    assert ids == sorted(ids) and len(ids) == len(set(ids)), "request ids are monotonic and unique"


async def test_tool_error_is_raised():
    server = FakeMcpServer()
    await server.start()
    try:
        async with EkosMcpClient("127.0.0.1", server.port) as client:
            with pytest.raises(McpToolError, match="kaboom"):
                await client.call_tool("boom")
    finally:
        await server.stop()


async def test_unknown_method_surfaces_as_mcp_error():
    server = FakeMcpServer()
    await server.start()
    try:
        client = EkosMcpClient("127.0.0.1", server.port)
        await client.connect()
        with pytest.raises(McpError, match="-32601"):
            await client._request_with_retry("nope/nope", {})
        await client.aclose()
    finally:
        await server.stop()


async def test_token_is_sent_in_the_initialize_meta():
    server = FakeMcpServer(require_token="hunter2")
    await server.start()
    try:
        async with EkosMcpClient("127.0.0.1", server.port, token="hunter2") as client:
            assert await client.list_tools()
    finally:
        await server.stop()
    assert server.requests[0]["params"]["_meta"]["token"] == "hunter2"


async def test_wrong_token_fails_to_connect():
    server = FakeMcpServer(require_token="hunter2")
    await server.start()
    try:
        with pytest.raises((McpError, ConnectionError, asyncio.IncompleteReadError)):
            await EkosMcpClient("127.0.0.1", server.port, token="wrong").connect()
    finally:
        await server.stop()
