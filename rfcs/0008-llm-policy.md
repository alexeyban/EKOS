# RFC 0008 — LLM Policy: Determinism, Caching, Model Pinning

**Status:** Accepted  
**Author:** alexeyban  
**Created:** 2026-07-02  
**Gating:** Phase 6

---

## Motivation

EKOS compiler passes must be deterministic: same inputs → same outputs. LLMs are inherently
non-deterministic by default. This RFC defines the policy that makes LLM-backed compiler passes
behave like deterministic functions — and the enforcement mechanisms that make it verifiable.

---

## Design

### Determinism contract

Every LLM call in EKOS must satisfy:

1. **Temperature = 0** on every request. This is the primary determinism lever.
2. **Model pinned** — the model name (e.g., `claude-sonnet-4-6`) is part of the cache key.
   Upgrading the model creates new cache entries; old builds are unaffected.
3. **Prompts are versioned** — each prompt template carries a version string that is part of
   the cache key. Changing a prompt invalidates its cache.
4. **Structured output** — all LLM responses are parsed as JSON. Free-text responses are rejected.
   If the model cannot produce valid JSON, the pass emits a diagnostic and falls back to
   structural-only output.

### Cache

```
Cache key = SHA-256(model + "\x00" + prompt_version + "\x00" + system_prompt + "\x00" + user_message)
Cache store = .ekos/llm-cache/<2-hex-prefix>/<64-hex>.json
```

Cache entries are permanent unless explicitly cleared. The same structure as the artifact store
(Git object-store layout) so the same `FileSystemArtifactStore` pattern can be reused.

A cache miss always calls the live API. A cache hit returns the stored response without a network
call, even if the model has been updated (the model name is in the key, so a version bump causes
new cache misses).

### `LlmProvider` trait

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn model_name(&self) -> &str;
    async fn complete(&self, req: &LlmRequest) -> Result<LlmResponse, LlmError>;
}

pub struct LlmRequest {
    pub system: String,
    pub user: String,
    pub prompt_version: &'static str,  // e.g. "sql-analyzer-v1"
    pub max_tokens: u32,
}
```

`CachedLlmProvider<T>` wraps any `LlmProvider`, checks the cache before calling `T::complete`,
and writes the response on a miss.

### Cost control

- `max_tokens` on every request (default: 4096 for analysis tasks).
- Batch SQL files: one LLM call per file, not per table.
- Cache means a full rebuild costs zero API calls if nothing changed.

### API key configuration

```toml
# ekos.toml
[llm]
provider = "anthropic"
model = "claude-sonnet-4-6"
api-key-env = "ANTHROPIC_API_KEY"
```

If `api-key-env` is not set or the env var is missing:
- Cache hits still work (offline mode).
- Cache misses emit a `Warning` diagnostic and return an empty enrichment; structural analysis
  still runs.

---

## Alternatives Considered

- **Seed / top-k sampling** — rejected; seed support is not universal across providers. Temperature=0
  is universally supported and well-documented.
- **In-process cache (HashMap)** — rejected; doesn't survive process restarts. Disk cache means
  CI builds are deterministic across machines once the cache is seeded.
- **Streaming responses** — not needed for analysis tasks; add in v1.0 if needed for UX.

---

## Acceptance Criteria

- [ ] `CachedLlmProvider` returns cached response without network call on second invocation.
- [ ] Cache key includes model name and prompt version.
- [ ] `AnthropicProvider` always sends `temperature: 0`.
- [ ] If `ANTHROPIC_API_KEY` is absent, structural SQL analysis still produces `KirObject`s.
- [ ] Unit test verifies cache hit vs. miss without a live API.
