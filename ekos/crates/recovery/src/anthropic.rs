//! Anthropic Claude backend for `LlmProvider`.
//!
//! Reads the API key from the env var specified in `EkosConfig.llm.api_key_env`
//! (default: `ANTHROPIC_API_KEY`). Always sends `temperature: 0`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::llm::{LlmError, LlmProvider, LlmRequest, LlmResponse};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
const DEFAULT_KEY_ENV: &str = "ANTHROPIC_API_KEY";

pub struct AnthropicProvider {
    model: String,
    api_key: String,
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

    pub fn new(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        }
    }
}

// ── Wire types for the Anthropic Messages API ───────────────────────────────

#[derive(Serialize)]
struct ApiRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    temperature: f32,
    system: &'a str,
    messages: [ApiMessage<'a>; 1],
}

#[derive(Serialize)]
struct ApiMessage<'a> {
    role: &'a str,
    content: &'a str,
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
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    async fn complete(&self, req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        let body = ApiRequest {
            model: &self.model,
            max_tokens: req.max_tokens,
            temperature: 0.0,
            system: req.system,
            messages: [ApiMessage {
                role: "user",
                content: req.user,
            }],
        };

        let http_resp = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
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
        })
    }
}
