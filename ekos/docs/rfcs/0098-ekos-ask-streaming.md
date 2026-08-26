# RFC 0098 — `ekos ask` streaming (CLI path)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-26
**Implemented:** 2026-08-26

---

## Motivation

RFC C of this session's Runtime/Retrieval gap-closure plan: `ekos ask` waited for a completion to
finish in full before printing anything, even though every real provider's API supports streaming
tokens back as they're generated. `docs/GAP_ANALYSIS.md` named this as open backlog restated across
RFC 0008/0009.

## Design

### `LlmProvider::complete_stream` — a default method, not a breaking trait change

A new trait method with a default implementation that falls back to `complete()` and reports the
whole response as one chunk. This means the three real providers
(`AnthropicProvider`/`OpenAiProvider`/`OllamaProvider`) are the only ones that need real code —
`MockLlmProvider` and every test-only `LlmProvider` implementor across the codebase (e.g.
`crates/recovery/src/cache.rs`'s own `CountingMock`) keep compiling and passing unchanged.

**A real, non-obvious `async_trait` pitfall found live, not anticipated in the original plan:**
the callback parameter was first written as `on_chunk: &mut (dyn FnMut(&str) + Send)`, matching the
plan's own stated design — this fails to borrow-check inside `async_trait`'s macro expansion.
`async_trait` rewrites elided lifetimes to one shared named lifetime across the whole signature,
which breaks the implicit `for<'r> FnMut(&'r str)` higher-ranked bound a borrowed-`&str` callback
parameter needs. Fixed by taking an owned `String` per chunk instead (`&mut (dyn FnMut(String) +
Send)`) — sidesteps the lifetime issue entirely, at the cost of one allocation per chunk, negligible
at LLM token-chunk granularity.

### `stream_lines` — one shared low-level primitive, no new dependencies

`crates/recovery/src/llm.rs` gains `pub(crate) async fn stream_lines(resp: reqwest::Response, on_line: impl FnMut(&str)) -> Result<(), LlmError>`, reading the response body via `reqwest::Response::chunk()`
in a loop (buffering partial lines across chunk boundaries) and calling `on_line` once per complete,
non-empty, trimmed line. Deliberately **not** built on `reqwest`'s `bytes_stream()`/the `futures`
crate — `Response::chunk()` needs no extra `reqwest` Cargo feature, so this shared helper is the
only place any incremental-body-reading logic needs to exist at all. Every provider's SSE
(Anthropic/OpenAI) or NDJSON (Ollama) parsing builds on this one primitive.

### Per-provider streaming, and the accumulator-struct pattern for testability

Each provider gets a `complete_stream` override plus two small, provider-local pieces:
a `#[derive(Default)] struct StreamAccumulator { content, model, input_tokens, output_tokens }`, and
a pure function `fn apply_stream_line(acc: &mut StreamAccumulator, line: &str, on_chunk: &mut dyn
FnMut(String))`. Keeping the parsing state in a plain struct instead of inline closure captures
means `apply_stream_line` is directly unit-testable with synthetic line strings — no live API key,
no mock HTTP server, no network at all.

- **Anthropic**: `content_block_delta` events carry `delta.text`; `message_start` carries model +
  input tokens; `message_delta` carries output tokens. Any other event `type`, or a non-`data:`/
  non-JSON SSE line (`event: ...`), is silently skipped — forward-compatible with new event types.
- **OpenAI**: `choices[0].delta.content` carries each piece; `stream_options.include_usage: true` is
  now sent on the streaming request specifically so a final usage-only chunk (empty `choices`,
  top-level `usage`) carries real token counts — without it, a streamed OpenAI response has no usage
  data anywhere, unlike the non-streaming path, which always has one. The `data: [DONE]` sentinel is
  skipped, not an error.
- **Ollama**: NDJSON, no SSE prefix — each line is a whole JSON object. `message.content` carries
  each piece; the terminal `done: true` line carries real `prompt_eval_count`/`eval_count`.

### `CachedLlmProvider<T>` — streaming deliberately bypasses the disk cache

`complete_stream` delegates straight to `self.inner.complete_stream(req, on_chunk)`, no hashing or
persistence. The disk cache (RFC 0008) needs one complete `LlmResponse` to key and write, which a
stream doesn't have until it's already finished; per-turn context for a streamed call is also
typically unique, so the cache-hit rate would be near zero regardless of effort spent making it work.

### `AiRuntime::ask_stream` and the CLI's `--stream` flag

`ask_stream` is `ask`'s twin: identical `gather_context` retrieval, identical prompt construction,
the only difference is calling `self.llm.complete_stream(&req, on_chunk)` instead of `complete`.
Citation extraction (`extract_citations`) still needs the *full* response text — it locates the
trailing `{"cited_evidence": [...]}` block via `rfind('{')`, which can't be resolved mid-stream — so
`AiAnswer` is only available once the stream ends regardless of how the caller consumed it live.

`ekos ask --stream` prints prose chunks as they arrive via `print!`+flush, then prints a blank line,
then the same `Sources:`/diagnostics section `ask` (non-streaming) already prints. `--stream` and
`--json` together are rejected with a clear error before the store is even opened — `--json` needs
the complete structured `AiAnswer`, not a partial one, and there's no reasonable way to reconcile
"stream tokens live" with "wait for the whole answer to serialize one JSON object."

**A real, named, accepted v1 limitation**: because the trailing citation JSON block is part of the
same stream the prose is, and there's no way to know live whether a `{` character is the start of
that real trailing block or just part of the prose (only `rfind` on the *complete* text can tell),
`--stream` prints the raw `{"cited_evidence": [...]}` block to the terminal too — the non-streaming
path stays fully clean (citation-stripped). Documented explicitly in `ask.rs`'s own comment rather
than silently accepted; a smarter buffering scheme was considered and rejected as needlessly risky
for a cosmetic gain (see Non-goals).

## Non-goals

- **MCP streaming.** The MCP server's `tools/call` loop is one JSON-RPC response per request over
  stdio with no progress-notification mechanism today — adding one is a separate, larger MCP
  protocol change, not part of this RFC. `ekos_ask` isn't even an MCP tool yet (RFC 0009's `AiRuntime`
  has never been exposed there) — that's also out of scope here.
- **A buffering scheme to hide the trailing citation JSON block during live streaming.** Investigated
  and rejected: any approach that holds back output until it's sure a `{` isn't the real trailing
  block risks either truncating legitimate prose that happens to contain a literal `{`, or silently
  duplicating/dropping text at the boundary — a real correctness risk for a purely cosmetic
  improvement. The honest, simple, always-correct choice (stream everything, including the raw
  trailing block) was preferred over a cleverer one with genuine edge-case risk.

## Verification

Recovery crate: 13 new unit tests across the three providers' `apply_stream_line` functions (content
accumulation and ordering, token-usage extraction from the correct line, malformed/sentinel lines
silently ignored) plus `CachedLlmProvider`'s bypass — all offline, no network. Runtime crate: 1 new
`ask_stream` test (via `MockLlmProvider`) confirming it returns the same grounded answer/citations
`ask` would and genuinely routes through `complete_stream`. CLI crate: 1 new test confirming
`--stream --json` is rejected before the store is even opened. Full workspace gate clean (`cargo
fmt`, `build --workspace`, `clippy --workspace -D warnings`, `test --workspace`), `tests/integration`
3/3.

Live-verified against a real local Ollama daemon twice: a standalone probe calling
`OllamaProvider::complete_stream` directly (9 real incremental chunks, correct final content, real
`input_tokens`/`output_tokens` from the `done: true` line) both before and after the
`StreamAccumulator` refactor; then the full real CLI path (`ekos ask --stream`) against a real,
freshly-built EKOS workspace — real streamed prose, the documented trailing-JSON limitation exactly
as expected, and a correct `Sources:` section citing the real evidence. Also confirmed that the
pre-existing, unmodified non-streaming `ask` path is equally slow against a large real ledger with a
broad question (a pre-existing local-model/large-context characteristic, not a regression introduced
here) by reproducing the same slowness on the unmodified `ask` command against the same real
workspace before switching to a smaller, faster verification target.
