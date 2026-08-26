# Devlog 115 — RFC 0098: `ekos ask` streaming, and an `async_trait` lifetime pitfall

**Date:** 2026-08-26
**PRs:** RFC 0098
**Branch:** main (direct)

---

## Summary

RFC C of this session's six-RFC Runtime/Retrieval gap-closure plan (RFC A/`devlog_113` was EKL's
`AS OF`/`COUNT`/`GROUP BY`; RFC B/`devlog_114` was the read-only `FactLedger` open + MCP store
cache). This one: real token-by-token streaming for `ekos ask`, across all three real LLM providers
(Anthropic, OpenAI, Ollama), live-verified against a real local Ollama daemon both in isolation and
through the full real CLI path.

---

## RFC 0098 — `ekos ask` streaming

### Problem / motivation

`ekos ask` always waited for a full completion before printing anything — a real, restated gap
(RFC 0008/0009) despite every real provider's API supporting incremental token streaming.

### What was built

| Component | Change |
|---|---|
| `LlmProvider` trait | New `complete_stream` method, **default-implemented** (falls back to `complete`) — no existing implementor breaks |
| `crates/recovery/src/llm.rs` | New shared `stream_lines` primitive (via `reqwest::Response::chunk()`, no new dependency) |
| `AnthropicProvider`/`OpenAiProvider`/`OllamaProvider` | Real SSE/NDJSON streaming overrides, each with a `StreamAccumulator` + pure `apply_stream_line` function |
| `CachedLlmProvider<T>` | `complete_stream` bypasses the disk cache entirely, delegates straight to `inner` |
| `AiRuntime::ask_stream` | `ask`'s twin — same retrieval/prompt, streams the completion call only |
| `ekos ask --stream` | New CLI flag; rejects `--stream --json` together before opening the store |

### Implementation details worth remembering

- **A real, non-obvious `async_trait` macro pitfall, not anticipated in the original plan.** The
  callback parameter was first written as `&mut (dyn FnMut(&str) + Send)`, matching the plan's own
  stated design — this fails to borrow-check specifically inside `#[async_trait]`'s macro
  expansion, because the macro rewrites elided lifetimes to one shared named lifetime across the
  whole function signature, which silently breaks the implicit `for<'r> FnMut(&'r str)`
  higher-ranked bound a borrowed-`&str` callback parameter needs. The compiler error itself doesn't
  point at `async_trait` at all — it just looks like an ordinary borrow-checker failure
  ("`content` does not live long enough"), which took a few iterations to actually trace to the
  macro rather than the code. Fixed by switching the callback to take an owned `String` per chunk —
  sidesteps the whole class of issue, at the cost of one allocation per chunk (negligible at LLM
  token-chunk granularity). Worth remembering for any future `#[async_trait]` method that wants to
  accept a `dyn Fn*(&T)` callback parameter: prefer an owned type unless there's a specific reason
  not to.
- **Extracting parsing state into a plain `StreamAccumulator` struct plus a pure `apply_stream_line`
  function (rather than inline closure captures) turned out to make all the difference for test
  coverage.** None of the three providers' streaming logic needs network access to test once it's
  structured this way — 13 new tests run entirely offline against synthetic SSE/NDJSON line
  strings, yet exercise the exact same parsing code the real network path calls.
- **`Response::chunk()` (not `bytes_stream()`) was the right call, found by checking before adding a
  dependency, not after.** The original plan implicitly assumed a `Stream`-returning API might be
  needed, which would have pulled in `reqwest`'s `"stream"` Cargo feature and possibly the `futures`
  crate as a new `ekos-recovery` dependency. `Response::chunk()` is part of reqwest's core async API
  with no extra feature needed, and was sufficient for the whole design — zero new dependencies for
  this entire RFC.
- **OpenAI's streaming wire format has no token usage at all unless `stream_options.include_usage:
  true` is explicitly requested** — unlike the non-streaming response, which always carries `usage`.
  Found by reading OpenAI's actual documented streaming shape before implementing, not discovered
  live as a bug; the field is now sent only on the streaming request (`None` on the non-streaming
  one, via `#[serde(skip_serializing_if = "Option::is_none")]`).

### Decisions (alternatives considered, why this choice)

- **The trailing `{"cited_evidence": [...]}` block is not hidden during live streaming — a
  deliberate, documented v1 limitation, not an oversight.** `extract_citations` can only strip it
  from the *complete* response text (it finds the *last* `{` via `rfind`, which can't be resolved
  until the stream ends). A buffering scheme that holds back recently-streamed text until it's sure
  a `{` isn't the real trailing block was considered and rejected: it risks either truncating
  legitimate prose containing a literal `{`, or a subtle duplication/drop bug at the boundary — a
  real correctness risk for a purely cosmetic improvement. The honest, always-correct choice
  (stream everything, including the raw JSON tail) was preferred, matching this project's repeated
  "don't force a fix that risks a worse regression than the problem it solves" pattern.
  Non-streaming `ask` stays fully clean either way.
  - **MCP streaming explicitly out of scope.** The server's `tools/call` loop is one JSON-RPC
  response per stdio request with no progress-notification mechanism — a separate, larger protocol
  change. `ekos_ask` also isn't an MCP tool at all yet, independent of this RFC.

---

## Knowledge Captured

- **`#[async_trait]` can silently break higher-ranked trait bounds on callback-shaped parameters**
  (`dyn FnMut(&T)`) by rewriting elided lifetimes to one shared named lifetime across the whole
  signature. The resulting borrow-check error reads like an ordinary lifetime bug in the method
  body, not a macro-expansion artifact — worth checking for this specific pattern (a borrowed-slice
  callback parameter on an `#[async_trait]` method) before spending time debugging the call site
  itself. The fix (an owned parameter type) is simple once recognized.
- **Live-verifying against this repo's own real, large self-analysis ledger was the wrong choice
  for testing a small, fast feature.** A real grounded `ask` question against a ~5,500-object ledger
  with local Ollama took over 90 seconds with zero output — not a bug (confirmed by reproducing the
  identical slowness on the pre-existing, unmodified non-streaming `ask` path against the same
  ledger), just a real characteristic of large-context local inference. A small, purpose-built
  scratch workspace (one file, one object) verified the actual feature in about 30 seconds instead.
  Worth choosing verification scope deliberately rather than defaulting to "the biggest real corpus
  available" — bigger isn't always the more informative test.
- **`reqwest::Response::chunk()` is a real, dependency-free way to consume a streaming HTTP body**
  incrementally in this codebase — worth remembering as the default choice over `bytes_stream()`
  (which needs the `"stream"` Cargo feature) for any future incremental-response-reading need.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/llm.rs` | New default `complete_stream` trait method; new `stream_lines` shared primitive |
| `ekos/crates/recovery/src/anthropic.rs` | Real SSE `complete_stream`; `StreamAccumulator`/`apply_stream_line`; 5 new tests |
| `ekos/crates/recovery/src/openai.rs` | Real SSE `complete_stream` with `stream_options.include_usage`; `StreamAccumulator`/`apply_stream_line`; 3 new tests |
| `ekos/crates/recovery/src/ollama.rs` | Real NDJSON `complete_stream`; `StreamAccumulator`/`apply_stream_line`; 3 new tests |
| `ekos/crates/recovery/src/cache.rs` | `CachedLlmProvider::complete_stream` bypasses the cache |
| `ekos/crates/runtime/src/ai.rs` | New `AiRuntime::ask_stream`; 1 new test |
| `ekos/crates/cli/src/bin/ekos.rs` | New `--stream` flag on `ekos ask` |
| `ekos/crates/cli/src/commands/ask.rs` | `--stream` wiring, `--stream`+`--json` rejection; 1 new test |
| `ekos/docs/rfcs/0098-ekos-ask-streaming.md` | New RFC |
