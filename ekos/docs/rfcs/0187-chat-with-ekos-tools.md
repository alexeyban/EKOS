# RFC 0187 — Chat with a configured model using the EKOS tools

**Status:** Accepted — implemented 2026-10-14 (devlog_271)
**Author:** EKOS team
**Created:** 2026-10-14
**Builds on:** RFC 0180 (roles), RFC 0181 (read scope), RFC 0182 (MCP gateway), RFC 0183 (metered calls per tenant), RFC 0184 (console settings, keychain), RFC 0185 (setup)
**Part of:** installer and web configuration (0184, 0185, 0186, this RFC)

## Motivation

The knowledge EKOS compiles is reachable by agents through MCP, which needs an agent on the
person's machine and someone to configure it. A person in the console should be able to ask a
project a question and get an answer built from the same tools, with the calls shown.

## 1. The model

Console settings (RFC 0184, group "Chat"): `chat_provider` (`anthropic`, `openai` — any
OpenAI-compatible Chat Completions host —, `ollama`, or empty for no chat), `chat_model`,
`chat_base_url`, `chat_api_key_env` (the **name** of the variable holding the key; the value
comes from the console's environment or from the keychain entry `env:<NAME>`, never from a
file), `chat_max_tool_calls` (12), `chat_max_tokens` (2048).

**Paid calls.** A provider is metered unless it is Ollama or its base URL is on this machine
(LM Studio, vLLM on loopback). A metered chat in a workspace runs only while that workspace's
tenant has metered calls on (RFC 0183's switch, which a platform owner sets); otherwise the
request is refused with that reason before any call. The page labels a metered model "paid per
call" before the first message.

Providers are called with `httpx` directly (no SDKs): Anthropic Messages (`tool_use` /
`tool_result`), OpenAI Chat Completions (`tool_calls`), Ollama `/api/chat` (`tool_calls`).
The conversation is kept in one internal shape and converted per provider.

## 2. The agent loop

A turn: the conversation so far → the model with the tool list → for each tool call, the call
→ the result back to the model → until the model answers in text, or `chat_max_tool_calls` is
reached (then it says so).

**Which tools.** The project's MCP tools that need only `read` (`mcp_policy.action_for`) and
that the person may call through the gateway's own check (`_Caller.may`: role, and for a
restricted reader only the scoped tools). Review tools and writing tools are never offered, to
anyone: a model is an agent, and review decisions are human-only (a test scans the chat code for
their names and checks every tool list sent to a provider).

**How tools are called.** Through `ScopedMcp` with the person's read scope (RFC 0181), exactly
as the gateway does; a restricted reader's chat cannot surface what they cannot see. Every call
is in the audit log (`chat: <tool>`), without its arguments. A tool result goes back to the
model up to 20 000 characters (cut with a note).

## 3. API and page

| Route | Does |
|---|---|
| `GET /api/chat/model` | provider, model, whether metered, whether configured |
| `GET /api/workspaces/{id}/chat` | this person's conversations in the workspace |
| `GET /api/workspaces/{id}/chat/{cid}` | one conversation's messages |
| `POST /api/workspaces/{id}/chat` | `{conversation_id?, message}` → Server-Sent Events: `conversation`, `tool_call`, `tool_result`, `text`, `usage`, `error`, `done` |
| `DELETE /api/workspaces/{id}/chat/{cid}` | delete one |

Conversations belong to the person who started them (`Conversation`, `ChatMessage` tables);
nobody else lists or reads them. The workspace's Chat tab shows the conversations, the answer,
each tool call with its arguments and a summary of its result (collapsible), the token usage,
and a Stop button.

`ekos setup` (RFC 0185) gains a Chat step: provider, model (Ollama's installed models listed),
base URL, key (to the keychain), and a check that makes **no paid call** (the provider's model
list).

## Tests (written first)

Scripted provider: search → state → answer; tool calls audited; the review and writing tools in
no tool list, for an owner either; a restricted reader's chat does not reach a hidden marker; a
metered model refused while the tenant switch is off, a local one allowed; conversations private
to their owner; the SSE event sequence; each provider's request and response conversion against
recorded shapes. End to end with a local Ollama model on a fixture project.

## Not built

Streaming of the model's text token by token (each step's text arrives whole), attachments,
sharing a conversation, per-tenant chat models.
