"""One chat turn (RFC 0187 §2): the model, its tool calls, their results, until it answers."""

from __future__ import annotations

from collections.abc import AsyncIterator, Awaitable, Callable
from typing import Any

MAX_RESULT = 20_000

CallTool = Callable[[str, dict[str, Any]], Awaitable[tuple[str, bool]]]


def system_prompt(project: str) -> str:
    return (
        f"You answer questions about the project '{project}' using the EKOS tools. EKOS has "
        "compiled the project's code, schemas and documents into a knowledge ledger; the tools "
        "read it. Use them before answering, say which objects an answer rests on (their ids), "
        "and say plainly when the tools did not find something. Do not invent facts the tools "
        "did not return. Answer briefly."
    )


def _cut(text: str) -> str:
    if len(text) <= MAX_RESULT:
        return text
    return text[:MAX_RESULT] + f"\n[cut: {len(text) - MAX_RESULT} more characters]"


async def turn(
    provider,
    system: str,
    messages: list[dict[str, Any]],
    tools: list[dict[str, Any]],
    call_tool: CallTool,
    *,
    max_calls: int,
) -> AsyncIterator[dict[str, Any]]:
    """Run one turn. `messages` (ending with the person's message) is extended in place with
    every assistant and tool message, so the caller can keep the conversation."""
    calls = 0
    while True:
        reply = await provider.complete(system, messages, tools)
        yield {"type": "usage", **reply.usage}
        if not reply.tool_calls:
            messages.append({"role": "assistant", "content": reply.text})
            yield {"type": "text", "text": reply.text}
            return
        if calls >= max_calls:
            text = (
                f"(Stopped: the limit of {max_calls} tool calls per question was reached. "
                "Ask a narrower question, or raise chat_max_tool_calls in the console settings.)"
            )
            messages.append({"role": "assistant", "content": text})
            yield {"type": "text", "text": text}
            return
        pending = reply.tool_calls[: max_calls - calls]
        messages.append(
            {
                "role": "assistant",
                "content": reply.text,
                "tool_calls": [
                    {"id": c.id, "name": c.name, "arguments": c.arguments} for c in pending
                ],
            }
        )
        for c in pending:
            calls += 1
            yield {"type": "tool_call", "id": c.id, "name": c.name, "arguments": c.arguments}
            text, failed = await call_tool(c.name, c.arguments)
            result = _cut(text)
            messages.append(
                {
                    "role": "tool",
                    "tool_call_id": c.id,
                    "name": c.name,
                    "content": result,
                    "ok": not failed,  # for the page; providers send only the content
                }
            )
            yield {
                "type": "tool_result",
                "id": c.id,
                "name": c.name,
                "ok": not failed,
                "summary": result[:400],
            }
