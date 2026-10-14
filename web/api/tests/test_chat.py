"""RFC 0187: chat with a configured model using the EKOS tools."""

from __future__ import annotations

import json
from collections.abc import Iterator
from pathlib import Path
from typing import Any

import pytest
from fastapi.testclient import TestClient

from app import models
from app.chat import agent, providers
from app.main import create_app

OWNER = {"Authorization": "Bearer w-tok"}
READER = {"Authorization": "Bearer r-tok"}
TOOLS = [
    {"name": "ekos_search", "description": "search", "inputSchema": {"type": "object"}},
    {"name": "ekos_state", "description": "state", "inputSchema": {"type": "object"}},
    {"name": "ekos_identity_review", "description": "review", "inputSchema": {"type": "object"}},
    {"name": "ekos_architecture_review", "description": "review", "inputSchema": {}},
    {"name": "ekos_session_note", "description": "note", "inputSchema": {"type": "object"}},
    {"name": "ekos_status", "description": "status", "inputSchema": {"type": "object"}},
]


# ── providers: request and response shapes ─────────────────────────────────

CONVERSATION = [
    {"role": "user", "content": "what is customers?"},
    {
        "role": "assistant",
        "content": "",
        "tool_calls": [{"id": "c1", "name": "ekos_search", "arguments": {"query": "customers"}}],
    },
    {"role": "tool", "tool_call_id": "c1", "name": "ekos_search", "content": "1 hit: t1"},
]
SPEC = [{"name": "ekos_search", "description": "search", "input_schema": {"type": "object"}}]


def test_anthropic_shapes() -> None:
    p = providers.Anthropic(model="claude-x", base_url="", key="k", max_tokens=100)
    url, headers, body = p.request("be brief", CONVERSATION, SPEC)
    assert url == "https://api.anthropic.com/v1/messages"
    assert headers["x-api-key"] == "k" and "anthropic-version" in headers
    assert body["system"] == "be brief" and body["tools"][0]["input_schema"] == {"type": "object"}
    assert body["messages"][1]["content"][0] == {
        "type": "tool_use",
        "id": "c1",
        "name": "ekos_search",
        "input": {"query": "customers"},
    }
    assert body["messages"][2] == {
        "role": "user",
        "content": [{"type": "tool_result", "tool_use_id": "c1", "content": "1 hit: t1"}],
    }
    reply = p.parse(
        {
            "content": [
                {"type": "text", "text": "Looking."},
                {"type": "tool_use", "id": "c2", "name": "ekos_state", "input": {"id": "t1"}},
            ],
            "usage": {"input_tokens": 10, "output_tokens": 5},
        }
    )
    assert reply.text == "Looking."
    assert reply.tool_calls == [providers.ToolCall("c2", "ekos_state", {"id": "t1"})]
    assert reply.usage == {"input": 10, "output": 5}


def test_openai_shapes() -> None:
    p = providers.OpenAI(model="gpt-x", base_url="http://127.0.0.1:1234/v1", key="", max_tokens=9)
    url, headers, body = p.request("sys", CONVERSATION, SPEC)
    assert url == "http://127.0.0.1:1234/v1/chat/completions"
    assert "Authorization" not in headers  # no key: a local server
    assert body["messages"][0] == {"role": "system", "content": "sys"}
    call = body["messages"][2]["tool_calls"][0]
    assert call["function"] == {"name": "ekos_search", "arguments": '{"query": "customers"}'}
    assert body["messages"][3] == {"role": "tool", "tool_call_id": "c1", "content": "1 hit: t1"}
    assert body["tools"][0]["function"]["parameters"] == {"type": "object"}
    reply = p.parse(
        {
            "choices": [
                {
                    "message": {
                        "content": None,
                        "tool_calls": [
                            {
                                "id": "c3",
                                "function": {"name": "ekos_state", "arguments": '{"id": "t1"}'},
                            }
                        ],
                    }
                }
            ],
            "usage": {"prompt_tokens": 3, "completion_tokens": 4},
        }
    )
    assert reply.tool_calls == [providers.ToolCall("c3", "ekos_state", {"id": "t1"})]
    assert reply.text == "" and reply.usage == {"input": 3, "output": 4}


def test_ollama_shapes() -> None:
    p = providers.Ollama(model="qwen2.5:1.5b", base_url="", key="", max_tokens=50)
    url, _, body = p.request("sys", CONVERSATION, SPEC)
    assert url == "http://127.0.0.1:11434/api/chat"
    assert body["stream"] is False and body["options"]["num_predict"] == 50
    assert body["messages"][2]["tool_calls"][0]["function"]["arguments"] == {"query": "customers"}
    reply = p.parse(
        {
            "message": {
                "content": "",
                "tool_calls": [{"function": {"name": "ekos_search", "arguments": {"query": "x"}}}],
            },
            "prompt_eval_count": 7,
            "eval_count": 2,
        }
    )
    assert reply.tool_calls[0].name == "ekos_search" and reply.tool_calls[0].id
    assert reply.usage == {"input": 7, "output": 2}


@pytest.mark.parametrize(
    ("provider", "base", "metered"),
    [
        ("anthropic", "", True),
        ("openai", "", True),
        ("openai", "http://127.0.0.1:1234/v1", False),
        ("openai", "http://localhost:8080/v1", False),
        ("ollama", "", False),
        ("ollama", "http://gpu-box:11434", False),
    ],
)
def test_what_is_paid_per_call(provider: str, base: str, metered: bool) -> None:
    assert providers.is_metered(provider, base) is metered


# ── the loop ─────────────────────────────────────────────────────────────────


class Scripted:
    """A model that answers from a script, and records what it was sent."""

    name = "scripted"

    def __init__(self, replies: list[providers.Reply]) -> None:
        self.replies = list(replies)
        self.seen: list[dict[str, Any]] = []

    async def complete(self, system, messages, tools):
        self.seen.append(
            {"system": system, "messages": json.loads(json.dumps(messages)), "tools": tools}
        )
        return self.replies.pop(0)


def _r(text="", calls=()):
    return providers.Reply(text, [providers.ToolCall(*c) for c in calls], {"input": 1, "output": 1})


async def test_a_turn_calls_tools_until_the_model_answers() -> None:
    model = Scripted(
        [
            _r(calls=[("a", "ekos_search", {"query": "customers"})]),
            _r(calls=[("b", "ekos_state", {"id": "t1"})]),
            _r("customers is a table [t1]."),
        ]
    )
    called: list[str] = []

    async def call(name, args):
        called.append(name)
        return f"result of {name}", False

    messages = [{"role": "user", "content": "what is customers?"}]
    events = [e async for e in agent.turn(model, "sys", messages, SPEC, call, max_calls=5)]
    kinds = [e["type"] for e in events]
    assert kinds == [
        "usage", "tool_call", "tool_result",
        "usage", "tool_call", "tool_result",
        "usage", "text",
    ]  # fmt: skip
    assert called == ["ekos_search", "ekos_state"]
    assert events[-1]["text"] == "customers is a table [t1]."
    assert messages[-1] == {"role": "assistant", "content": "customers is a table [t1]."}
    assert model.seen[1]["messages"][-1]["role"] == "tool"


async def test_the_tool_call_limit_stops_the_turn() -> None:
    model = Scripted([_r(calls=[(str(i), "ekos_search", {})]) for i in range(5)])

    async def call(name, args):
        return "x", False

    events = [e async for e in agent.turn(model, "s", [], SPEC, call, max_calls=2)]
    assert sum(e["type"] == "tool_call" for e in events) == 2
    assert events[-1]["type"] == "text" and "limit" in events[-1]["text"]


async def test_a_long_result_is_cut() -> None:
    model = Scripted([_r(calls=[("a", "ekos_search", {})]), _r("ok")])

    async def call(name, args):
        return "x" * 50_000, False

    messages: list[dict] = []
    _ = [e async for e in agent.turn(model, "s", messages, SPEC, call, max_calls=3)]
    tool_msg = next(m for m in messages if m["role"] == "tool")
    assert len(tool_msg["content"]) < 21_000 and "cut" in tool_msg["content"]


def test_the_chat_never_names_a_review_tool() -> None:
    """Review decisions are human-only (RFC 0180); the chat is an agent."""
    src = "".join(p.read_text() for p in (Path(__file__).parents[1] / "app" / "chat").glob("*.py"))
    src += (Path(__file__).parents[1] / "app" / "routes" / "chat.py").read_text()
    for name in ("ekos_identity_review", "ekos_architecture_review", "semantics confirm"):
        assert name not in src


# ── the route ────────────────────────────────────────────────────────────────


class FakeMcp:
    def __init__(self) -> None:
        self.calls: list[tuple[str, dict, Any]] = []

    async def list_tools(self):
        return TOOLS

    async def call_tool_raw(self, name, arguments=None, *, scope=None):
        self.calls.append((name, arguments or {}, scope))
        return {"content": [{"type": "text", "text": f"{name} says HIDDENMARKER-free text"}]}


@pytest.fixture
def chat_client(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, reset_settings: None
) -> Iterator[TestClient]:
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_TOKEN", "r-tok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_WRITE_TOKEN", "w-tok")
    monkeypatch.setenv("EKOS_CONSOLE_CONSOLE_DB", str(tmp_path / "c.db"))
    monkeypatch.setenv("EKOS_CONSOLE_SESSION_SECRET", "s")
    monkeypatch.setenv("EKOS_BIN", "/bin/true")
    monkeypatch.setenv("EKOS_CONSOLE_CHAT_PROVIDER", "ollama")
    monkeypatch.setenv("EKOS_CONSOLE_CHAT_MODEL", "qwen2.5:1.5b")
    ws = tmp_path / "ws"
    ws.mkdir()
    (ws / "ekos.toml").write_text("")
    fake = FakeMcp()
    model = Scripted(
        [_r(calls=[("a", "ekos_search", {"query": "customers"})]), _r("It is a table [t1].")]
    )
    with TestClient(create_app()) as c:
        models.add_workspace(models.Workspace(id="w", name="w", path=str(ws)))

        async def acquire(_ws):
            return fake

        monkeypatch.setattr(c.app.state.supervisor, "acquire", acquire)
        monkeypatch.setattr(providers, "from_settings", lambda settings: model)
        c.fake = fake  # type: ignore[attr-defined]
        c.model = model  # type: ignore[attr-defined]
        yield c


def _events(r) -> list[dict]:
    out = []
    for line in r.text.splitlines():
        if line.startswith("data: "):
            out.append(json.loads(line[6:]))
    return out


def test_a_chat_turn_streams_events_and_is_kept(chat_client: TestClient) -> None:
    r = chat_client.post("/api/workspaces/w/chat", headers=OWNER, json={"message": "customers?"})
    assert r.status_code == 200 and r.headers["content-type"].startswith("text/event-stream")
    events = _events(r)
    kinds = [e["type"] for e in events]
    assert kinds[0] == "conversation" and kinds[-1] == "done"
    assert "tool_call" in kinds and "tool_result" in kinds
    assert next(e for e in events if e["type"] == "text")["text"] == "It is a table [t1]."
    cid = events[0]["id"]

    # Only reading tools reach the model — the review and writing ones not even for an owner.
    sent = {t["name"] for t in chat_client.model.seen[0]["tools"]}  # type: ignore[attr-defined]
    assert sent == {"ekos_search", "ekos_state", "ekos_status"}

    # The tool was called through the gateway's path and audited without its arguments.
    assert chat_client.fake.calls[0][:2] == ("ekos_search", {"query": "customers"})  # type: ignore[attr-defined]
    audit = [a for a in models.list_audit("default") if a.action == "chat: ekos_search"]
    assert audit and "customers" not in audit[0].detail

    convs = chat_client.get("/api/workspaces/w/chat", headers=OWNER).json()
    assert [c["id"] for c in convs] == [cid] and convs[0]["title"].startswith("customers")
    msgs = chat_client.get(f"/api/workspaces/w/chat/{cid}", headers=OWNER).json()["messages"]
    assert [m["role"] for m in msgs] == ["user", "assistant", "tool", "assistant"]


def test_a_conversation_belongs_to_its_person(chat_client: TestClient) -> None:
    r = chat_client.post("/api/workspaces/w/chat", headers=OWNER, json={"message": "hi"})
    cid = _events(r)[0]["id"]
    assert chat_client.get("/api/workspaces/w/chat", headers=READER).json() == []
    assert chat_client.get(f"/api/workspaces/w/chat/{cid}", headers=READER).status_code == 404
    assert chat_client.delete(f"/api/workspaces/w/chat/{cid}", headers=READER).status_code == 404
    assert chat_client.delete(f"/api/workspaces/w/chat/{cid}", headers=OWNER).status_code == 200
    assert chat_client.get("/api/workspaces/w/chat", headers=OWNER).json() == []


def test_a_paid_model_needs_the_tenant_switch(
    chat_client: TestClient, monkeypatch: pytest.MonkeyPatch
) -> None:
    from app import settings as settings_mod

    s = chat_client.app.state.settings
    monkeypatch.setattr(s, "chat_provider", "anthropic")
    monkeypatch.setattr(settings_mod, "_settings", s)
    r = chat_client.post("/api/workspaces/w/chat", headers=OWNER, json={"message": "hi"})
    assert r.status_code == 409 and "metered" in r.text
    assert chat_client.model.seen == []  # type: ignore[attr-defined]
    models.update_tenant("default", allow_metered=True)
    r = chat_client.post("/api/workspaces/w/chat", headers=OWNER, json={"message": "hi"})
    assert r.status_code == 200


def test_no_model_configured(chat_client: TestClient, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(chat_client.app.state.settings, "chat_provider", "")
    info = chat_client.get("/api/chat/model", headers=READER).json()
    assert info["configured"] is False
    r = chat_client.post("/api/workspaces/w/chat", headers=OWNER, json={"message": "hi"})
    assert r.status_code == 409 and "no chat model" in r.text


def test_the_model_info(chat_client: TestClient) -> None:
    info = chat_client.get("/api/chat/model", headers=READER).json()
    assert info == {
        "configured": True,
        "provider": "ollama",
        "model": "qwen2.5:1.5b",
        "metered": False,
    }


def test_a_restricted_reader_chats_within_their_scope(chat_client: TestClient) -> None:
    """RFC 0181: the person's read scope goes with every tool call, and only the scoped tools
    are offered."""
    from app import auth

    t, h, p = auth.new_token()
    models.add(models.ApiToken(id="t1", tenant_id="default", name="bot", token_hash=h, prefix=p))
    models.add(
        models.RoleBinding(
            id="b1",
            tenant_id="default",
            principal_kind="token",
            principal_id="t1",
            role="viewer",
            workspace_id="w",
            source="api",
        )
    )
    r = chat_client.post(
        "/api/workspaces/w/chat",
        headers={"Authorization": f"Bearer {t}"},
        json={"message": "customers?"},
    )
    assert r.status_code == 200, r.text
    sent = {x["name"] for x in chat_client.model.seen[0]["tools"]}  # type: ignore[attr-defined]
    assert sent == {"ekos_search", "ekos_state"}  # ekos_status is not a scoped tool
    _, _, scope = chat_client.fake.calls[0]  # type: ignore[attr-defined]
    assert scope is not None and any(rule.get("source") == "api" for rule in scope["rules"])
