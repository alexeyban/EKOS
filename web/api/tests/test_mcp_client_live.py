"""The Python client against a real `ekos mcp serve --tcp` process (RFC 0128 §2, RFC 0127 §13).

Skipped unless $EKOS_BIN points at a built `ekos` binary.
"""

from __future__ import annotations

import asyncio

import pytest

from app.mcp_client import EkosMcpClient, McpError


async def test_lists_and_calls_graph_export_over_a_real_socket(mcp_server):
    host, port, token = mcp_server
    async with EkosMcpClient(host, port, token=token) as client:
        names = {t["name"] for t in await client.list_tools()}
        assert "ekos_graph_export" in names
        assert "ekos_status" in names

        graph = await client.call_tool("ekos_graph_export", {"level": "aggregate"})
        assert graph["schema_version"] == 1
        assert graph["level"] == "aggregate"
        assert "counts" in graph and "nodes" in graph


async def test_wrong_token_is_rejected_by_the_real_server(mcp_server):
    host, port, _token = mcp_server
    with pytest.raises((McpError, ConnectionError, asyncio.IncompleteReadError, OSError)):
        await EkosMcpClient(host, port, token="not-the-token").connect()
