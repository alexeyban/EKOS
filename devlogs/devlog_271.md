# Devlog 271 — Chat with a project using its EKOS tools (RFC 0187)

**Date:** 2026-10-14
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

Last of four phases (RFC 0184–0187). A project's Chat tab answers questions with a configured
model that calls the project's EKOS tools until it can answer, each call shown. Providers are
called over plain HTTP: Ollama, any OpenAI-compatible server, Anthropic. The model gets only
reading tools, with the person's rights and read scope, through the same check as the MCP
gateway; review tools are never offered. A hosted model needs the tenant's metered switch.
`ekos setup` has a Chat step. Verified end to end with a local Ollama model (`qwen2.5:1.5b`) in
the package check, in the API and in a browser; no paid call was made anywhere.

---

## RFC 0187

### What was built

| Piece | What it does |
|---|---|
| `app/chat/providers.py` | `Anthropic`, `OpenAI`, `Ollama`: pure `request`/`parse` per provider, `complete` over `httpx`; `is_metered`; key from the environment or keychain by name |
| `app/chat/agent.py` | One turn: model → tool calls → results → … → text; tool-call limit; results cut at 20 000 characters |
| `routes/chat.py` | `GET /chat/model`; list/get/delete conversations; `POST …/chat` streaming Server-Sent Events |
| `models.py` | `Conversation`, `ChatMessage` (new tables; `create_all` adds them) |
| Settings | `chat_provider`, `chat_model`, `chat_base_url`, `chat_api_key_env`, `chat_max_tool_calls`, `chat_max_tokens` |
| UI `Chat.tsx` | Conversations, streamed events read from `fetch`, collapsible tool calls, usage, Stop |
| `ekos setup` | `[chat]` answers, `StoreKey` (keychain via the bundled Python), `/api/ollama-models`, `/api/check-model` (provider model list — no model call) |
| Package check | The answers set a local model; the console stage chats over the API, the browser stage on the page |

### Implementation details worth remembering

- **The tool filter is two checks:** `mcp_policy.action_for(tool) == "read"` (so no writing or
  review tool even for an owner), and `mcp_gateway._Caller.may` (role, and scoped tools only
  for a restricted reader). The MCP client comes from `mcp_for_workspace`, the scoped one —
  the route-walk test insists every scoped route reaching MCP does so.
- `POST` cannot use `EventSource`; the page reads `res.body.getReader()` and splits on blank
  lines (`splitEvents`, tested with a partial event at the end of a chunk).
- The conversation is stored in one internal shape (with an extra `ok` on tool messages for the
  page; providers ignore it) and converted per provider; Anthropic needs all tool results of a
  turn in one user message.
- `qwen2.5:1.5b` through Ollama's `/api/chat` reliably returns `tool_calls` with arguments as an
  object (OpenAI returns a JSON string; both are handled).

### Decisions

- **No SDKs.** Three small request/response shapes are easier to test than three dependencies,
  and the console image stays the same size.
- **Metered by default unless provably local** (Ollama, or a loopback base URL). Paid calls use
  the RFC 0183 tenant switch rather than a new one.
- **Answers arrive whole per step** — tool calls stream as they happen, text does not. Token
  streaming differs per provider and was not needed to be useful.

---

## Knowledge Captured

- A guard test (`test_only_the_listed_routes_are_open_to_a_restricted_reader`) caught the first
  version of the chat route building its own MCP client. Routes open to a restricted reader must
  take `mcp_for_workspace`; history routes that only read the person's own rows are listed as
  the exception, with the reason.
- Setup's wizard step count is part of the browser check: adding a step means one more Next.

---

## Verification

`deploy/e2e/package.sh`: 19 stages, console stage 26 checks (chat with the local model, a tool
called, answered, kept), browser stage 9. Unit tests: providers' shapes, the loop, the review
scan, scope, metered refusal, privacy, SSE. Gates: see the commit. Not checked: Anthropic and
OpenAI against their real services (no paid call), Windows and macOS.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0187-chat-with-ekos-tools.md` | New RFC |
| `web/api/app/chat/{__init__,providers,agent}.py`, `routes/chat.py` | New |
| `web/api/app/{models,settings,console_file,main}.py` | Tables, settings, metadata, router |
| `web/api/tests/test_chat.py`, `test_authz_routes.py` | Tests |
| `web/ui/src/pages/Chat.tsx` (+ test), `main.tsx`, `WorkspaceShell.tsx`, `index.css` | Chat tab |
| `ekos/crates/cli/src/commands/{setup,setup_server,setup_tests}.rs`, `setup.html` | Chat step |
| `deploy/e2e/package.sh`, `package_console.py`, `package_browser.py`, `package_wizard.py` | Chat in the package check |
| `deploy/package/INSTALL-full.md`, `README.md`, `docs/deployment.md`, capabilities page, `CHANGELOG.md`, `TODO.md`, `CLAUDE.md` | Docs |
