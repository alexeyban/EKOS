# Devlog 116 — RFC 0099: multi-turn `ekos ask` history, and a mechanical-refactor near-miss

**Date:** 2026-08-26
**PRs:** RFC 0099
**Branch:** main (direct)

---

## Summary

RFC D of this session's six-RFC Runtime/Retrieval gap-closure plan — the last of the four RFCs
actually completed this session (A/`devlog_113` EKL, B/`devlog_114` read-only `FactLedger` + MCP
cache, C/`devlog_115` streaming). Real multi-turn conversation memory for `ekos ask --session
<name>`, live-verified with a real local Ollama daemon on a question that could only be answered
correctly by genuinely using the conversation history sent to the model — not just by code that
compiles and looks plausible.

---

## RFC 0099 — Multi-turn conversation history

### Problem / motivation

`ekos ask` was one-shot: no session, no memory across invocations, confirmed genuinely greenfield
during this whole plan's original exploration (nothing session-shaped existed anywhere).

### What was built

| Component | Change |
|---|---|
| `LlmRequest` | New `history: &[Message]` field; 15 call sites across 11 files updated to `history: &[]` |
| `CachedLlmProvider` | Cache key folds `history` in — no-op extension for the empty-history case |
| Anthropic/OpenAI/Ollama | `messages` becomes `Vec<ApiMessage>`, built as history + current turn |
| `AiRuntime` | New `ask_with_history`/`ask_stream_with_history`; `ask`/`ask_stream` now thin wrappers with empty history |
| `ekos ask --session <name>` | New flag; `.ekos/ask-sessions/<name>.json` transcript, validated name |

### Implementation details worth remembering

- **A real near-miss in the mechanical part of this change, caught before it reached a test run,
  not after.** Adding `history: &[]` to 15 real `LlmRequest` construction sites across 11 files is
  exactly the kind of change a search-and-replace handles well — except a first attempt matched on
  the text `max_tokens: <value>,` alone, blind to which *struct* that field belonged to. Two
  unrelated structs that happen to also have a `max_tokens` field got silently corrupted:
  `AiRuntimeConfig::default()` in `ai.rs` (an entirely different config struct, no `history` field
  at all) and `openai.rs`'s own `ApiRequest` wire-type *definition* — not even a construction site,
  the struct's own field list. Both failed to compile immediately, and were found and fixed by
  reading the actual `cargo build` error output line by line rather than trusting the script's own
  "N insertions" count as proof of correctness. The general lesson: a search-and-replace across many
  files needs the *compiler*, not the script's own success message, as the real verification step —
  this session hit the identical shape of near-miss in the exact same way during RFC 0096's `AS OF`
  work too (a stale-fingerprint bug caught by a real regression test, not by the code "looking
  right"), and this is the same discipline applied at refactor time instead of runtime.
- **Storing the raw grounded prompt or raw citation-block response as conversation history would
  have been a real, working-but-wasteful design — caught at design time, not after implementing
  it.** The obvious naive approach ("just store what was actually sent/received each turn") would
  re-inflate every later prompt with the *same* retrieved-context JSON blob from earlier turns,
  repeated verbatim, forever growing. Storing the clean `question`/`answer` pair instead — decided
  before writing `ConversationTurn`, not discovered as a bug afterward — keeps a session's token
  cost bounded by actual conversation length, not by how much ledger context each turn happened to
  retrieve.

### Decisions (alternatives considered, why this choice)

- **Retrieval stays turn-local, not conversation-aware — a named v1 limitation, not silently
  assumed.** Each turn's `gather_context` search still runs off that turn's own question text alone.
  Blending retrieval across turns (resolving "it"/"that table" against what a prior turn actually
  retrieved) is a real, separate, harder open problem — folding it in here would have doubled this
  RFC's real scope for a benefit not yet requested or demonstrated as needed.
- **No token-budget cap on accumulated history in v1.** A long enough session could eventually push
  `history`'s serialized size past a provider's context window — the same class of problem
  `gather_context`'s own `max_context_chars` (RFC 0046) already solves for retrieved context. Left
  as real, deliberately-scoped future work once a real long session shows it's actually needed,
  rather than guessing at a truncation/summarization policy with no real usage data to design it
  against.
- **`--session` name validation is strict (`[A-Za-z0-9_-]+` only), not "sanitize and proceed."** The
  name becomes a path component; anything permissive enough to include `/` or `..` risks writing
  outside `.ekos/ask-sessions/` entirely. A clear rejection error was chosen over silently stripping
  unsafe characters, matching this session's own established "explicit failure over silently-wrong
  behavior" convention (RFC 0096's `AS OF` + `FROM` rejection, `COUNT` + `RETURN` rejection).

---

## Knowledge Captured

- **A mechanical refactor across many files is only as trustworthy as its actual compile check** —
  a script reporting "N insertions succeeded" describes what it *attempted*, not what it produced;
  two of the insertions in this session's own refactor landed in the wrong struct entirely and only
  a full `cargo build` (read line by line, not just checked for exit code 0 on the first pass)
  caught them. Worth treating any bulk text-based code edit as unverified until the compiler — not
  the edit tool's own success report — confirms it.
- **A live end-to-end test that the feature *can't* pass by accident is worth constructing
  deliberately.** Asking the model a question entirely about the conversation itself ("what was my
  previous question about?") rather than about the ledger content is what made this session's live
  verification actually prove the history was used, rather than merely proving the code didn't
  crash — a grounded-content question could have looked identically correct whether or not history
  actually reached the model, since `gather_context`'s own retrieval would produce a real answer
  either way.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/llm.rs` | New `Message` type; `LlmRequest.history` field |
| `ekos/crates/recovery/src/anthropic.rs` | `build_messages`; `messages: Vec<ApiMessage>`; 2 new tests |
| `ekos/crates/recovery/src/openai.rs` | `build_messages`; `messages: Vec<ApiMessage>`; 2 new tests |
| `ekos/crates/recovery/src/ollama.rs` | `build_request` extended for history; `messages: Vec<ApiMessage>`; 1 new test |
| `ekos/crates/recovery/src/cache.rs` | Cache key folds `history` in; 2 new tests |
| `ekos/crates/recovery/src/{sql_analyzer,document_semantics_analyzer,architecture_reasoning,llm_description}.rs`, `crates/cli/src/commands/docs.rs`, `crates/marketing/src/tweet.rs`, `crates/clickhouse-query/src/lib.rs` | Mechanical `history: &[]` at existing `LlmRequest` construction sites |
| `ekos/crates/runtime/src/ai.rs` | New `ConversationTurn`; `ask_with_history`/`ask_stream_with_history`; `ask`/`ask_stream` now thin wrappers; 2 new tests |
| `ekos/crates/runtime/src/lib.rs` | Export `ConversationTurn` |
| `ekos/crates/cli/src/bin/ekos.rs` | New `--session` flag |
| `ekos/crates/cli/src/commands/ask.rs` | `--session` wiring, name validation, session load/save; 6 new tests |
| `ekos/docs/rfcs/0099-ekos-ask-multi-turn-history.md` | New RFC |
