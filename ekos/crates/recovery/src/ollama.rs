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

use crate::llm::{LlmError, LlmProvider, LlmRequest, LlmResponse};

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
        ApiRequest {
            model: &self.model,
            stream: false,
            messages: [
                ApiMessage {
                    role: "system",
                    content: req.system,
                },
                ApiMessage {
                    role: "user",
                    content: req.user,
                },
            ],
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
    messages: [ApiMessage<'a>; 2],
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_name_reflects_construction() {
        let provider = OllamaProvider::new("llama3.1:8b", "http://localhost:11434");
        assert_eq!(provider.model_name(), "llama3.1:8b");
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
        };
        let body = provider.build_request(&req);
        assert_eq!(body.options.temperature, 0.0);
        assert_eq!(body.options.num_predict, 123);
        assert!(!body.stream);
        assert_eq!(body.messages[0].role, "system");
        assert_eq!(body.messages[1].role, "user");
    }
}
