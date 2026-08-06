# RFC 0021 — Local LLM Provider (Ollama)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-07-24
**Gating:** none (additive; independent of any phase)

---

## Motivation

`todo_v2.md`'s AI-001 debt item ("Single LLM Provider... Add: ... Ollama...
local models") has stood unaddressed since it was written. Every LLM-backed
recovery pass today hard-depends on `AnthropicProvider` — a live network
call and an API key — even though `LlmProvider` (RFC 0008) was designed to
be provider-agnostic from the start. A local provider means:

- Enterprise source content used for LLM enrichment never has to leave the
  machine, strengthening EKOS's "compiled knowledge you can trust" pitch.
- Zero marginal cost for iteration, and a usable path when no API key is
  configured that still gets real (not structural-only) enrichment.
- A cheap backend for future agent-side reasoning steps, the same
  motivation that already put `estate-scout` on `haiku` instead of a bigger
  model for its mechanical navigation work.

This RFC is deliberately small: RFC 0008's trait and cache design already
require no changes. The only real design decision is the one RFC 0008 left
implicit — what "degraded mode" means for a provider that has no API key
concept at all.

## Design

### `OllamaProvider`

New file `ekos/crates/recovery/src/ollama.rs`, implementing `LlmProvider`
exactly like `AnthropicProvider` (`ekos/crates/recovery/src/anthropic.rs`):

```rust
pub struct OllamaProvider {
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn from_env() -> Self { Self::from_env_model("llama3.1:8b") }
    pub fn from_env_model(default_model: impl Into<String>) -> Self { /* reads OLLAMA_BASE_URL, OLLAMA_MODEL */ }
    pub fn new(model: impl Into<String>, base_url: impl Into<String>) -> Self { /* ... */ }
}
```

- No API key: `OLLAMA_BASE_URL` (default `http://localhost:11434`) and
  `OLLAMA_MODEL` (default `llama3.1:8b`) env vars, both overridable.
  Construction **cannot fail** the way `AnthropicProvider::from_env_var`
  can (no `LlmError::NoApiKey` case applies) — reachability is only ever
  discovered on the first `complete()` call, as an ordinary `LlmError::Http`
  or `LlmError::Api`.
- Wire format: Ollama's `/api/chat` (`{"model", "messages":[{"role","content"}],
  "stream": false, "options": {"temperature": 0}}`), reusing the same
  workspace `reqwest` dependency (`ekos/Cargo.toml`, `rustls-tls`,
  `features = ["json"]`) `AnthropicProvider` already uses — no new
  dependency.
- Response mapping: Ollama's `message.content` → `LlmResponse.content`;
  `prompt_eval_count`/`eval_count` → `input_tokens`/`output_tokens` (`0` if
  absent, since not every Ollama model reports them).
- `temperature: 0` is hardcoded in the request body, identically to
  `anthropic.rs:90` — satisfies RFC 0008's determinism contract by
  construction, not by caller discipline.
- `model_name()` returns the real model tag (e.g. `"llama3.1:8b"`), so
  `CachedLlmProvider`'s cache key (`ekos/crates/recovery/src/cache.rs:12-22`,
  unchanged — it is generic over any `T: LlmProvider`) naturally
  invalidates when the local model changes.

### Provider selection

`build_llm_provider` (`ekos/crates/cli/src/commands/recover.rs:209-237`)
already threads `config.llm.provider: Option<String>`
(`ekos/crates/compiler-core/src/config.rs:75-79`) through `EkosConfig` but
never reads it — every call tries Anthropic first regardless of config.
This RFC makes that field live:

```rust
match config.llm.provider.as_deref() {
    Some("ollama") => Arc::new(CachedLlmProvider::new(OllamaProvider::from_env(), cache_dir)),
    _ => match AnthropicProvider::from_env_var(key_env) {
        Ok(provider) => Arc::new(CachedLlmProvider::new(provider, cache_dir)),
        Err(_) => Arc::new(MockLlmProvider::new(r#"{"entities":[],"relationships":[]}"#)),
    },
}
```

`ekos.toml` gains a working `provider = "ollama"` value under `[llm]`
(the key already existed in the schema per RFC 0008's example config; it
was simply never honored).

### Degraded-mode addendum to RFC 0008

RFC 0008 §"API key configuration" frames the only degraded-mode trigger as
"api-key-env not set or missing" — written before a keyless provider
existed. This RFC adds a parallel condition, without editing RFC 0008's
accepted text: **when `provider = "ollama"` and the daemon is unreachable
on the first call**, the recovery pass emits the same `Warning` diagnostic
RFC 0008 already specifies for the no-key case and falls back to
structural-only output — the failure just surfaces one call later (at
`complete()` time) than the Anthropic path's (at construction time), since
there is no way to check "is a local daemon running" cheaply up front
without adding a network round-trip to every `build_llm_provider` call.

## Alternatives Considered

- **Probe the daemon at construction time** (e.g. a `GET /api/tags` health
  check in `from_env()`) — rejected for v1: adds a synchronous network call
  to a path that today is instant, for a check that `complete()` already
  performs naturally on first use. Revisit if the deferred-failure UX
  proves confusing in practice.
- **A generic `provider = "openai" | "gemini" | ...` matrix now** — out of
  scope; `todo_v2.md`'s AI-001 lists several providers, but this RFC closes
  only the Ollama entry, which is the one with a concrete motivating use
  case (local/offline). The `match` shape added to `build_llm_provider`
  is the extension point for the rest, added one at a time as needed.

## Testing

- `OllamaProvider::model_name()` returns the constructed model tag.
- Request-body construction always sets `temperature: 0` regardless of
  caller input (mirrors `anthropic.rs`'s existing implicit guarantee —
  test by constructing the request the same way `complete()` does and
  asserting the JSON shape, without a live daemon).
- `build_llm_provider` selects `OllamaProvider` when `config.llm.provider ==
  Some("ollama")` and falls through to the existing Anthropic/Mock chain
  otherwise (unit test on the `match`, no network).
- Manual/optional: point a real local Ollama daemon at the recover pipeline
  end-to-end and confirm structurally-equivalent output to the
  Anthropic/Mock paths on the same fixture.

## Acceptance Criteria

- [ ] `OllamaProvider` implements `LlmProvider` and always sends
      `temperature: 0`.
- [ ] `config.llm.provider = "ollama"` routes `build_llm_provider` to
      `OllamaProvider`, still wrapped in `CachedLlmProvider`.
- [ ] Cache key correctness is unchanged and inherited for free (no edits
      to `cache.rs`).
- [ ] Unreachable-daemon failures degrade to structural-only output with a
      `Warning` diagnostic, matching the no-API-key UX in spirit.
- [ ] No new workspace dependency added.
