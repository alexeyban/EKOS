# RFC 0046 — OpenAI LLM Provider

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-12
**Gating:** none (additive; independent of any phase)

---

## Motivation

RFC 0045's demo server needs a working LLM key at boot (`check_llm_keys_or_exit`) and per-request
(`AiRuntime::ask`) to answer anything — this session's environment has no `ANTHROPIC_API_KEY`, but
does have (or the user intends to supply) an OpenAI key. RFC 0021 already anticipated this exact
situation and deliberately left the door open: *"A generic `provider = "openai" | "gemini" | ...`
matrix now — out of scope... The `match` shape added to `build_llm_provider` is the extension
point for the rest, added one at a time as needed."* This RFC adds the OpenAI entry to that match,
the same way RFC 0021 added Ollama's — `LlmProvider` (RFC 0008) was designed provider-agnostic from
the start, so this is implementing an existing extension point, not a new design.

## Design

### `OpenAiProvider`

New file `ekos/crates/recovery/src/openai.rs`, implementing `LlmProvider` exactly like
`AnthropicProvider`/`OllamaProvider`:

```rust
pub struct OpenAiProvider {
    model: String,
    api_key: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn from_env() -> Result<Self, LlmError> { Self::from_env_var("OPENAI_API_KEY") }
    pub fn from_env_var(env_var: &str) -> Result<Self, LlmError> { /* reads env_var, OPENAI_MODEL */ }
    pub fn new(model: impl Into<String>, api_key: impl Into<String>) -> Self { /* ... */ }
}
```

- API key: read from the given env var (default `OPENAI_API_KEY`, mirroring
  `AnthropicProvider::from_env_var`'s `LlmError::NoApiKey` behavior on absence — so RFC 0045's
  boot-time guardrail works unmodified against this provider too, see Provider selection below).
- Model: `OPENAI_MODEL` env var, default `"gpt-4o-mini"` — real, cheap, fast, matches RFC 0021's
  "cheap backend for mechanical work" cost-consciousness. Overridable, unlike
  `AnthropicProvider::from_env_var`, which hardcodes its model today (existing gap,
  `config.llm.model` is unused for the Anthropic path too — not fixed by this RFC, out of scope).
- Wire format: OpenAI's Chat Completions endpoint (`POST /v1/chat/completions`,
  `Authorization: Bearer <key>`), reusing the same workspace `reqwest` dependency (`rustls-tls`,
  `features = ["json"]`) every other provider already uses — no new dependency. Request body:
  `{"model", "temperature": 0, "max_tokens", "messages": [{"role":"system","content"},
  {"role":"user","content"}]}` (system prompt as its own message, not the Anthropic
  Messages-API-style top-level `system` field). Response mapping: `choices[0].message.content` →
  `LlmResponse.content`; `usage.prompt_tokens`/`usage.completion_tokens` →
  `input_tokens`/`output_tokens`; `model` (OpenAI echoes the resolved model id, which may differ
  from the requested alias) → `LlmResponse.model`.
- `temperature: 0` hardcoded in the request body, identically to `anthropic.rs:90` and
  `ollama.rs` — RFC 0008's determinism contract satisfied by construction, not caller discipline.
- `model_name()` returns the configured model string, so `CachedLlmProvider`'s cache key
  (unchanged, generic over any `T: LlmProvider`) naturally invalidates on model change.

### Provider selection

`build_llm_provider` (`ekos/crates/cli/src/commands/recover.rs`) gains one more arm in the
`match` RFC 0021 already established as the extension point:

```rust
match config.llm.provider.as_deref() {
    Some("ollama") => Arc::new(CachedLlmProvider::new(OllamaProvider::from_env(), cache_dir)),
    Some("openai") => match OpenAiProvider::from_env_var(key_env) {
        Ok(provider) => Arc::new(CachedLlmProvider::new(provider, cache_dir)),
        Err(_) => Arc::new(MockLlmProvider::new(r#"{"entities":[],"relationships":[]}"#)),
    },
    _ => match AnthropicProvider::from_env_var(key_env) {
        Ok(provider) => Arc::new(CachedLlmProvider::new(provider, cache_dir)),
        Err(_) => Arc::new(MockLlmProvider::new(r#"{"entities":[],"relationships":[]}"#)),
    },
}
```

`ekos.toml`'s `[llm] provider = "openai"` (the same field RFC 0021 made `"ollama"`-aware) selects
this path; `api-key-env` still defaults to `"ANTHROPIC_API_KEY"` in `EkosConfig`'s schema for
backward compatibility, so an OpenAI-configured `ekos.toml` must set
`api-key-env = "OPENAI_API_KEY"` explicitly (or whatever env var actually holds the key) — this
RFC does not special-case a different default per provider, keeping `build_llm_provider`'s
`key_env` resolution the single source of truth every provider arm already shares.

### RFC 0045's boot-time guardrail

`demo-server`'s `first_missing_key` (`crates/demo-server/src/main.rs`) special-cased `"ollama"` as
needing no key; it now also branches on `"openai"` to check via `OpenAiProvider::from_env_var`
instead of `AnthropicProvider::from_env_var` — same shape, same "fail loudly at boot" guarantee,
just checking the provider the catalog's `ekos.toml` actually selects.

## Alternatives Considered

- **OpenAI's Responses API** (the newer `/v1/responses` endpoint) instead of Chat Completions —
  rejected for v1; Chat Completions is the longer-established, more universally-supported surface
  and this RFC's only goal is parity with the existing Anthropic/Ollama providers, not adopting
  OpenAI's latest API shape. Revisit if Chat Completions is ever deprecated.
- **Threading `config.llm.model` through `AnthropicProvider` at the same time** — real, adjacent
  gap, but out of scope; this RFC only needs `OpenAiProvider` to honor its own model override
  (`OPENAI_MODEL`), and fixing Anthropic's separately is unrelated scope creep for what's meant to
  be a small, additive change, same discipline RFC 0021 held to.

## Testing

- `OpenAiProvider::model_name()` returns the constructed/env-resolved model string.
- Request-body construction always sets `temperature: 0` regardless of caller input (mirrors
  `anthropic.rs`'s and `ollama.rs`'s existing tests — assert the JSON shape without a live call).
- `build_llm_provider` selects `OpenAiProvider` when `config.llm.provider == Some("openai")` and
  falls through to the existing Ollama/Anthropic/Mock chain otherwise (unit test on the `match`,
  no network).
- `demo-server::first_missing_key` correctly resolves the `"openai"` branch (unit test, mirrors
  the existing Anthropic-path test added in RFC 0045).
- Full workspace: `cargo build/test/clippy/fmt` clean.

## Acceptance Criteria

- [x] `OpenAiProvider` implements `LlmProvider` and always sends `temperature: 0`.
- [x] `config.llm.provider = "openai"` routes `build_llm_provider` to `OpenAiProvider`, still
      wrapped in `CachedLlmProvider`.
- [x] `demo-server`'s boot-time key check handles the `"openai"` provider correctly.
- [x] No new workspace dependency added.
- [x] Manual: pointed a real `OPENAI_API_KEY` at the demo server and confirmed live, cited `/ask`
      answers against both catalog repos (`fd`, EKOS-self) — see devlog_46 for the full
      pre-vetting results, including a real, reproducible finding that citation compliance is
      inconsistent with `gpt-4o-mini` (roughly half of reasonable single-keyword questions return
      real citations; the rest return an ungrounded-looking but still factually-correct answer
      with an empty `cited_evidence` block) and that broad/hub-like query terms against the larger
      EKOS-self ledger can exceed the model's context/rate limits entirely. Neither finding is
      specific to this RFC's provider-selection code — both are pre-existing `AiRuntime::ask`
      behavior (`gather_context`'s unbounded-by-term-frequency retrieval, and a system prompt
      evidently tuned primarily against Claude) now surfaced for the first time by testing against
      a second real provider.
