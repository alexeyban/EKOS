"""Chat model providers (RFC 0187 §1), called with `httpx` directly.

One internal conversation shape, converted per provider:

    {"role": "user", "content": str}
    {"role": "assistant", "content": str, "tool_calls": [{"id", "name", "arguments"}]}
    {"role": "tool", "tool_call_id": str, "name": str, "content": str}

and tools as `{"name", "description", "input_schema"}`. `request` and `parse` are pure, so each
provider's wire shape is tested without a network; `complete` sends.

A key is read from the console's environment or the keychain by name, never from a file, and
never logged.
"""

from __future__ import annotations

import ipaddress
import json
import os
import uuid
from dataclasses import dataclass, field
from typing import Any
from urllib.parse import urlparse

import httpx

from .. import keychain


@dataclass(frozen=True)
class ToolCall:
    id: str
    name: str
    arguments: dict[str, Any]


@dataclass
class Reply:
    text: str
    tool_calls: list[ToolCall] = field(default_factory=list)
    usage: dict[str, int] = field(default_factory=dict)


class ProviderError(RuntimeError):
    pass


def _local(base_url: str) -> bool:
    host = urlparse(base_url).hostname or ""
    if host == "localhost":
        return True
    try:
        return ipaddress.ip_address(host).is_loopback
    except ValueError:
        return False


def is_metered(provider: str, base_url: str) -> bool:
    """Paid per call: anything but Ollama or a server on this machine."""
    if provider == "ollama":
        return False
    return not (base_url and _local(base_url))


@dataclass
class _Base:
    model: str
    base_url: str
    key: str
    max_tokens: int
    timeout: float = 180.0

    name = "base"

    def request(
        self, system: str, messages: list[dict], tools: list[dict]
    ) -> tuple[str, dict, dict]:
        raise NotImplementedError

    def parse(self, body: dict) -> Reply:
        raise NotImplementedError

    async def complete(self, system: str, messages: list[dict], tools: list[dict]) -> Reply:
        url, headers, payload = self.request(system, messages, tools)
        try:
            async with httpx.AsyncClient(timeout=self.timeout) as client:
                r = await client.post(url, headers=headers, json=payload)
        except httpx.HTTPError as exc:
            raise ProviderError(f"{self.name}: {type(exc).__name__} reaching {url}") from exc
        if r.status_code >= 400:
            raise ProviderError(f"{self.name} answered {r.status_code}: {r.text[:300]}")
        return self.parse(r.json())


class Anthropic(_Base):
    name = "anthropic"

    def request(self, system, messages, tools):
        out: list[dict] = []
        for m in messages:
            if m["role"] == "user":
                out.append({"role": "user", "content": m["content"]})
            elif m["role"] == "assistant":
                blocks: list[dict] = []
                if m.get("content"):
                    blocks.append({"type": "text", "text": m["content"]})
                for c in m.get("tool_calls", []):
                    blocks.append(
                        {
                            "type": "tool_use",
                            "id": c["id"],
                            "name": c["name"],
                            "input": c["arguments"],
                        }
                    )
                out.append({"role": "assistant", "content": blocks})
            elif m["role"] == "tool":
                block = {
                    "type": "tool_result",
                    "tool_use_id": m["tool_call_id"],
                    "content": m["content"],
                }
                # Results of one assistant turn go back together in one user message.
                if out and out[-1]["role"] == "user" and isinstance(out[-1]["content"], list):
                    out[-1]["content"].append(block)
                else:
                    out.append({"role": "user", "content": [block]})
        body = {
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": system,
            "messages": out,
            "tools": [
                {
                    "name": t["name"],
                    "description": t["description"],
                    "input_schema": t["input_schema"],
                }
                for t in tools
            ],
        }
        url = (self.base_url or "https://api.anthropic.com").rstrip("/") + "/v1/messages"
        headers = {"x-api-key": self.key, "anthropic-version": "2023-06-01"}
        return url, headers, body

    def parse(self, body):
        text = "".join(
            b.get("text", "") for b in body.get("content", []) if b.get("type") == "text"
        )
        calls = [
            ToolCall(b["id"], b["name"], b.get("input") or {})
            for b in body.get("content", [])
            if b.get("type") == "tool_use"
        ]
        u = body.get("usage", {})
        return Reply(
            text, calls, {"input": u.get("input_tokens", 0), "output": u.get("output_tokens", 0)}
        )


def _openai_messages(system: str, messages: list[dict], *, args_as_text: bool) -> list[dict]:
    out: list[dict] = [{"role": "system", "content": system}]
    for m in messages:
        if m["role"] == "user":
            out.append({"role": "user", "content": m["content"]})
        elif m["role"] == "assistant":
            msg: dict[str, Any] = {"role": "assistant", "content": m.get("content") or ""}
            if m.get("tool_calls"):
                msg["tool_calls"] = [
                    {
                        "id": c["id"],
                        "type": "function",
                        "function": {
                            "name": c["name"],
                            "arguments": json.dumps(c["arguments"])
                            if args_as_text
                            else c["arguments"],
                        },
                    }
                    for c in m["tool_calls"]
                ]
            out.append(msg)
        elif m["role"] == "tool":
            out.append({"role": "tool", "tool_call_id": m["tool_call_id"], "content": m["content"]})
    return out


def _openai_tools(tools: list[dict]) -> list[dict]:
    return [
        {
            "type": "function",
            "function": {
                "name": t["name"],
                "description": t["description"],
                "parameters": t["input_schema"],
            },
        }
        for t in tools
    ]


def _arguments(raw: Any) -> dict[str, Any]:
    if isinstance(raw, dict):
        return raw
    try:
        value = json.loads(raw or "{}")
    except ValueError:
        return {}
    return value if isinstance(value, dict) else {}


class OpenAI(_Base):
    """Any OpenAI-compatible Chat Completions host (OpenAI, Zen, LM Studio, vLLM)."""

    name = "openai"

    def request(self, system, messages, tools):
        url = (self.base_url or "https://api.openai.com/v1").rstrip("/") + "/chat/completions"
        headers = {"Authorization": f"Bearer {self.key}"} if self.key else {}
        body = {
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": _openai_messages(system, messages, args_as_text=True),
            "tools": _openai_tools(tools),
        }
        return url, headers, body

    def parse(self, body):
        msg = (body.get("choices") or [{}])[0].get("message", {})
        calls = [
            ToolCall(
                c.get("id") or uuid.uuid4().hex[:12],
                c["function"]["name"],
                _arguments(c["function"].get("arguments")),
            )
            for c in msg.get("tool_calls") or []
        ]
        u = body.get("usage", {})
        return Reply(
            msg.get("content") or "",
            calls,
            {"input": u.get("prompt_tokens", 0), "output": u.get("completion_tokens", 0)},
        )


class Ollama(_Base):
    name = "ollama"

    def request(self, system, messages, tools):
        url = self.base_url or os.environ.get("OLLAMA_HOST") or "http://127.0.0.1:11434"
        if not url.startswith("http"):
            url = "http://" + url
        body = {
            "model": self.model,
            "stream": False,
            "messages": _openai_messages(system, messages, args_as_text=False),
            "tools": _openai_tools(tools),
            "options": {"num_predict": self.max_tokens},
        }
        return url.rstrip("/") + "/api/chat", {}, body

    def parse(self, body):
        msg = body.get("message", {})
        calls = [
            ToolCall(
                uuid.uuid4().hex[:12],
                c["function"]["name"],
                _arguments(c["function"].get("arguments")),
            )
            for c in msg.get("tool_calls") or []
        ]
        return Reply(
            msg.get("content") or "",
            calls,
            {"input": body.get("prompt_eval_count", 0), "output": body.get("eval_count", 0)},
        )


PROVIDERS = {"anthropic": Anthropic, "openai": OpenAI, "ollama": Ollama}


def key_for(env_name: str) -> str:
    """The key: the console's environment, else the keychain entry of that name."""
    if not env_name:
        return ""
    return os.environ.get(env_name) or keychain.get_env(env_name) or ""


def from_settings(settings) -> _Base:
    cls = PROVIDERS.get(settings.chat_provider)
    if cls is None:
        raise ProviderError("no chat model is configured (console Settings → Chat)")
    return cls(
        model=settings.chat_model,
        base_url=settings.chat_base_url,
        key=key_for(settings.chat_api_key_env),
        max_tokens=settings.chat_max_tokens,
    )
