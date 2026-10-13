"""RFC 0182: the MCP gateway. The project's server is a recorder here, so what is checked is
what the gateway lets through, what it attaches, and what it writes down. The real server is in
`test_projects_live.py`."""

# ruff: noqa: F811 — the fixtures imported from test_tenants are requested by name below.
from __future__ import annotations

import json
import re
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app import mcp_policy
from app.mcp_client import McpToolError

# Reuses the two-tenant world and sign-in helpers of the RFC 0180 tests.
from tests.test_tenants import As, client, env, world  # noqa: F401  # noqa: F401

URL = "/mcp/acme/api"

TOOLS = [
    "ekos_search",
    "ekos_query",
    "ekos_ekl",
    "ekos_state",
    "ekos_status",
    "ekos_conflicts",
    "ekos_session_note",
    "ekos_identity_review",
    "ekos_architecture_review",
]


class Backend:
    """Stands in for the connection to `ekos mcp serve`."""

    def __init__(self) -> None:
        self.calls: list[tuple[str, dict, dict | None]] = []
        self.listed = 0

    async def list_tools(self):
        self.listed += 1
        return [{"name": name, "description": "", "inputSchema": {}} for name in TOOLS]

    async def call_tool_raw(self, name, arguments=None, *, scope=None):
        self.calls.append((name, arguments or {}, scope))
        if name == "ekos_state":
            raise McpToolError("object not found: x")
        return {"content": [{"type": "text", "text": json.dumps({"tool": name})}], "isError": False}


@pytest.fixture
def backend(client: TestClient) -> Backend:
    b = Backend()

    async def acquire(_ws):
        return b

    client.app.state.supervisor.acquire = acquire
    return b


def rpc(who: As, method: str, params: dict | None = None, *, url: str = URL, msg_id=1, **kw):
    body = {"jsonrpc": "2.0", "id": msg_id, "method": method}
    if params is not None:
        body["params"] = params
    return who.request("POST", url, json=body, **kw)


def token(world, client, role: str = "viewer", **extra) -> tuple[As, dict]:
    made = world["ann"].post("/api/tenants/acme/tokens", {"name": "agent", "role": role, **extra})
    assert made.status_code == 201, made.text
    return As(client, bearer=made.json()["token"]), made.json()


def names(response) -> list[str]:
    return [t["name"] for t in response.json()["result"]["tools"]]


# ── protocol ─────────────────────────────────────────────────────────────────


def test_initialize_and_ping_are_answered_by_the_gateway(world, client, backend) -> None:
    bot, _ = token(world, client)
    r = rpc(bot, "initialize", {"protocolVersion": "2025-03-26", "capabilities": {}})
    assert r.status_code == 200
    result = r.json()["result"]
    assert result["protocolVersion"] == "2025-03-26"
    assert result["capabilities"] == {"tools": {}}
    assert result["serverInfo"]["name"] == "ekos-gateway"
    assert rpc(bot, "initialize", {}).json()["result"]["protocolVersion"] == "2025-06-18"
    assert rpc(bot, "ping").json() == {"jsonrpc": "2.0", "id": 1, "result": {}}
    assert backend.calls == [] and backend.listed == 0


def test_a_notification_is_accepted_and_not_forwarded(world, client, backend) -> None:
    bot, _ = token(world, client)
    r = bot.request("POST", URL, json={"jsonrpc": "2.0", "method": "notifications/initialized"})
    assert r.status_code == 202 and r.content == b""
    # A tool call sent as a notification is not run either.
    r = bot.request(
        "POST", URL, json={"jsonrpc": "2.0", "method": "tools/call", "params": {"name": "ekos_ekl"}}
    )
    assert r.status_code == 202 and backend.calls == []


def test_a_batch_is_answered_in_order(world, client, backend) -> None:
    bot, _ = token(world, client)
    batch = [
        {"jsonrpc": "2.0", "id": "a", "method": "ping"},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": "b", "method": "tools/call", "params": {"name": "ekos_search"}},
    ]
    r = bot.request("POST", URL, json=batch)
    assert [m["id"] for m in r.json()] == ["a", "b"]
    assert bot.request("POST", URL, json=[]).status_code == 400
    assert bot.request("POST", URL, json=[batch[0]] * 51).status_code == 400


def test_sse_when_the_client_asks_for_it(world, client, backend) -> None:
    bot, _ = token(world, client)
    bot._headers["Accept"] = "application/json, text/event-stream"
    r = rpc(bot, "ping")
    assert r.headers["content-type"].startswith("text/event-stream")
    assert r.text == 'event: message\ndata: {"jsonrpc":"2.0","id":1,"result":{}}\n\n'
    bot._headers["Accept"] = "*/*"
    assert rpc(bot, "ping").headers["content-type"].startswith("application/json")


def test_bad_requests(world, client, backend) -> None:
    bot, _ = token(world, client)
    r = bot.request("POST", URL, content=b"{not json")
    assert r.status_code == 400 and r.json()["error"]["code"] == -32700
    assert rpc(bot, "resources/list").json()["error"]["code"] == -32601
    assert (
        bot.request("POST", URL, json={"id": 1, "method": "ping"}).json()["error"]["code"] == -32600
    )
    assert rpc(bot, "tools/call", {"arguments": {}}).json()["error"]["code"] == -32602
    bad = rpc(bot, "tools/call", {"name": "ekos_ekl", "arguments": "FIND Object"})
    assert bad.json()["error"]["code"] == -32602
    assert bot.request("GET", URL).status_code == 405
    assert bot.request("DELETE", URL).status_code == 405


# ── who reaches what ─────────────────────────────────────────────────────────


def test_no_credential_is_401_and_an_invisible_project_is_404(world, client, backend) -> None:
    anon = As(client)
    assert rpc(anon, "ping").status_code == 401
    assert rpc(As(client, bearer="ekos_" + "x" * 43), "ping").status_code == 401

    bot, _ = token(world, client)
    foreign = rpc(bot, "ping", url="/mcp/globex/api")
    missing = rpc(bot, "ping", url="/mcp/acme/no-such")
    other_tenant_same_id = rpc(bot, "ping", url="/mcp/default/acme.api")
    assert foreign.status_code == missing.status_code == other_tenant_same_id.status_code == 404
    assert rpc(world["gus"], "ping").status_code == 404
    assert backend.calls == []


def test_a_viewer_token_sees_and_calls_only_read_tools(world, client, backend) -> None:
    bot, _ = token(world, client)
    listed = names(rpc(bot, "tools/list"))
    assert listed == [
        "ekos_search",
        "ekos_query",
        "ekos_ekl",
        "ekos_state",
        "ekos_status",
        "ekos_conflicts",
    ]
    ok = rpc(bot, "tools/call", {"name": "ekos_search", "arguments": {"query": "orders"}})
    assert ok.json()["result"]["isError"] is False
    assert backend.calls == [("ekos_search", {"query": "orders"}, None)]

    for tool in ["ekos_session_note", "ekos_identity_review", "ekos_architecture_review"]:
        r = rpc(bot, "tools/call", {"name": tool, "arguments": {}})
        result = r.json()["result"]
        assert result["isError"] is True and "needs the right to" in result["content"][0]["text"]
    assert len(backend.calls) == 1


def test_an_operator_token_may_write_a_note_and_still_cannot_review(world, client, backend) -> None:
    bot, _ = token(world, client, role="operator")
    assert "ekos_session_note" in names(rpc(bot, "tools/list"))
    assert "ekos_identity_review" not in names(rpc(bot, "tools/list"))
    assert (
        rpc(bot, "tools/call", {"name": "ekos_session_note"}).json()["result"]["isError"] is False
    )
    r = rpc(bot, "tools/call", {"name": "ekos_identity_review"})
    assert r.json()["result"]["isError"] is True


def test_a_signed_in_reviewer_reaches_the_review_tools(world, client, backend) -> None:
    listed = names(rpc(world["rita"], "tools/list"))
    assert "ekos_identity_review" in listed and "ekos_session_note" not in listed
    r = rpc(world["rita"], "tools/call", {"name": "ekos_identity_review", "arguments": {"id": "x"}})
    assert r.json()["result"]["isError"] is False


def test_a_tool_failure_comes_back_as_a_tool_error(world, client, backend) -> None:
    bot, _ = token(world, client)
    r = rpc(bot, "tools/call", {"name": "ekos_state", "arguments": {"id": "x"}})
    assert r.status_code == 200
    assert r.json()["result"] == {
        "content": [{"type": "text", "text": "object not found: x"}],
        "isError": True,
    }


# ── scope ────────────────────────────────────────────────────────────────────


def _restrict(world, made: dict, **rule) -> dict:
    body = {"token_id": made["id"], "role": "viewer", "workspace_id": "acme.api", **rule}
    r = world["ann"].post("/api/tenants/acme/bindings", body)
    assert r.status_code == 201, r.text
    return rule


def test_a_restricted_token_gets_the_scoped_tools_and_its_scope_on_every_call(
    world,
    client,
    backend,
) -> None:
    bot, made = token(world, client, workspace_id="acme.api")
    _restrict(world, made, effect="deny", source="payments", path="secrets/**")
    expected = {
        "default": "allow",
        "rules": [{"effect": "deny", "source": "payments", "path": "secrets/**"}],
    }

    assert names(rpc(bot, "tools/list")) == ["ekos_search", "ekos_query", "ekos_ekl", "ekos_state"]
    rpc(bot, "tools/call", {"name": "ekos_ekl", "arguments": {"query": "FIND Object"}})
    assert backend.calls == [("ekos_ekl", {"query": "FIND Object"}, expected)]

    for tool in ["ekos_status", "ekos_conflicts", "ekos_session_note", "something_new"]:
        r = rpc(bot, "tools/call", {"name": tool})
        assert r.json()["result"]["isError"] is True, tool
        assert "restricted access" in r.json()["result"]["content"][0]["text"], tool
    assert len(backend.calls) == 1


def test_a_scope_sent_by_the_client_is_dropped(world, client, backend) -> None:
    wide_open = {"ekos/readScope": {"default": "allow"}, "token": "x"}

    # An unrestricted caller: the forwarded call carries no scope, whatever they sent.
    bot, _ = token(world, client)
    rpc(bot, "tools/call", {"name": "ekos_search", "arguments": {"query": "a"}, "_meta": wide_open})
    assert backend.calls[-1] == ("ekos_search", {"query": "a"}, None)

    # A restricted caller: theirs is the computed one, not the one they sent.
    limited, made = token(world, client, workspace_id="acme.api")
    _restrict(world, made, effect="deny", source="payments")
    rpc(
        limited,
        "tools/call",
        {
            "name": "ekos_search",
            "arguments": {"query": "a", "_meta": wide_open},
            "_meta": wide_open,
        },
    )
    _name, arguments, scope = backend.calls[-1]
    assert scope == {"default": "allow", "rules": [{"effect": "deny", "source": "payments"}]}
    # `_meta` inside the arguments is the tool's business (it is an unknown argument there),
    # not a scope: it was passed as an argument and nowhere else.
    assert arguments == {"query": "a", "_meta": wide_open}


def test_the_scoped_tool_list_matches_the_server() -> None:
    """The gateway's list and the one `ekos mcp serve` enforces are the same list."""
    source = Path(__file__).resolve().parents[3] / "ekos/crates/cli/src/commands/mcp.rs"
    text = source.read_text()
    block = re.search(r"pub const SCOPED_TOOLS: &\[&str\] = &\[(.*?)\];", text, re.S)
    assert block, "SCOPED_TOOLS not found in commands/mcp.rs"
    in_rust = set(re.findall(r'"(ekos_[a-z_]+)"', block.group(1)))
    assert len(in_rust) >= 10
    assert in_rust == set(mcp_policy.SCOPED_TOOLS)
    # The review tools named here exist there, and are refused under a scope there too.
    refused = re.search(r"pub const SCOPE_REFUSED_TOOLS: .*? = &\[(.*?)\n\];", text, re.S)
    assert refused, "SCOPE_REFUSED_TOOLS not found in commands/mcp.rs"
    for tool in mcp_policy.REVIEW_TOOLS | mcp_policy.OPERATE_TOOLS:
        assert f'"{tool}"' in refused.group(1), tool


# ── audit ────────────────────────────────────────────────────────────────────


def test_every_tool_call_is_recorded_without_its_arguments(world, client, backend) -> None:
    bot, _ = token(world, client)
    rpc(bot, "tools/call", {"name": "ekos_search", "arguments": {"query": "zebra-secret-query"}})
    rpc(bot, "tools/call", {"name": "ekos_identity_review", "arguments": {"id": "x"}})
    rpc(bot, "tools/list")
    rpc(bot, "ping")

    log = world["ann"].get("/api/tenants/acme/audit").json()
    mcp_entries = [e for e in log if e["action"].startswith("mcp: ")]
    assert sorted((e["action"], e["outcome"], e["workspace_id"]) for e in mcp_entries) == [
        ("mcp: ekos_identity_review", "denied", "acme.api"),
        ("mcp: ekos_search", "allowed", "acme.api"),
    ]
    assert all(e["actor"].startswith("token:agent#") for e in mcp_entries)
    assert "zebra-secret-query" not in json.dumps(log)
