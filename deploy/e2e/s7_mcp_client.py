"""The gateway spoken to by the MCP project's own Python client, not by this suite's HTTP calls."""

import asyncio
import sys

from mcp import Client
from mcp.client.streamable_http import create_mcp_http_client, streamable_http_client

from lib import BASE, check, sign_in, summary

alice = sign_in("alice@example.com")
token = alice.post("/api/tenants/acme/tokens", json={"name": "sdk-client", "role": "viewer"}).json()["token"]


async def session(url: str, bearer: str | None):
    headers = {"Authorization": f"Bearer {bearer}"} if bearer else {}
    async with create_mcp_http_client(headers=headers) as http:
        async with Client(streamable_http_client(url, http_client=http)) as client:
            tools = await client.list_tools()
            names = {t.name for t in tools.tools}
            found = await client.call_tool("ekos_search", {"query": "compensation bands", "limit": 10})
            refused = await client.call_tool("ekos_identity_review", {"id": "x", "decision": "confirm"})
            return client.server_info, client.protocol_version, names, found, refused


async def main() -> None:
    info, version, names, found, refused = await session(BASE + "/mcp/acme/shop", token)
    print("   server:", info.name, info.version, "protocol:", version)
    check(info.name == "ekos-gateway", "the client completes its handshake with the gateway")
    check("ekos_search" in names and "ekos_identity_review" not in names, f"it lists {len(names)} tools, no review tool")
    text = " ".join(getattr(c, "text", "") for c in found.content)
    check(not found.is_error and "pay.md" in text, "its search call returns the pay page", text[:200])
    check(refused.is_error, "its review call comes back as a tool error")
    for label, url, bearer in (("without a token", "/mcp/acme/shop", None), ("on another tenant's project", "/mcp/globex/billing", token)):
        try:
            await session(BASE + url, bearer)
            check(False, f"the client {label} is refused")
        except BaseException as exc:  # the client raises a group of transport errors
            check(True, f"the client {label} is refused ({type(exc).__name__})")


asyncio.run(main())
sys.exit(summary())
