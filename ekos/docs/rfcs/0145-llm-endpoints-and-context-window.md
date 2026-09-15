# RFC 0145 — OpenAI-compatible endpoints and an explicit Ollama context window

**Status:** Accepted (per user direction, 2026-09-15 — "small context window in ollama; can we use a cheap cloud model, e.g. opencode.ai/zen")
**Author:** EKOS team
**Created:** 2026-09-15
**Builds on:** RFC 0008 (LLM cache + `temperature: 0` contract), RFC 0021 (Ollama provider), RFC 0046 (OpenAI provider), RFC 0138 (eval harness)

---

## Motivation

Two independent limits sit between the knowledge EKOS retrieves and the answer a model writes.

**1. Ollama silently truncates the prompt.** `OllamaProvider` sends only `temperature` and
`num_predict`. It never sends `num_ctx`, so Ollama uses its own default window (4,096 tokens on
0.17.x, less on older builds) and silently drops the *front* of any longer prompt, which is
where the system prompt lives. Nothing in EKOS can notice: the response just gets worse. In the
devlog_183 eval reports, the largest recorded `input_tokens` is 3,875, which fits a 4k window
filling up rather than prompts naturally stopping just short of 4k. Meanwhile `[ai] max-context-chars` defaults to 200,000 chars
(about 50k tokens) of evidence the model can never see.

**2. There is no way to point EKOS at a cheap hosted model.** `OpenAiProvider` hardcodes
`https://api.openai.com/v1/chat/completions` and reads its model only from `OPENAI_MODEL`, ignoring
`[llm] model` (the Ollama provider was fixed for the same bug earlier; this one never was). Many
inexpensive hosts speak the same Chat Completions protocol: OpenCode Zen
(`https://opencode.ai/zen/v1`, e.g. DeepSeek V4 Flash at $0.14/$0.28 per 1M tokens), OpenRouter,
DeepSeek, Groq, and self-hosted vLLM or llama.cpp servers. None of them can be used today.

## Design

### `[llm] context-window` → Ollama `num_ctx`

```toml
[llm]
provider = "ollama"
model = "llama3:latest"
context-window = 8192     # optional
```

- New `LlmConfig::context_window: Option<u32>`. Precedence: `[llm] context-window`, then the
  `OLLAMA_NUM_CTX` env var, then **8192**, a safe default for every model this project uses
  (llama3's native window).
- `OllamaProvider` sends `options.num_ctx` on every request, streaming and non-streaming.
- **Truncation warning.** When a response reports `prompt_eval_count + num_predict >= num_ctx`,
  the provider logs a `warn!` naming the window and suggesting a larger `context-window`. The
  truncation stops being invisible.
- `ekos doctor` prints the effective context window for an Ollama workspace.

### `[llm] base-url` → any OpenAI-compatible endpoint

```toml
[llm]
provider = "openai"
base-url = "https://opencode.ai/zen/v1"
model = "deepseek-v4-flash"
api-key-env = "OPENCODE_API_KEY"
```

- New `LlmConfig::base_url: Option<String>`. Precedence: `[llm] base-url`, then `OPENAI_BASE_URL`,
  then `https://api.openai.com/v1`. The provider posts to `{base_url}/chat/completions`, with any
  trailing `/` stripped.
- Model precedence becomes `[llm] model`, then `OPENAI_MODEL`, then `gpt-4o-mini`, matching the Ollama provider.
- `api-key-env` already works (devlog_180 fixed its per-provider default).
- `provider = "ollama"` ignores `base-url`: Ollama keeps `OLLAMA_BASE_URL`, whose API shape
  (`/api/chat`) differs.

### Cache correctness

The RFC 0008 cache key is `SHA-256(model ‖ prompt_version ‖ system ‖ user ‖ history)`. Two new
settings change a response without changing any of those inputs:

- a different `num_ctx` (the same prompt, truncated differently), and
- a different `base-url` serving a model with the same name (Zen vs. OpenRouter `deepseek-…`).

Replaying a cached answer across either would be silently wrong. Eval comparisons would be the
worst hit, because they would replay the old truncated answers. So `LlmProvider` gains
`fn cache_namespace(&self) -> Option<String>` (default `None`). `CachedLlmProvider` folds it into
the key **only when `Some`**:

- `OllamaProvider` returns `num_ctx=<n>`.
- `OpenAiProvider` returns the base URL only when it is not the OpenAI default.

Every existing key for Anthropic, default OpenAI, and the mock stays byte-identical. Ollama
entries written before this RFC are effectively invalidated. That is intended: they were computed
under an unknown, smaller window.

## Non-goals

- A per-provider automatic cap on `max-context-chars`. A cloud model's window is large, and a
  local model's is now explicit and warned about. Tuning `[ai] max-context-chars` stays a config
  decision.
- Zen's Anthropic-Messages and Responses-API endpoints (Claude/GPT models on Zen). The
  Chat Completions path already covers the cheap models. `AnthropicProvider` base-URL
  configuration can follow the same pattern if ever needed.
- Automatic cost tracking. Token counts are already reported per eval scenario.

## Cost and data notes (for users)

Hosted endpoints bill per token, outside any subscription. A full 101-scenario `ekos eval run` at
about 2k input tokens per scenario is about 0.2M tokens, a few cents on a $0.14/M model. Free-tier
models on some hosts (including Zen's "free" models) may use submitted prompts for training.
RFC 0043 redaction still runs before any content is ledgered, but the prompt carries real source
excerpts, so prefer zero-retention models for private code.

## Testing

- `ollama`: `num_ctx` serialized when set; precedence (config > env > default); streaming body
  carries it too; truncation predicate.
- `openai`: endpoint built from base URL (trailing slash tolerated); `[llm] model` wins over
  `OPENAI_MODEL`; `cache_namespace` is `None` for the default URL and `Some` otherwise.
- `cache`: key unchanged when namespace is `None`; different when `Some`.
