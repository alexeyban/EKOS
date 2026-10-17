//! Anthropic Claude backend for `LlmProvider`.
//!
//! Reads the API key from the env var specified in `EkosConfig.llm.api_key_env`
//! (default: `ANTHROPIC_API_KEY`). Always sends `temperature: 0`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::llm::{LlmError, LlmProvider, LlmRequest, LlmResponse, stream_lines};

/// Without a trailing `/`; requests go to `{base}/messages`.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
const DEFAULT_KEY_ENV: &str = "ANTHROPIC_API_KEY";

pub struct AnthropicProvider {
    model: String,
    api_key: String,
    /// RFC 0197: any Messages-API host (OpenCode Zen: `https://opencode.ai/zen/v1`).
    base_url: String,
    /// RFC 0197: mark the system prompt cacheable.
    prompt_cache: bool,
    /// RFC 0197: set once the model refused `temperature` (current Claude models do).
    no_temperature: std::sync::atomic::AtomicBool,
    client: reqwest::Client,
}

impl AnthropicProvider {
    /// Create from environment. Returns `Err(LlmError::NoApiKey)` if the env var is absent.
    pub fn from_env() -> Result<Self, LlmError> {
        Self::from_env_var(DEFAULT_KEY_ENV)
    }

    pub fn from_env_var(env_var: &str) -> Result<Self, LlmError> {
        let api_key =
            std::env::var(env_var).map_err(|_| LlmError::NoApiKey(env_var.to_string()))?;
        Ok(Self::new(DEFAULT_MODEL, api_key))
    }

    /// RFC 0197: `[llm] model` and `base-url` honoured (the model was always
    /// `claude-sonnet-4-6` before).
    pub fn from_config(
        env_var: &str,
        model: Option<&str>,
        base_url: Option<&str>,
    ) -> Result<Self, LlmError> {
        let api_key =
            std::env::var(env_var).map_err(|_| LlmError::NoApiKey(env_var.to_string()))?;
        let mut p = Self::new(model.unwrap_or(DEFAULT_MODEL), api_key);
        if let Some(b) = base_url {
            p = p.with_base_url(b);
        }
        Ok(p)
    }

    pub fn new(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
            prompt_cache: false,
            no_temperature: std::sync::atomic::AtomicBool::new(false),
            client: reqwest::Client::new(),
        }
    }

    /// RFC 0197: point at another Messages-API host. A trailing `/` is tolerated.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    /// RFC 0197: send the system prompt as a cacheable block.
    pub fn with_prompt_cache(mut self, on: bool) -> Self {
        self.prompt_cache = on;
        self
    }

    fn endpoint(&self) -> String {
        format!("{}/messages", self.base_url)
    }

    fn body<'a>(&'a self, req: &LlmRequest<'a>, stream: bool) -> ApiRequest<'a> {
        let no_temp = self
            .no_temperature
            .load(std::sync::atomic::Ordering::Relaxed);
        ApiRequest {
            model: &self.model,
            max_tokens: req.max_tokens,
            temperature: (!no_temp).then_some(0.0),
            system: if self.prompt_cache {
                serde_json::json!([{"type": "text", "text": req.system,
                                    "cache_control": {"type": "ephemeral"}}])
            } else {
                serde_json::Value::String(req.system.to_string())
            },
            messages: build_messages(req),
            stream,
        }
    }

    /// POST, retrying once without `temperature` when the model refuses it.
    async fn send(
        &self,
        req: &LlmRequest<'_>,
        stream: bool,
    ) -> Result<reqwest::Response, LlmError> {
        let mut dropped_temperature = false;
        loop {
            let http_resp = self
                .client
                .post(self.endpoint())
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .json(&self.body(req, stream))
                .send()
                .await?;
            if http_resp.status().is_success() {
                return Ok(http_resp);
            }
            let status = http_resp.status().as_u16();
            let body = http_resp.text().await.unwrap_or_default();
            let had_temp = !self
                .no_temperature
                .load(std::sync::atomic::Ordering::Relaxed);
            // RFC 0197: some models refuse `temperature`, and not all say so (Mistral Large 4
            // on Zen answers a bare `{"object":"error"}`). Retry once without it; keep going
            // without only when that retry gets past the 400.
            if status == 400 && had_temp {
                self.no_temperature
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                dropped_temperature = true;
                continue;
            }
            if dropped_temperature && status == 400 {
                self.no_temperature
                    .store(false, std::sync::atomic::Ordering::Relaxed);
            }
            return Err(LlmError::Api { status, body });
        }
    }
}

// ── Wire types for the Anthropic Messages API ───────────────────────────────

#[derive(Serialize)]
struct ApiRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    /// A string, or one cacheable text block (RFC 0197).
    system: serde_json::Value,
    messages: Vec<ApiMessage<'a>>,
    stream: bool,
}

#[derive(Serialize)]
struct ApiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// RFC 0099: `history` (empty for every pre-multi-turn caller) plus the
/// current turn's `user` message, in order — the Messages API takes prior
/// turns as ordinary `user`/`assistant` messages in the same array, with
/// `system` staying the one top-level field it already was.
fn build_messages<'a>(req: &LlmRequest<'a>) -> Vec<ApiMessage<'a>> {
    let mut messages: Vec<ApiMessage<'a>> = req
        .history
        .iter()
        .map(|m| ApiMessage {
            role: m.role,
            content: m.content,
        })
        .collect();
    messages.push(ApiMessage {
        role: "user",
        content: req.user,
    });
    messages
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ApiContent>,
    model: String,
    usage: ApiUsage,
}

#[derive(Deserialize)]
struct ApiContent {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct ApiUsage {
    input_tokens: u32,
    output_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    /// RFC 0197: only a non-default host changes the key, so existing cache entries stay valid.
    fn cache_namespace(&self) -> Option<String> {
        (self.base_url != DEFAULT_BASE_URL).then(|| self.base_url.clone())
    }

    async fn complete(&self, req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        let http_resp = self.send(req, false).await?;

        let api_resp: ApiResponse = http_resp.json().await?;
        let content = api_resp
            .content
            .into_iter()
            .find(|c| c.kind == "text")
            .and_then(|c| c.text)
            .unwrap_or_default();

        Ok(LlmResponse {
            content,
            model: api_resp.model,
            input_tokens: api_resp.usage.input_tokens,
            output_tokens: api_resp.usage.output_tokens,
            cache_read_tokens: api_resp.usage.cache_read_input_tokens.unwrap_or(0),
            cache_write_tokens: api_resp.usage.cache_creation_input_tokens.unwrap_or(0),
        })
    }

    /// Real SSE streaming against the Messages API's `content_block_delta`
    /// events (RFC 0098). `message_start` carries the model name and input
    /// token count; each `content_block_delta` carries one `delta.text`
    /// piece, forwarded to `on_chunk` and accumulated into the final
    /// content; the terminal `message_delta` carries the output token
    /// count. Any line this doesn't recognize (a different `type`, or a
    /// non-JSON/non-`data:` SSE line like `event: ...`) is silently
    /// skipped, not an error — forward-compatible with new event types.
    async fn complete_stream(
        &self,
        req: &LlmRequest<'_>,
        on_chunk: &mut (dyn FnMut(String) + Send),
    ) -> Result<LlmResponse, LlmError> {
        let http_resp = self.send(req, true).await?;

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
            cache_read_tokens: acc.cache_read_tokens,
            cache_write_tokens: acc.cache_write_tokens,
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
    cache_read_tokens: u32,
    cache_write_tokens: u32,
}

/// Applies one Messages-API SSE line to `acc`, forwarding any new
/// `content_block_delta` text piece to `on_chunk`. A line with no `data:`
/// prefix (e.g. `event: ...`), non-JSON payload, or an event `type` this
/// doesn't recognize is silently ignored — forward-compatible with new
/// event types.
fn apply_stream_line(acc: &mut StreamAccumulator, line: &str, on_chunk: &mut dyn FnMut(String)) {
    let Some(payload) = line.strip_prefix("data:") else {
        return;
    };
    let Ok(event) = serde_json::from_str::<serde_json::Value>(payload.trim()) else {
        return;
    };
    match event.get("type").and_then(|t| t.as_str()) {
        Some("message_start") => {
            if let Some(message) = event.get("message") {
                if let Some(m) = message.get("model").and_then(|v| v.as_str()) {
                    acc.model = m.to_string();
                }
                let usage = |k: &str| {
                    message
                        .get("usage")
                        .and_then(|u| u.get(k))
                        .and_then(serde_json::Value::as_u64)
                        .map(|t| t as u32)
                };
                if let Some(tokens) = usage("input_tokens") {
                    acc.input_tokens = tokens;
                }
                acc.cache_read_tokens = usage("cache_read_input_tokens").unwrap_or(0);
                acc.cache_write_tokens = usage("cache_creation_input_tokens").unwrap_or(0);
            }
        }
        Some("content_block_delta") => {
            if let Some(text) = event
                .get("delta")
                .and_then(|d| d.get("text"))
                .and_then(|t| t.as_str())
            {
                acc.content.push_str(text);
                on_chunk(text.to_string());
            }
        }
        Some("message_delta") => {
            if let Some(tokens) = event
                .get("usage")
                .and_then(|u| u.get("output_tokens"))
                .and_then(serde_json::Value::as_u64)
            {
                acc.output_tokens = tokens as u32;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> LlmRequest<'static> {
        LlmRequest {
            system: "sys",
            user: "usr",
            prompt_version: "v1",
            max_tokens: 10,
            history: &[],
        }
    }

    #[test]
    fn rfc0197_model_and_host_come_from_config_and_cache_is_namespaced_by_host() {
        let p = AnthropicProvider::new("claude-haiku-5-5", "k")
            .with_base_url("https://opencode.ai/zen/v1/");
        assert_eq!(p.endpoint(), "https://opencode.ai/zen/v1/messages");
        assert_eq!(
            p.cache_namespace().as_deref(),
            Some("https://opencode.ai/zen/v1")
        );
        assert_eq!(AnthropicProvider::new("m", "k").cache_namespace(), None);
    }

    #[test]
    fn rfc0197_temperature_is_dropped_after_a_refusal_and_the_system_can_be_cached() {
        let p = AnthropicProvider::new("claude-haiku-5-5", "k");
        let r = req();
        let v = serde_json::to_value(p.body(&r, false)).unwrap();
        assert_eq!(v["temperature"], 0.0);
        assert_eq!(v["system"], "sys");
        p.no_temperature
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let p = p.with_prompt_cache(true);
        let v = serde_json::to_value(p.body(&r, false)).unwrap();
        assert!(v.get("temperature").is_none());
        assert_eq!(v["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(v["system"][0]["text"], "sys");
    }

    #[test]
    fn rfc0197_cache_usage_is_read_from_a_recorded_zen_reply() {
        // claude-haiku-5-5 on OpenCode Zen, 2026-10-17 (id shortened).
        let recorded = r#"{"id":"msg_x","type":"message","role":"assistant","stop_reason":"end_turn",
          "model":"claude-haiku-5-5","content":[{"type":"text","text":"Paris"}],
          "usage":{"input_tokens":22,"output_tokens":5,"cache_creation_input_tokens":3,
                   "cache_read_input_tokens":7,"output_tokens_details":{"thinking_tokens":0}}}"#;
        let r: ApiResponse = serde_json::from_str(recorded).unwrap();
        assert_eq!(r.usage.input_tokens, 22);
        assert_eq!(r.usage.cache_read_input_tokens, Some(7));
        assert_eq!(r.usage.cache_creation_input_tokens, Some(3));
        let mut acc = StreamAccumulator::default();
        apply_stream_line(
            &mut acc,
            r#"data: {"type":"message_start","message":{"model":"claude-haiku-5-5","usage":{"input_tokens":4,"cache_read_input_tokens":9,"cache_creation_input_tokens":2}}}"#,
            &mut |_| {},
        );
        assert_eq!(
            (
                acc.input_tokens,
                acc.cache_read_tokens,
                acc.cache_write_tokens
            ),
            (4, 9, 2)
        );
    }

    #[test]
    fn model_name_returns_the_constructed_model() {
        let provider = AnthropicProvider::new("claude-sonnet-4-6", "test-key");
        assert_eq!(provider.model_name(), "claude-sonnet-4-6");
    }

    // ── RFC 0099: multi-turn history ─────────────────────────────────────

    #[test]
    fn build_messages_with_no_history_is_just_the_current_user_turn() {
        let req = LlmRequest {
            system: "sys",
            user: "current question",
            prompt_version: "v1",
            max_tokens: 100,
            history: &[],
        };
        let messages = build_messages(&req);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "current question");
    }

    #[test]
    fn build_messages_places_history_before_the_current_user_turn() {
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
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "first question");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "first answer");
        assert_eq!(messages[2].role, "user");
        assert_eq!(messages[2].content, "second question");
    }

    // ── RFC 0098: streaming SSE line parsing ────────────────────────────

    #[test]
    fn apply_stream_line_reads_model_and_input_tokens_from_message_start() {
        let mut acc = StreamAccumulator::default();
        let mut on_chunk = |_: String| {};
        apply_stream_line(
            &mut acc,
            r#"data: {"type":"message_start","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":42}}}"#,
            &mut on_chunk,
        );
        assert_eq!(acc.model, "claude-sonnet-4-6");
        assert_eq!(acc.input_tokens, 42);
    }

    #[test]
    fn apply_stream_line_accumulates_content_block_deltas_in_order() {
        let mut acc = StreamAccumulator::default();
        let mut chunks = Vec::new();
        let mut on_chunk = |s: String| chunks.push(s);

        apply_stream_line(
            &mut acc,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}"#,
            &mut on_chunk,
        );
        apply_stream_line(
            &mut acc,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}"#,
            &mut on_chunk,
        );

        assert_eq!(chunks, vec!["Hel".to_string(), "lo".to_string()]);
        assert_eq!(acc.content, "Hello");
    }

    #[test]
    fn apply_stream_line_reads_output_tokens_from_message_delta() {
        let mut acc = StreamAccumulator::default();
        let mut on_chunk = |_: String| {};
        apply_stream_line(
            &mut acc,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":15}}"#,
            &mut on_chunk,
        );
        assert_eq!(acc.output_tokens, 15);
    }

    #[test]
    fn apply_stream_line_ignores_non_data_and_unrecognized_event_lines() {
        let mut acc = StreamAccumulator::default();
        let mut on_chunk = |_: String| panic!("must not be called");
        apply_stream_line(&mut acc, "event: content_block_start", &mut on_chunk);
        apply_stream_line(
            &mut acc,
            r#"data: {"type":"content_block_stop","index":0}"#,
            &mut on_chunk,
        );
        apply_stream_line(&mut acc, "data: not valid json", &mut on_chunk);
        assert_eq!(acc.content, "");
        assert_eq!(acc.output_tokens, 0);
    }

    #[test]
    fn apply_stream_line_full_sequence_produces_the_expected_final_response() {
        let mut acc = StreamAccumulator::default();
        let mut chunks = Vec::new();
        let mut on_chunk = |s: String| chunks.push(s);

        for line in [
            r#"data: {"type":"message_start","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":100}}}"#,
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"!"}}"#,
            r#"data: {"type":"content_block_stop","index":0}"#,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}"#,
            r#"data: {"type":"message_stop"}"#,
        ] {
            apply_stream_line(&mut acc, line, &mut on_chunk);
        }

        assert_eq!(chunks, vec!["Hi".to_string(), "!".to_string()]);
        assert_eq!(acc.content, "Hi!");
        assert_eq!(acc.model, "claude-sonnet-4-6");
        assert_eq!(acc.input_tokens, 100);
        assert_eq!(acc.output_tokens, 2);
    }
}
