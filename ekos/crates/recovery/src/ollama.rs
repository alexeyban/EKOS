//! Local Ollama backend for `LlmProvider` (RFC 0021).
//!
//! No API key — reads `OLLAMA_BASE_URL` (default `http://localhost:11434`)
//! and `OLLAMA_MODEL` (default `llama3.1:8b`). Always sends
//! `temperature: 0`, same determinism guarantee as `AnthropicProvider`.
//! Construction cannot fail (there is no key to be missing); an
//! unreachable daemon surfaces as an ordinary `LlmError::Http` on the
//! first `complete()` call instead.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::llm::{LlmError, LlmProvider, LlmRequest, LlmResponse, stream_lines};

const DEFAULT_BASE_URL: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "llama3.1:8b";

pub struct OllamaProvider {
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    /// Create from environment: `OLLAMA_BASE_URL` / `OLLAMA_MODEL`, each
    /// falling back to a sane local default. Cannot fail.
    pub fn from_env() -> Self {
        Self::from_env_with_model(None)
    }

    /// Same as [`from_env`](Self::from_env), but `model_override` (`[llm].model` in
    /// `ekos.toml`) takes priority over `OLLAMA_MODEL`, which in turn takes priority over the
    /// built-in default — previously `[llm].model` was silently ignored for the Ollama provider,
    /// only ever consulted for Anthropic/OpenAI.
    pub fn from_env_with_model(model_override: Option<&str>) -> Self {
        let base_url =
            std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let model = model_override
            .map(str::to_string)
            .or_else(|| std::env::var("OLLAMA_MODEL").ok())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        Self::new(model, base_url)
    }

    pub fn new(model: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }

    /// The request body `complete()` sends — split out so determinism
    /// (temperature: 0 regardless of caller input) is directly testable
    /// without a live daemon.
    fn build_request<'a>(&'a self, req: &'a LlmRequest<'_>) -> ApiRequest<'a> {
        let mut messages = Vec::with_capacity(req.history.len() + 2);
        messages.push(ApiMessage {
            role: "system",
            content: req.system,
        });
        messages.extend(req.history.iter().map(|m| ApiMessage {
            role: m.role,
            content: m.content,
        }));
        messages.push(ApiMessage {
            role: "user",
            content: req.user,
        });
        ApiRequest {
            model: &self.model,
            stream: false,
            messages,
            options: ApiOptions {
                temperature: 0.0,
                num_predict: req.max_tokens,
            },
        }
    }
}

// ── Wire types for Ollama's /api/chat ───────────────────────────────────────

#[derive(Serialize)]
struct ApiRequest<'a> {
    model: &'a str,
    stream: bool,
    /// RFC 0099: `[system] + history + [current user turn]`, same shape as
    /// OpenAI's Chat Completions (`/api/chat` mirrors that convention).
    messages: Vec<ApiMessage<'a>>,
    options: ApiOptions,
}

#[derive(Serialize)]
struct ApiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ApiOptions {
    temperature: f32,
    num_predict: u32,
}

#[derive(Deserialize)]
struct ApiResponse {
    message: ApiResponseMessage,
    model: String,
    #[serde(default)]
    prompt_eval_count: u32,
    #[serde(default)]
    eval_count: u32,
}

#[derive(Deserialize)]
struct ApiResponseMessage {
    content: String,
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    async fn complete(&self, req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        let body = self.build_request(req);

        let http_resp = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await?;

        let status = http_resp.status().as_u16();
        if !http_resp.status().is_success() {
            let body_text = http_resp.text().await.unwrap_or_default();
            return Err(LlmError::Api {
                status,
                body: body_text,
            });
        }

        let api_resp: ApiResponse = http_resp.json().await?;
        Ok(LlmResponse {
            content: api_resp.message.content,
            model: api_resp.model,
            input_tokens: api_resp.prompt_eval_count,
            output_tokens: api_resp.eval_count,
        })
    }

    /// Real NDJSON streaming against `/api/chat`'s `stream: true` mode
    /// (RFC 0098) — each line is a whole JSON object (no SSE `data:`
    /// prefix, unlike Anthropic/OpenAI), carrying one `message.content`
    /// piece until the final line (`done: true`) also carries real
    /// `prompt_eval_count`/`eval_count` token usage.
    async fn complete_stream(
        &self,
        req: &LlmRequest<'_>,
        on_chunk: &mut (dyn FnMut(String) + Send),
    ) -> Result<LlmResponse, LlmError> {
        let mut body = self.build_request(req);
        body.stream = true;

        let http_resp = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await?;

        let status = http_resp.status().as_u16();
        if !http_resp.status().is_success() {
            let body_text = http_resp.text().await.unwrap_or_default();
            return Err(LlmError::Api {
                status,
                body: body_text,
            });
        }

        let mut acc = StreamAccumulator {
            model: self.model.clone(),
            ..Default::default()
        };
        stream_lines(http_resp, |line| {
            apply_stream_line(&mut acc, line, on_chunk)
        })
        .await?;

        Ok(LlmResponse {
            content: acc.content,
            model: acc.model,
            input_tokens: acc.input_tokens,
            output_tokens: acc.output_tokens,
        })
    }
}

/// Streamed-response state, built up one NDJSON line at a time — kept as a
/// plain struct (not inline closure captures) specifically so
/// `apply_stream_line` is a pure function unit-testable with synthetic line
/// strings, no live daemon or mock HTTP server needed.
#[derive(Default)]
struct StreamAccumulator {
    content: String,
    model: String,
    input_tokens: u32,
    output_tokens: u32,
}

/// Applies one `/api/chat` NDJSON line to `acc`, forwarding any new content
/// piece to `on_chunk`. A line that isn't valid JSON, or that carries
/// neither `message.content` nor a `done: true` usage payload, is silently
/// ignored — forward-compatible with fields this doesn't recognize.
fn apply_stream_line(acc: &mut StreamAccumulator, line: &str, on_chunk: &mut dyn FnMut(String)) {
    let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    if let Some(m) = event.get("model").and_then(|v| v.as_str()) {
        acc.model = m.to_string();
    }
    if let Some(text) = event
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .filter(|t| !t.is_empty())
    {
        acc.content.push_str(text);
        on_chunk(text.to_string());
    }
    if event.get("done").and_then(|d| d.as_bool()) == Some(true) {
        if let Some(t) = event.get("prompt_eval_count").and_then(|v| v.as_u64()) {
            acc.input_tokens = t as u32;
        }
        if let Some(t) = event.get("eval_count").and_then(|v| v.as_u64()) {
            acc.output_tokens = t as u32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_name_reflects_construction() {
        let provider = OllamaProvider::new("llama3.1:8b", "http://localhost:11434");
        assert_eq!(provider.model_name(), "llama3.1:8b");
    }

    // ── RFC 0099: multi-turn history ─────────────────────────────────────

    #[test]
    fn build_request_places_history_between_system_and_current_user_turn() {
        use crate::llm::Message;
        let history = [
            Message {
                role: "user",
                content: "first question",
            },
            Message {
                role: "assistant",
                content: "first answer",
            },
        ];
        let provider = OllamaProvider::new("m", "http://x");
        let req = LlmRequest {
            system: "sys",
            user: "second question",
            prompt_version: "v1",
            max_tokens: 100,
            history: &history,
        };
        let body = provider.build_request(&req);
        assert_eq!(body.messages.len(), 4);
        assert_eq!(body.messages[0].role, "system");
        assert_eq!(body.messages[1].role, "user");
        assert_eq!(body.messages[1].content, "first question");
        assert_eq!(body.messages[2].role, "assistant");
        assert_eq!(body.messages[2].content, "first answer");
        assert_eq!(body.messages[3].role, "user");
        assert_eq!(body.messages[3].content, "second question");
    }

    #[test]
    fn from_env_falls_back_to_defaults_when_unset() {
        // SAFETY: test-local env mutation, no concurrent access to these
        // specific vars elsewhere in the suite.
        unsafe {
            std::env::remove_var("OLLAMA_BASE_URL");
            std::env::remove_var("OLLAMA_MODEL");
        }
        let provider = OllamaProvider::from_env();
        assert_eq!(provider.model_name(), DEFAULT_MODEL);
        assert_eq!(provider.base_url, DEFAULT_BASE_URL);

        // Precedence chain for `from_env_with_model`, in the same test function as the
        // defaults check above: both mutate the same process-global `OLLAMA_MODEL` var, and
        // Rust runs test functions in parallel by default, so this can't safely be split into
        // separate `#[test]` functions without a race (confirmed live — it raced on the first
        // attempt).
        // SAFETY: test-local env mutation, no concurrent access to this var elsewhere.
        unsafe {
            std::env::set_var("OLLAMA_MODEL", "env-model");
        }
        assert_eq!(
            OllamaProvider::from_env_with_model(None).model_name(),
            "env-model",
            "no override given — should fall back to OLLAMA_MODEL"
        );
        assert_eq!(
            OllamaProvider::from_env_with_model(Some("configured-model")).model_name(),
            "configured-model",
            "explicit override should win over OLLAMA_MODEL"
        );
        unsafe {
            std::env::remove_var("OLLAMA_MODEL");
        }
    }

    /// RFC 0008/0021 determinism contract: temperature is always 0,
    /// regardless of what the caller passes in the request.
    #[test]
    fn request_always_sets_temperature_zero() {
        let provider = OllamaProvider::new("m", "http://x");
        let req = LlmRequest {
            system: "sys",
            user: "user",
            prompt_version: "v1",
            max_tokens: 123,
            history: &[],
        };
        let body = provider.build_request(&req);
        assert_eq!(body.options.temperature, 0.0);
        assert_eq!(body.options.num_predict, 123);
        assert!(!body.stream);
        assert_eq!(body.messages[0].role, "system");
        assert_eq!(body.messages[1].role, "user");
    }

    // ── RFC 0098: streaming NDJSON line parsing ─────────────────────────

    #[test]
    fn apply_stream_line_accumulates_content_chunks_in_order() {
        let mut acc = StreamAccumulator::default();
        let mut chunks = Vec::new();
        let mut on_chunk = |s: String| chunks.push(s);

        apply_stream_line(
            &mut acc,
            r#"{"model":"llama3","message":{"role":"assistant","content":"Hel"},"done":false}"#,
            &mut on_chunk,
        );
        apply_stream_line(
            &mut acc,
            r#"{"model":"llama3","message":{"role":"assistant","content":"lo"},"done":false}"#,
            &mut on_chunk,
        );

        assert_eq!(chunks, vec!["Hel".to_string(), "lo".to_string()]);
        assert_eq!(acc.content, "Hello");
        assert_eq!(acc.model, "llama3");
    }

    #[test]
    fn apply_stream_line_captures_usage_only_from_the_done_line() {
        let mut acc = StreamAccumulator::default();
        let mut on_chunk = |_: String| {};

        apply_stream_line(
            &mut acc,
            r#"{"model":"llama3","message":{"role":"assistant","content":"hi"},"done":false}"#,
            &mut on_chunk,
        );
        assert_eq!(acc.input_tokens, 0);
        assert_eq!(acc.output_tokens, 0);

        apply_stream_line(
            &mut acc,
            r#"{"model":"llama3","message":{"role":"assistant","content":""},"done":true,"prompt_eval_count":32,"eval_count":10}"#,
            &mut on_chunk,
        );
        assert_eq!(acc.input_tokens, 32);
        assert_eq!(acc.output_tokens, 10);
        assert_eq!(
            acc.content, "hi",
            "the empty final content must not be appended"
        );
    }

    #[test]
    fn apply_stream_line_ignores_malformed_json_without_panicking() {
        let mut acc = StreamAccumulator::default();
        let mut on_chunk = |_: String| panic!("must not be called for malformed input");
        apply_stream_line(&mut acc, "not json at all", &mut on_chunk);
        assert_eq!(acc.content, "");
    }
}
