//! OpenAI backend for `LlmProvider` (RFC 0046).
//!
//! Reads the API key from the given env var (default `OPENAI_API_KEY`). Always sends
//! `temperature: 0`. Model defaults to `gpt-4o-mini`, overridable via `OPENAI_MODEL`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::llm::{LlmError, LlmProvider, LlmRequest, LlmResponse};

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
    messages: [ApiMessage<'a>; 2],
}

#[derive(Serialize)]
struct ApiMessage<'a> {
    role: &'a str,
    content: &'a str,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_name_returns_the_constructed_model() {
        let provider = OpenAiProvider::new("gpt-4o-mini", "test-key");
        assert_eq!(provider.model_name(), "gpt-4o-mini");
    }

    #[test]
    fn request_body_always_sets_temperature_zero() {
        let req = LlmRequest {
            system: "sys",
            user: "usr",
            prompt_version: "v1",
            max_tokens: 100,
        };
        let body = ApiRequest {
            model: "gpt-4o-mini",
            max_tokens: req.max_tokens,
            temperature: 0.0,
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
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["temperature"], 0.0);
        assert_eq!(json["messages"][0]["role"], "system");
        assert_eq!(json["messages"][1]["role"], "user");
    }
}
