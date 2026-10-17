//! OpenAI Responses API backend for `LlmProvider` (RFC 0197).
//!
//! OpenCode Zen serves GPT, Grok and Muse models only on `POST {base}/responses`. The answer is
//! the `output_text` of the `message` output item (a `reasoning` item comes first on reasoning
//! models); `max_output_tokens` covers the reasoning too. Streaming is not implemented: the
//! trait's default reports the whole answer as one chunk.

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

use crate::llm::{LlmError, LlmProvider, LlmRequest, LlmResponse};

pub struct OpenAiResponsesProvider {
    model: String,
    api_key: String,
    /// Without a trailing `/`.
    base_url: String,
    reasoning_effort: Option<String>,
    no_temperature: std::sync::atomic::AtomicBool,
    client: reqwest::Client,
}

impl OpenAiResponsesProvider {
    pub fn from_config(env_var: &str, model: &str, base_url: &str) -> Result<Self, LlmError> {
        let api_key =
            std::env::var(env_var).map_err(|_| LlmError::NoApiKey(env_var.to_string()))?;
        Ok(Self::new(model, api_key, base_url))
    }

    pub fn new(model: impl Into<String>, api_key: impl Into<String>, base_url: &str) -> Self {
        Self {
            model: model.into(),
            api_key: api_key.into(),
            base_url: base_url.trim_end_matches('/').to_string(),
            reasoning_effort: None,
            no_temperature: std::sync::atomic::AtomicBool::new(false),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_reasoning_effort(mut self, effort: Option<String>) -> Self {
        self.reasoning_effort = effort;
        self
    }

    fn body<'a>(&'a self, req: &LlmRequest<'a>) -> ApiRequest<'a> {
        let mut input: Vec<ApiMessage<'a>> = req
            .history
            .iter()
            .map(|m| ApiMessage {
                role: m.role,
                content: m.content,
            })
            .collect();
        input.push(ApiMessage {
            role: "user",
            content: req.user,
        });
        let no_temp = self
            .no_temperature
            .load(std::sync::atomic::Ordering::Relaxed);
        ApiRequest {
            model: &self.model,
            instructions: req.system,
            input,
            max_output_tokens: req.max_tokens,
            temperature: (!no_temp).then_some(0.0),
            reasoning: self
                .reasoning_effort
                .as_deref()
                .map(|effort| Reasoning { effort }),
        }
    }
}

#[derive(Serialize)]
struct ApiRequest<'a> {
    model: &'a str,
    instructions: &'a str,
    input: Vec<ApiMessage<'a>>,
    max_output_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<Reasoning<'a>>,
}

#[derive(Serialize)]
struct ApiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct Reasoning<'a> {
    effort: &'a str,
}

/// The answer and usage of a Responses API reply.
pub(crate) fn parse_response(v: &Value, requested_model: &str) -> LlmResponse {
    let content = v["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|o| o["type"] == "message")
        .flat_map(|o| o["content"].as_array().cloned().unwrap_or_default())
        .filter(|c| c["type"] == "output_text")
        .filter_map(|c| c["text"].as_str().map(str::to_string))
        .collect::<Vec<_>>()
        .join("");
    let n = |x: &Value| x.as_u64().unwrap_or(0) as u32;
    let u = &v["usage"];
    let input = n(&u["input_tokens"]);
    let cached = n(&u["input_tokens_details"]["cached_tokens"]).min(input);
    let written = n(&u["input_tokens_details"]["cache_write_tokens"]).min(input - cached);
    LlmResponse {
        content,
        model: v["model"].as_str().unwrap_or(requested_model).to_string(),
        input_tokens: input - cached - written,
        output_tokens: n(&u["output_tokens"]),
        cache_read_tokens: cached,
        cache_write_tokens: written,
    }
}

#[async_trait]
impl LlmProvider for OpenAiResponsesProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    fn cache_namespace(&self) -> Option<String> {
        Some(format!("responses:{}", self.base_url))
    }

    async fn complete(&self, req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        let mut dropped_temperature = false;
        loop {
            let http_resp = self
                .client
                .post(format!("{}/responses", self.base_url))
                .bearer_auth(&self.api_key)
                .json(&self.body(req))
                .send()
                .await?;
            let status = http_resp.status().as_u16();
            if http_resp.status().is_success() {
                let v: Value = http_resp.json().await?;
                return Ok(parse_response(&v, &self.model));
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded from `gpt-6-luna` on OpenCode Zen, 2026-10-17 (ids and the encrypted reasoning
    /// shortened).
    const RECORDED: &str = r#"{"id":"resp_x","object":"response","status":"completed",
      "model":"gpt-6-luna","output":[
        {"id":"rs_x","type":"reasoning","encrypted_content":"gAAA"},
        {"type":"message","status":"completed","role":"assistant","phase":"final_answer",
         "content":[{"type":"output_text","text":"Paris","annotations":[],"logprobs":[]}]}],
      "usage":{"input_tokens":19,"output_tokens":18,"total_tokens":37,
        "input_tokens_details":{"cached_tokens":0,"cache_write_tokens":0},
        "output_tokens_details":{"reasoning_tokens":11}}}"#;

    #[test]
    fn reads_the_message_text_after_the_reasoning_item() {
        let r = parse_response(&serde_json::from_str(RECORDED).unwrap(), "gpt-6-luna");
        assert_eq!(r.content, "Paris");
        assert_eq!((r.input_tokens, r.output_tokens), (19, 18));
        assert_eq!((r.cache_read_tokens, r.cache_write_tokens), (0, 0));
    }

    #[test]
    fn cached_tokens_come_out_of_the_input_count() {
        let v = serde_json::json!({"model":"gpt-6-luna","output":[],
            "usage":{"input_tokens":100,"output_tokens":1,
                     "input_tokens_details":{"cached_tokens":60,"cache_write_tokens":30}}});
        let r = parse_response(&v, "gpt-6-luna");
        assert_eq!(
            (r.input_tokens, r.cache_read_tokens, r.cache_write_tokens),
            (10, 60, 30)
        );
    }

    #[test]
    fn the_request_names_instructions_input_and_effort() {
        let p = OpenAiResponsesProvider::new("gpt-6-luna", "k", "https://opencode.ai/zen/v1/")
            .with_reasoning_effort(Some("low".into()));
        let req = LlmRequest {
            system: "sys",
            user: "hi",
            prompt_version: "v",
            max_tokens: 64,
            history: &[],
        };
        let v = serde_json::to_value(p.body(&req)).unwrap();
        assert_eq!(v["instructions"], "sys");
        assert_eq!(v["input"][0]["content"], "hi");
        assert_eq!(v["max_output_tokens"], 64);
        assert_eq!(v["reasoning"]["effort"], "low");
        assert_eq!(p.base_url, "https://opencode.ai/zen/v1");
    }
}
