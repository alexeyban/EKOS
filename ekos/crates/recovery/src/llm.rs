//! LLM provider trait and types (RFC 0008).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// One prior turn in a multi-turn conversation (RFC 0099) — a clean
/// question/answer pair, never the raw grounded prompt a turn was actually
/// sent with (which can carry a large retrieved-context JSON blob nobody
/// wants repeated verbatim in every later turn).
pub struct Message<'a> {
    /// `"user"` or `"assistant"` — the two roles every real provider's wire
    /// format needs; never validated beyond that here, each provider maps
    /// it directly onto its own API's role field.
    pub role: &'a str,
    pub content: &'a str,
}

/// A single LLM completion request.
pub struct LlmRequest<'a> {
    /// System-role instructions (persona + output format).
    pub system: &'a str,
    /// User-role message containing the content to analyse.
    pub user: &'a str,
    /// Short identifier baked into the cache key; bump to invalidate cached responses.
    pub prompt_version: &'static str,
    /// Hard cap on generated tokens.
    pub max_tokens: u32,
    /// Prior conversation turns (RFC 0099), oldest first, inserted between
    /// the system prompt and this request's own `user` message. Empty for
    /// every single-shot caller (analyzer passes, `docs --prose`,
    /// `marketing`, the NL-to-SQL bridge, …) — only `ekos ask --session`
    /// populates this.
    pub history: &'a [Message<'a>],
}

/// Successful LLM completion result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub content: String,
    pub model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Error returned by any `LlmProvider` implementation.
#[derive(Debug, Error)]
pub enum LlmError {
    #[error("no API key configured (env var: {0})")]
    NoApiKey(String),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api error {status}: {body}")]
    Api { status: u16, body: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

impl LlmError {
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

/// Provider-agnostic interface for LLM completions.
///
/// # Contract (RFC 0008)
/// - Every implementation MUST send `temperature: 0`.
/// - Every implementation MUST include the model name in the cache key.
/// - Structured (JSON) output is expected; callers reject free-text responses.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn model_name(&self) -> &str;
    async fn complete(&self, req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError>;

    /// Same contract and final return value as [`Self::complete`], but calls
    /// `on_chunk` with each piece of generated text as it becomes available,
    /// in the order the provider emits it, before returning (RFC 0098).
    /// Takes an owned `String` per chunk rather than `&str` — `async_trait`'s
    /// lifetime-elision rewriting doesn't preserve the higher-ranked
    /// `for<'r> FnMut(&'r str)` a borrowed-str callback parameter needs here,
    /// confirmed live (a real, non-obvious borrow-check error, not a
    /// hypothetical); an owned `String` sidesteps it entirely, at the cost of
    /// one allocation per chunk — negligible at LLM token-chunk granularity.
    ///
    /// Default implementation falls back to [`Self::complete`] and reports
    /// the whole response as a single chunk — real incremental streaming is
    /// a per-provider opt-in. `AnthropicProvider`/`OpenAiProvider`/
    /// `OllamaProvider` override this with true SSE/NDJSON streaming;
    /// `MockLlmProvider` and other test-only implementors don't need to.
    async fn complete_stream(
        &self,
        req: &LlmRequest<'_>,
        on_chunk: &mut (dyn FnMut(String) + Send),
    ) -> Result<LlmResponse, LlmError> {
        let resp = self.complete(req).await?;
        on_chunk(resp.content.clone());
        Ok(resp)
    }

    /// Cumulative `(hits, misses)` for a disk-backed cache wrapping this provider, if any (RFC
    /// 0138 — the eval harness's "tokens saved" metric samples this before/after each call to
    /// tell whether that specific call was served from cache). `None` by default — only
    /// `CachedLlmProvider` overrides it; every other implementor (including a `CachedLlmProvider`
    /// wrapping another `CachedLlmProvider`, which nobody does) reports no cache at all.
    fn cache_stats(&self) -> Option<(u64, u64)> {
        None
    }
}

/// Reads an HTTP response body incrementally and calls `on_line` once per
/// non-empty, trimmed line as it arrives — the shared low-level primitive
/// every streaming `LlmProvider` implementation's SSE (Anthropic/OpenAI) or
/// NDJSON (Ollama) parsing builds on. A line split across two chunks is
/// buffered and only yielded once complete. Uses `Response::chunk()` rather
/// than `bytes_stream()` deliberately — it needs no extra `reqwest`
/// feature/dependency, and this shared helper is the only place that logic
/// needs to exist at all.
pub(crate) async fn stream_lines(
    mut resp: reqwest::Response,
    mut on_line: impl FnMut(&str),
) -> Result<(), LlmError> {
    let mut buf = String::new();
    while let Some(chunk) = resp.chunk().await? {
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim().to_string();
            buf.drain(..=pos);
            if !line.is_empty() {
                on_line(&line);
            }
        }
    }
    let trailing = buf.trim();
    if !trailing.is_empty() {
        on_line(trailing);
    }
    Ok(())
}

/// In-process no-op provider for unit tests. Returns a fixed response without network calls.
pub struct MockLlmProvider {
    pub model: String,
    pub response: String,
}

impl MockLlmProvider {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            model: "mock-v1".into(),
            response: response.into(),
        }
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    async fn complete(&self, _req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        Ok(LlmResponse {
            content: self.response.clone(),
            model: self.model.clone(),
            input_tokens: 0,
            output_tokens: 0,
        })
    }
}
