# RFC 0099 — Multi-turn conversation history in `ekos ask`

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-26
**Implemented:** 2026-08-26

---

## Motivation

RFC D of this session's Runtime/Retrieval gap-closure plan (RFC A/`devlog_113` EKL, RFC B/
`devlog_114` read-only `FactLedger` + MCP cache, RFC C/`devlog_115` streaming). `ekos ask` was
always a one-shot question: no session, no memory of a prior turn, confirmed genuinely greenfield
during this plan's original exploration pass — nothing resembling session/history state existed
anywhere in the codebase.

## Design

### `LlmRequest` grows a `history` field — a default-safe, mechanical change

`LlmRequest<'a>` gains `history: &'a [Message<'a>]` (`Message { role, content }`, a new small type
in `crates/recovery/src/llm.rs`). 15 real call sites across 11 files construct `LlmRequest` literally
(analyzer passes, `docs --prose`, `marketing`, the ClickHouse NL-to-SQL bridge, every provider's own
tests) — every one of them now passes `history: &[]`, a purely mechanical, behavior-preserving
addition. `CachedLlmProvider`'s cache key is extended to fold `history` in (role+content per turn,
after the existing system/user bytes) — a no-op extension for every empty-history caller, so the key
is byte-for-byte identical to before RFC 0099 for the pre-existing (overwhelming majority) case.

**A real mechanical-refactor hazard found live, not filed away as a hypothetical.** The first
attempt inserted `history: &[]` by matching every `max_tokens: <value>,` line via search-and-replace
across the 11 files — a fast approach, but blind to *which* struct a `max_tokens` field belonged to.
It silently corrupted two unrelated structs that happen to also have a `max_tokens` field: a real
`AiRuntimeConfig::default()` construction in `ai.rs` (a completely different struct, no `history`
field at all) and `openai.rs`'s own `ApiRequest` wire-type *definition* (not just its construction
sites). Both compiled-error immediately (`E0560`/parse error) and were caught and fixed before ever
reaching a test run, by reading a full-workspace `cargo build` diff rather than trusting the
search-and-replace's own reported "success" count.

### Per-provider wire-format wiring

- **Anthropic**: `messages` becomes `Vec<ApiMessage>` (was a fixed `[ApiMessage; 1]`), built as
  `history + [current user turn]` — `system` stays Anthropic's own separate top-level field,
  unaffected.
- **OpenAI / Ollama** (`/api/chat` mirrors Chat Completions' convention): `messages` becomes
  `Vec<ApiMessage>`, built as `[system] + history + [current user turn]`.

Each gets a small `build_messages`/(Ollama's existing `build_request`, extended in place) helper —
directly unit-testable with synthetic `LlmRequest`s, no network needed, matching RFC 0098's
established `StreamAccumulator`/pure-function testing pattern.

### `AiRuntime::ask_with_history` / `ask_stream_with_history`

New methods alongside the existing `ask`/`ask_stream` (which now just delegate with an empty
history — zero behavior change for either). Take `history: &[ConversationTurn]`
(`ConversationTurn { question, answer }`, a new public type in `runtime/src/ai.rs`) and expand it
into alternating `user`/`assistant` `Message`s via a small `history_messages` helper.

**The one genuine design choice here, not just plumbing — decided and documented, not silently
assumed.** `ConversationTurn` stores the *clean* question and citation-stripped answer, never the
raw grounded prompt a turn was actually sent with (`"Question: ...\n\nContext:\n...json..."`) or the
raw LLM response (still carrying its trailing `{"cited_evidence": [...]}` block). Storing the clean
version means a long session's history doesn't re-inflate every later prompt with repeated
retrieved-context JSON nobody needs to see again — the model gets real conversational memory of what
was asked and answered, at a fraction of the token cost repeating full grounding context every turn
would carry.

**Retrieval stays turn-local — the RFC's other explicit, named v1 limitation.** `gather_context`
(unchanged) still runs its FTS5 search off *this* turn's `question` text alone, never the whole
conversation. Blending retrieval across turns (e.g. "resolve 'it' in this question against the prior
turn's retrieved objects") is a real, separate, harder problem — deliberately not attempted here, so
it doesn't silently expand what was meant to be the smaller of the two AI-runtime RFCs in this plan.

### `ekos ask --session <name>`

New CLI flag. Session transcript persisted at `.ekos/ask-sessions/<name>.json` — mirroring the
existing `.ekos/llm-cache/` convention (a plain, inspectable JSON file, not a new ledger table).
`--session`'s `name` becomes a path component, so it's validated to `[A-Za-z0-9_-]+` before use —
rejects anything that could escape the `ask-sessions` directory (`..`, `/`, an absolute path, a bare
`.`) with a clear error, rather than silently sanitizing or writing somewhere unintended. Missing
session file → empty history (first turn in a new session); each real turn appends its clean
question/answer pair and rewrites the file. Works with both `ekos ask --session <name>` and
`ekos ask --session <name> --stream`.

## Non-goals

- **Cross-turn retrieval blending.** Named above — real, substantial, separate future work.
- **Session expiry, listing, or deletion commands.** `.ekos/ask-sessions/*.json` are plain files a
  user can already inspect/delete by hand; a dedicated `ekos ask sessions list/rm` surface is real
  but unscoped product design, not attempted speculatively ahead of an actual need.
- **A token-budget cap on accumulated history.** A very long session could eventually push
  `history`'s own serialized size past a provider's context window, the same class of problem
  `gather_context`'s `max_context_chars` budget (RFC 0046) already solves for retrieved context —
  but doing the equivalent for conversation history is a real, separate scoping decision (truncate
  oldest turns? summarize them? error?) left for a future session once real usage shows it's needed.

## Verification

18 new unit tests: 5 across the three providers' `build_messages`/`build_request` (history correctly
placed between system and the current turn, empty-history case unchanged), 2 in `cache.rs`
(different histories are different cache entries; identical history is still a cache hit), 2 in
`runtime/src/ai.rs` (`ask_with_history` genuinely threads `ConversationTurn`s into the `LlmRequest`,
verified via a request-recording mock provider — not just that it compiles; `ask` without history
sends an empty one), 6 in `cli/src/commands/ask.rs` (`--session` name validation accepts/rejects the
right shapes, lands under the right path, missing-file/round-trip session load/save). Full workspace
gate clean (`cargo fmt`, `build --workspace`, `clippy --workspace -D warnings`, `test --workspace`),
`tests/integration` 3/3.

Live-verified against a real local Ollama daemon with a real two-turn session
(`ekos ask --session demo`) against a real, freshly built scratch workspace: turn 2's question
("What was my previous question about?") has no possible answer from ledger retrieval at all — it's
a question about the conversation itself — and the model answered it correctly ("Your previous
question was about what the main.rs file does"), which is only possible if the real conversation
history was genuinely sent and used, not just plumbed through unused code. The session file on disk
round-tripped correctly as two clean question/answer pairs, with no raw retrieval context or citation
JSON leaked into storage.
