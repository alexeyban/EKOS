//! OpenAI backend for `LlmProvider` (RFC 0046).
//!
//! Reads the API key from the given env var (default `OPENAI_API_KEY`). Always sends
//! `temperature: 0`. Model defaults to `gpt-4o-mini`, overridable via `OPENAI_MODEL`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::llm::{LlmError, LlmProvider, LlmRequest, LlmResponse, stream_lines};

const OPENAI_API_URL: &str = "https://api.openai.com/v1/chat/completions";
const DEFAULT_MODEL: &str = "gpt-4o-mini";
const DEFAULT_KEY_ENV: &str = "OPENAI_API_KEY";

pub struct OpenAiProvider {
    model: String,
    api_key: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
    /// Create from environment. Returns `Err(LlmError::NoApiKey)` if the env var is absent.
    pub fn from_env() -> Result<Self, LlmError> {
        Self::from_env_var(DEFAULT_KEY_ENV)
    }

    pub fn from_env_var(env_var: &str) -> Result<Self, LlmError> {
        let api_key =
            std::env::var(env_var).map_err(|_| LlmError::NoApiKey(env_var.to_string()))?;
        let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        Ok(Self::new(model, api_key))
    }

    pub fn new(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        }
    }
}

// ── Wire types for the OpenAI Chat Completions API ──────────────────────────

#[derive(Serialize)]
struct ApiRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    temperature: f32,
    messages: Vec<ApiMessage<'a>>,
    stream: bool,
    /// Only set (and only meaningful) for streaming requests — asks the API
    /// to emit one final chunk carrying real token usage, since the
    /// streaming wire format otherwise never includes it at all (unlike the
    /// non-streaming response, which always has a top-level `usage`).
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Serialize)]
struct ApiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// RFC 0099: `[system] + history + [current user turn]`, in order — Chat
/// Completions has no separate top-level system field the way Anthropic
/// does, so it always occupies the first slot in the same array.
fn build_messages<'a>(req: &LlmRequest<'a>) -> Vec<ApiMessage<'a>> {
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
    messages
}

#[derive(Deserialize)]
struct ApiResponse {
    choices: Vec<ApiChoice>,
    model: String,
    usage: ApiUsage,
}

#[derive(Deserialize)]
struct ApiChoice {
    message: ApiResponseMessage,
}

#[derive(Deserialize)]
struct ApiResponseMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct ApiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    async fn complete(&self, req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        let body = ApiRequest {
            model: &self.model,
            max_tokens: req.max_tokens,
            temperature: 0.0,
            messages: build_messages(req),
            stream: false,
            stream_options: None,
        };

        let http_resp = self
            .client
            .post(OPENAI_API_URL)
            .bearer_auth(&self.api_key)
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
        let content = api_resp
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .unwrap_or_default();

        Ok(LlmResponse {
            content,
            model: api_resp.model,
            input_tokens: api_resp.usage.prompt_tokens,
            output_tokens: api_resp.usage.completion_tokens,
        })
    }

    /// Real SSE streaming against the Chat Completions API's
    /// `choices[0].delta.content` chunks (RFC 0098), with
    /// `stream_options.include_usage` requested so a final usage-only chunk
    /// (empty `choices`, top-level `usage`) carries real token counts —
    /// without it, a streamed OpenAI response has no usage data anywhere,
    /// unlike the non-streaming path. The terminal `data: [DONE]` line and
    /// any line that isn't valid JSON are silently skipped, not an error.
    async fn complete_stream(
        &self,
        req: &LlmRequest<'_>,
        on_chunk: &mut (dyn FnMut(String) + Send),
    ) -> Result<LlmResponse, LlmError> {
        let body = ApiRequest {
            model: &self.model,
            max_tokens: req.max_tokens,
            temperature: 0.0,
            messages: build_messages(req),
            stream: true,
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
        };

        let http_resp = self
            .client
            .post(OPENAI_API_URL)
            .bearer_auth(&self.api_key)
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

/// Streamed-response state, built up one SSE line at a time — kept as a
/// plain struct (not inline closure captures) specifically so
/// `apply_stream_line` is a pure function unit-testable with synthetic line
/// strings, no live API key or mock HTTP server needed.
#[derive(Default)]
struct StreamAccumulator {
    content: String,
    model: String,
    input_tokens: u32,
    output_tokens: u32,
}

/// Applies one Chat-Completions SSE line to `acc`, forwarding any new
/// `choices[0].delta.content` text piece to `on_chunk`. The terminal
/// `data: [DONE]` line, a non-JSON payload, or a line with no `data:`
/// prefix is silently ignored.
fn apply_stream_line(acc: &mut StreamAccumulator, line: &str, on_chunk: &mut dyn FnMut(String)) {
    let Some(payload) = line.strip_prefix("data:") else {
        return;
    };
    let payload = payload.trim();
    if payload == "[DONE]" {
        return;
    }
    let Ok(event) = serde_json::from_str::<serde_json::Value>(payload) else {
        return;
    };
    if let Some(m) = event.get("model").and_then(|v| v.as_str()) {
        acc.model = m.to_string();
    }
    if let Some(text) = event
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("delta"))
        .and_then(|d| d.get("content"))
        .and_then(|t| t.as_str())
    {
        acc.content.push_str(text);
        on_chunk(text.to_string());
    }
    if let Some(usage) = event.get("usage") {
        if let Some(t) = usage.get("prompt_tokens").and_then(|v| v.as_u64()) {
            acc.input_tokens = t as u32;
        }
        if let Some(t) = usage.get("completion_tokens").and_then(|v| v.as_u64()) {
            acc.output_tokens = t as u32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_name_returns_the_constructed_model() {
        let provider = OpenAiProvider::new("gpt-4o-mini", "test-key");
        assert_eq!(provider.model_name(), "gpt-4o-mini");
    }

    // ── RFC 0099: multi-turn history ─────────────────────────────────────

    #[test]
    fn build_messages_with_no_history_is_system_then_current_user_turn() {
        let req = LlmRequest {
            system: "sys",
            user: "current question",
            prompt_version: "v1",
            max_tokens: 100,
            history: &[],
        };
        let messages = build_messages(&req);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[1].content, "current question");
    }

    #[test]
    fn build_messages_places_history_between_system_and_current_user_turn() {
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
        let req = LlmRequest {
            system: "sys",
            user: "second question",
            prompt_version: "v1",
            max_tokens: 100,
            history: &history,
        };
        let messages = build_messages(&req);
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[1].content, "first question");
        assert_eq!(messages[2].role, "assistant");
        assert_eq!(messages[2].content, "first answer");
        assert_eq!(messages[3].role, "user");
        assert_eq!(messages[3].content, "second question");
    }

    #[test]
    fn request_body_always_sets_temperature_zero() {
        let req = LlmRequest {
            system: "sys",
            user: "usr",
            prompt_version: "v1",
            max_tokens: 100,
            history: &[],
        };
        let body = ApiRequest {
            model: "gpt-4o-mini",
            max_tokens: req.max_tokens,
            temperature: 0.0,
            messages: build_messages(&req),
            stream: false,
            stream_options: None,
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["temperature"], 0.0);
        assert_eq!(json["messages"][0]["role"], "system");
        assert_eq!(json["messages"][1]["role"], "user");
    }

    // ── RFC 0098: streaming SSE line parsing ────────────────────────────

    #[test]
    fn apply_stream_line_accumulates_delta_content_in_order() {
        let mut acc = StreamAccumulator::default();
        let mut chunks = Vec::new();
        let mut on_chunk = |s: String| chunks.push(s);

        apply_stream_line(
            &mut acc,
            r#"data: {"model":"gpt-4o-mini","choices":[{"index":0,"delta":{"content":"Hel"},"finish_reason":null}]}"#,
            &mut on_chunk,
        );
        apply_stream_line(
            &mut acc,
            r#"data: {"model":"gpt-4o-mini","choices":[{"index":0,"delta":{"content":"lo"},"finish_reason":null}]}"#,
            &mut on_chunk,
        );

        assert_eq!(chunks, vec!["Hel".to_string(), "lo".to_string()]);
        assert_eq!(acc.content, "Hello");
        assert_eq!(acc.model, "gpt-4o-mini");
    }

    #[test]
    fn apply_stream_line_reads_usage_from_the_final_empty_choices_chunk() {
        // The real shape when stream_options.include_usage is set: the
        // usage-carrying chunk has an empty `choices` array, not a delta.
        let mut acc = StreamAccumulator::default();
        let mut on_chunk = |_: String| {};
        apply_stream_line(
            &mut acc,
            r#"data: {"choices":[],"usage":{"prompt_tokens":50,"completion_tokens":8}}"#,
            &mut on_chunk,
        );
        assert_eq!(acc.input_tokens, 50);
        assert_eq!(acc.output_tokens, 8);
    }

    #[test]
    fn apply_stream_line_ignores_the_done_sentinel_and_malformed_lines() {
        let mut acc = StreamAccumulator::default();
        let mut on_chunk = |_: String| panic!("must not be called");
        apply_stream_line(&mut acc, "data: [DONE]", &mut on_chunk);
        apply_stream_line(&mut acc, "data: not valid json", &mut on_chunk);
        apply_stream_line(&mut acc, "not even an sse line", &mut on_chunk);
        assert_eq!(acc.content, "");
    }
}
