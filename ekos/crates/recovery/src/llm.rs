//! LLM provider trait and types (RFC 0008).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

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
