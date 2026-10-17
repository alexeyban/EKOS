//! OpenAI backend for `LlmProvider` (RFC 0046).
//!
//! Reads the API key from the given env var (default `OPENAI_API_KEY`). Always sends
//! `temperature: 0`. Model: `[llm] model` > `OPENAI_MODEL` > `gpt-4o-mini`.
//!
//! RFC 0145: works against any OpenAI-compatible Chat Completions host — OpenCode Zen, OpenRouter,
//! DeepSeek, Groq, vLLM — via `[llm] base-url` > `OPENAI_BASE_URL` > `https://api.openai.com/v1`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::llm::{LlmError, LlmProvider, LlmRequest, LlmResponse, stream_lines};

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_MODEL: &str = "gpt-4o-mini";
const DEFAULT_KEY_ENV: &str = "OPENAI_API_KEY";

pub struct OpenAiProvider {
    model: String,
    api_key: String,
    /// RFC 0197: sent as `reasoning_effort` when set.
    reasoning_effort: Option<String>,
    /// RFC 0197: set once the model refused `temperature`.
    no_temperature: std::sync::atomic::AtomicBool,
    /// RFC 0145: without a trailing `/`; requests go to `{base_url}/chat/completions`.
    base_url: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
    /// Create from environment. Returns `Err(LlmError::NoApiKey)` if the env var is absent.
    pub fn from_env() -> Result<Self, LlmError> {
        Self::from_env_var(DEFAULT_KEY_ENV)
    }

    pub fn from_env_var(env_var: &str) -> Result<Self, LlmError> {
        Self::from_config(env_var, None, None)
    }

    /// RFC 0145: `model_override`/`base_url_override` are `[llm] model`/`[llm] base-url`, each
    /// winning over its env var (`OPENAI_MODEL`/`OPENAI_BASE_URL`), which wins over the default.
    pub fn from_config(
        env_var: &str,
        model_override: Option<&str>,
        base_url_override: Option<&str>,
    ) -> Result<Self, LlmError> {
        let api_key =
            std::env::var(env_var).map_err(|_| LlmError::NoApiKey(env_var.to_string()))?;
        let model = model_override
            .map(str::to_string)
            .or_else(|| std::env::var("OPENAI_MODEL").ok())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let base_url = base_url_override
            .map(str::to_string)
            .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Ok(Self::new(model, api_key).with_base_url(base_url))
    }

    pub fn new(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            api_key: api_key.into(),
            reasoning_effort: None,
            no_temperature: std::sync::atomic::AtomicBool::new(false),
            base_url: DEFAULT_BASE_URL.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// RFC 0197: `minimal`/`low`/`medium`/`high`, for hosts and models that take it.
    pub fn with_reasoning_effort(mut self, effort: Option<String>) -> Self {
        self.reasoning_effort = effort;
        self
    }

    fn body<'a>(&'a self, req: &LlmRequest<'a>, stream: bool) -> ApiRequest<'a> {
        let no_temp = self
            .no_temperature
            .load(std::sync::atomic::Ordering::Relaxed);
        ApiRequest {
            model: &self.model,
            max_tokens: req.max_tokens,
            temperature: (!no_temp).then_some(0.0),
            reasoning_effort: self.reasoning_effort.as_deref(),
            messages: build_messages(req),
            stream,
            stream_options: stream.then_some(StreamOptions {
                include_usage: true,
            }),
        }
    }

    /// POST, retrying once without `temperature` when the model refuses it (RFC 0197).
    async fn send(
        &self,
        req: &LlmRequest<'_>,
        stream: bool,
    ) -> Result<reqwest::Response, LlmError> {
        loop {
            let http_resp = self
                .client
                .post(self.endpoint())
                .bearer_auth(&self.api_key)
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
            if status == 400 && had_temp && crate::anthropic::refuses_temperature(&body) {
                self.no_temperature
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                continue;
            }
            return Err(LlmError::Api { status, body });
        }
    }

    /// RFC 0145: point at any OpenAI-compatible host. A trailing `/` is tolerated.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

// ── Wire types for the OpenAI Chat Completions API ──────────────────────────

#[derive(Serialize)]
struct ApiRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'a str>,
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
    usage: serde_json::Value,
}

#[derive(Deserialize)]
struct ApiChoice {
    message: ApiResponseMessage,
}

#[derive(Deserialize)]
struct ApiResponseMessage {
    content: Option<String>,
}

/// `(input billed at the full price, output, cache read)` from a Chat Completions `usage`
/// object. Hosts report cached prompt tokens as `prompt_tokens_details.cached_tokens` (OpenAI)
/// or a top-level `cache_read_input_tokens` (DeepSeek on Zen); either way they are part of
/// `prompt_tokens`, so they are taken out of the input count (RFC 0197).
pub(crate) fn chat_usage(usage: &serde_json::Value) -> (u32, u32, u32) {
    let n =
        |v: Option<&serde_json::Value>| v.and_then(serde_json::Value::as_u64).unwrap_or(0) as u32;
    let prompt = n(usage.get("prompt_tokens"));
    let output = n(usage.get("completion_tokens"));
    let cached = n(usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens")))
    .max(n(usage.get("cache_read_input_tokens")));
    (prompt.saturating_sub(cached), output, cached.min(prompt))
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    /// RFC 0145: only a non-default host changes the key, so existing OpenAI cache entries stay valid.
    fn cache_namespace(&self) -> Option<String> {
        (self.base_url != DEFAULT_BASE_URL).then(|| self.base_url.clone())
    }

    async fn complete(&self, req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        let http_resp = self.send(req, false).await?;

        let api_resp: ApiResponse = http_resp.json().await?;
        let (input_tokens, output_tokens, cache_read_tokens) = chat_usage(&api_resp.usage);
        let content = api_resp
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .unwrap_or_default();

        Ok(LlmResponse {
            content,
            model: api_resp.model,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            ..Default::default()
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
            ..Default::default()
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
    if let Some(usage) = event.get("usage").filter(|u| u.is_object()) {
        let (i, o, c) = chat_usage(usage);
        acc.input_tokens = i;
        acc.output_tokens = o;
        acc.cache_read_tokens = c;
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

    // ── RFC 0145: OpenAI-compatible endpoints ────────────────────────────

    #[test]
    fn base_url_builds_the_endpoint_and_only_a_custom_host_namespaces_the_cache() {
        let default = OpenAiProvider::new("m", "k");
        assert_eq!(
            default.endpoint(),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(default.cache_namespace(), None);

        let zen = OpenAiProvider::new("deepseek-v4-flash", "k")
            .with_base_url("https://opencode.ai/zen/v1/");
        assert_eq!(
            zen.endpoint(),
            "https://opencode.ai/zen/v1/chat/completions"
        );
        assert_eq!(
            zen.cache_namespace().as_deref(),
            Some("https://opencode.ai/zen/v1")
        );
    }

    #[test]
    fn from_config_precedence() {
        // One test function: OPENAI_MODEL / OPENAI_BASE_URL / the key var are process-global.
        // SAFETY: test-local env mutation; these var names are used by no other test.
        unsafe {
            std::env::set_var("EKOS_TEST_RFC0145_KEY", "k");
            std::env::set_var("OPENAI_MODEL", "env-model");
            std::env::set_var("OPENAI_BASE_URL", "https://env.example/v1");
        }
        let from_env = OpenAiProvider::from_config("EKOS_TEST_RFC0145_KEY", None, None).unwrap();
        assert_eq!(from_env.model_name(), "env-model");
        assert_eq!(from_env.base_url, "https://env.example/v1");

        let configured = OpenAiProvider::from_config(
            "EKOS_TEST_RFC0145_KEY",
            Some("cfg-model"),
            Some("https://cfg.example/v1"),
        )
        .unwrap();
        assert_eq!(configured.model_name(), "cfg-model");
        assert_eq!(configured.base_url, "https://cfg.example/v1");

        unsafe {
            std::env::remove_var("OPENAI_MODEL");
            std::env::remove_var("OPENAI_BASE_URL");
            std::env::remove_var("EKOS_TEST_RFC0145_KEY");
        }
        assert!(matches!(
            OpenAiProvider::from_config("EKOS_TEST_RFC0145_KEY", None, None),
            Err(LlmError::NoApiKey(_))
        ));
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
    fn request_body_sets_temperature_zero_until_the_model_refuses_it() {
        let req = LlmRequest {
            system: "sys",
            user: "usr",
            prompt_version: "v1",
            max_tokens: 100,
            history: &[],
        };
        let p = OpenAiProvider::new("gpt-4o-mini", "k");
        let json = serde_json::to_value(p.body(&req, false)).unwrap();
        assert_eq!(json["temperature"], 0.0);
        assert!(json.get("reasoning_effort").is_none());
        assert!(json.get("stream_options").is_none());
        assert_eq!(json["messages"][0]["role"], "system");
        assert_eq!(json["messages"][1]["role"], "user");
        // RFC 0197: after a refusal the request goes without, and an effort is passed through.
        let p = p.with_reasoning_effort(Some("low".into()));
        p.no_temperature
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let json = serde_json::to_value(p.body(&req, true)).unwrap();
        assert!(json.get("temperature").is_none());
        assert_eq!(json["reasoning_effort"], "low");
        assert_eq!(json["stream_options"]["include_usage"], true);
    }

    #[test]
    fn cached_prompt_tokens_are_read_from_either_usage_shape() {
        // OpenAI's shape.
        let openai = serde_json::json!({"prompt_tokens": 100, "completion_tokens": 5,
            "prompt_tokens_details": {"cached_tokens": 64}});
        assert_eq!(chat_usage(&openai), (36, 5, 64));
        // DeepSeek on Zen (recorded 2026-10-17, RFC 0197).
        let deepseek = serde_json::json!({"prompt_tokens": 33, "completion_tokens": 25,
            "total_tokens": 58, "cache_read_input_tokens": 0});
        assert_eq!(chat_usage(&deepseek), (33, 25, 0));
        assert_eq!(chat_usage(&serde_json::json!({})), (0, 0, 0));
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
