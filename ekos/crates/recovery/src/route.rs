//! LLM task routing (RFC 0197 §1–2): the one place a provider is built from `[llm]` and
//! `[llm.tasks.<task>]`.
//!
//! Every provider it returns is `Fallback(Cached(Metered(model)))` for each model of the route:
//! disk-cache hits cost nothing and are not logged; every call that reaches a host is checked
//! against the data policy and the budget, then logged to `llm-usage.jsonl`.

use crate::anthropic::AnthropicProvider;
use crate::cache::CachedLlmProvider;
use crate::llm::{LlmError, LlmProvider, Task};
use crate::ollama::OllamaProvider;
use crate::openai::OpenAiProvider;
use crate::openai_responses::OpenAiResponsesProvider;
use crate::usage::{Budget, FallbackLlmProvider, Meter, MeteredLlmProvider};
use ekos_compiler_core::config::LlmConfig;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// OpenCode Zen's base URL (RFC 0145, 0197).
pub const ZEN_BASE_URL: &str = "https://opencode.ai/zen/v1";

/// The wire format a model speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    Messages,
    Responses,
    ChatCompletions,
    Ollama,
}

impl Wire {
    pub fn as_str(self) -> &'static str {
        match self {
            Wire::Messages => "messages",
            Wire::Responses => "responses",
            Wire::ChatCompletions => "chat-completions",
            Wire::Ollama => "ollama",
        }
    }
}

/// Which format OpenCode Zen serves `model` on (opencode.ai/docs/zen, read 2026-10-17).
pub fn zen_wire(model: &str) -> Wire {
    let qwen_messages = ["qwen3.5-", "qwen3.6-", "qwen3.7-", "qwen3.8-flash"]
        .iter()
        .any(|p| model.starts_with(p))
        && !model.ends_with("-max");
    if model.starts_with("claude-") || qwen_messages {
        Wire::Messages
    } else if ["gpt-", "grok-", "muse-"]
        .iter()
        .any(|p| model.starts_with(p))
    {
        Wire::Responses
    } else {
        Wire::ChatCompletions
    }
}

/// The key variable a provider uses when `api-key-env` is not set.
pub fn default_key_env(provider: Option<&str>) -> &'static str {
    match provider {
        Some("openai") => "OPENAI_API_KEY",
        Some("zen") => "OPENCODE_API_KEY",
        _ => "ANTHROPIC_API_KEY",
    }
}

/// One task's resolved route, for display (`ekos llm routes`, `doctor`).
#[derive(Debug, Clone, PartialEq)]
pub struct RouteInfo {
    pub task: Task,
    pub provider: String,
    pub models: Vec<String>,
    pub wire: Wire,
    pub key_env: Option<String>,
}

/// Builds each task's provider; one per command so every task shares its budget.
pub struct LlmRouter {
    llm: LlmConfig,
    cache_dir: Option<PathBuf>,
    usage_log: Option<PathBuf>,
    budget: Arc<Budget>,
}

impl LlmRouter {
    /// `state_dir` holds `llm-cache/` and `llm-usage.jsonl` (the workspace's `.ekos`).
    pub fn new(llm: &LlmConfig, state_dir: &Path) -> Self {
        let cache_dir = state_dir.join("llm-cache");
        std::fs::create_dir_all(&cache_dir).ok();
        Self {
            llm: llm.clone(),
            cache_dir: Some(cache_dir),
            usage_log: Some(state_dir.join("llm-usage.jsonl")),
            budget: Budget::new(llm.budget_usd),
        }
    }

    /// Without the disk cache (`ekos llm bench` and `probe` must reach the host).
    pub fn uncached(mut self) -> Self {
        self.cache_dir = None;
        self
    }

    /// A budget other than `[llm] budget-usd`.
    pub fn with_budget(mut self, budget: Arc<Budget>) -> Self {
        self.budget = budget;
        self
    }

    pub fn budget(&self) -> &Arc<Budget> {
        &self.budget
    }

    pub fn usage_log(&self) -> Option<&Path> {
        self.usage_log.as_deref()
    }

    /// `[llm]` with `[llm.tasks.<task>]` over it.
    pub fn config_for(&self, task: Task) -> LlmConfig {
        self.llm.for_task(task.as_str())
    }

    pub fn route(&self, task: Task) -> RouteInfo {
        let c = self.config_for(task);
        let provider = c.provider.clone().unwrap_or_else(|| "anthropic".into());
        let model = c
            .model
            .clone()
            .unwrap_or_else(|| default_model(&provider).into());
        let mut models = vec![model.clone()];
        models.extend(c.fallback.iter().cloned());
        RouteInfo {
            task,
            wire: wire_for(&provider, &model),
            key_env: (provider != "ollama").then(|| key_env(&c).to_string()),
            provider,
            models,
        }
    }

    /// The provider for `task`.
    pub fn provider(&self, task: Task) -> Result<Arc<dyn LlmProvider>, LlmError> {
        self.provider_for_models(task, None)
    }

    /// `task`'s route with its model list replaced (`ekos llm bench --models`).
    pub fn provider_for_models(
        &self,
        task: Task,
        models: Option<&[String]>,
    ) -> Result<Arc<dyn LlmProvider>, LlmError> {
        let c = self.config_for(task);
        let route = self.route(task);
        let models = models.map(<[String]>::to_vec).unwrap_or(route.models);
        let mut chain: Vec<Arc<dyn LlmProvider>> = Vec::with_capacity(models.len());
        for model in &models {
            chain.push(self.one(task, &c, &route.provider, model)?);
        }
        Ok(if chain.len() == 1 {
            chain.pop().expect("one")
        } else {
            Arc::new(FallbackLlmProvider::new(chain))
        })
    }

    fn one(
        &self,
        task: Task,
        c: &LlmConfig,
        provider: &str,
        model: &str,
    ) -> Result<Arc<dyn LlmProvider>, LlmError> {
        let meter = Meter {
            task,
            provider: provider.to_string(),
            metered: provider != "ollama",
            prices: c.prices.clone(),
            allow_data_collection: c.allow_data_collection,
            budget: self.budget.clone(),
            log: self.usage_log.clone(),
        };
        let key = key_env(c);
        let effort = c.reasoning_effort.clone();
        let cache = c.prompt_cache.unwrap_or(false);
        macro_rules! wrap {
            ($p:expr) => {{
                let metered = MeteredLlmProvider::new($p, meter);
                match &self.cache_dir {
                    Some(dir) => Arc::new(CachedLlmProvider::new(metered, dir.clone()))
                        as Arc<dyn LlmProvider>,
                    None => Arc::new(metered) as Arc<dyn LlmProvider>,
                }
            }};
        }
        Ok(match provider {
            "ollama" => wrap!(
                OllamaProvider::from_env_with_model(Some(model))
                    .with_context_window(c.context_window)
            ),
            "openai" => wrap!(
                OpenAiProvider::from_config(key, Some(model), c.base_url.as_deref())?
                    .with_reasoning_effort(effort)
            ),
            "zen" => {
                let base = c.base_url.as_deref().unwrap_or(ZEN_BASE_URL);
                match zen_wire(model) {
                    Wire::Messages => wrap!(
                        AnthropicProvider::from_config(key, Some(model), Some(base))?
                            .with_prompt_cache(cache)
                    ),
                    Wire::Responses => wrap!(
                        OpenAiResponsesProvider::from_config(key, model, base)?
                            .with_reasoning_effort(effort)
                    ),
                    _ => wrap!(
                        OpenAiProvider::from_config(key, Some(model), Some(base))?
                            .with_reasoning_effort(effort)
                    ),
                }
            }
            _ => wrap!(
                AnthropicProvider::from_config(key, Some(model), c.base_url.as_deref())?
                    .with_prompt_cache(cache)
            ),
        })
    }
}

fn key_env(c: &LlmConfig) -> &str {
    c.api_key_env
        .as_deref()
        .unwrap_or_else(|| default_key_env(c.provider.as_deref()))
}

fn default_model(provider: &str) -> &'static str {
    match provider {
        "ollama" => "llama3.1:8b",
        "openai" => "gpt-4o-mini",
        "zen" => "claude-haiku-5-5",
        _ => "claude-sonnet-4-6",
    }
}

fn wire_for(provider: &str, model: &str) -> Wire {
    match provider {
        "ollama" => Wire::Ollama,
        "openai" => Wire::ChatCompletions,
        "zen" => zen_wire(model),
        _ => Wire::Messages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_compiler_core::config::LlmRoute;

    #[test]
    fn zen_models_map_to_their_wire_formats() {
        assert_eq!(zen_wire("claude-haiku-5-5"), Wire::Messages);
        assert_eq!(zen_wire("qwen3.8-flash"), Wire::Messages);
        assert_eq!(zen_wire("qwen3.8-max"), Wire::ChatCompletions);
        assert_eq!(zen_wire("gpt-6-luna"), Wire::Responses);
        assert_eq!(zen_wire("grok-build-0.1"), Wire::Responses);
        assert_eq!(zen_wire("deepseek-v4.1-flash"), Wire::ChatCompletions);
        assert_eq!(zen_wire("glm-5.3-flash"), Wire::ChatCompletions);
    }

    fn config() -> LlmConfig {
        let mut c = LlmConfig {
            provider: Some("zen".into()),
            model: Some("deepseek-v4.1-flash".into()),
            ..Default::default()
        };
        c.tasks.insert(
            "doc-semantics".into(),
            LlmRoute {
                model: Some("qwen3.8-flash".into()),
                fallback: Some(vec!["glm-5.3-flash".into()]),
                ..Default::default()
            },
        );
        c.tasks.insert(
            "ask".into(),
            LlmRoute {
                provider: Some("ollama".into()),
                model: Some("llama3".into()),
                ..Default::default()
            },
        );
        c
    }

    #[test]
    fn a_task_route_overrides_the_default_and_inherits_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let r = LlmRouter::new(&config(), dir.path());
        let d = r.route(Task::DocSemantics);
        assert_eq!(d.provider, "zen");
        assert_eq!(d.models, vec!["qwen3.8-flash", "glm-5.3-flash"]);
        assert_eq!(d.wire, Wire::Messages);
        assert_eq!(d.key_env.as_deref(), Some("OPENCODE_API_KEY"));
        let other = r.route(Task::Description);
        assert_eq!(other.models, vec!["deepseek-v4.1-flash"]);
        assert_eq!(other.wire, Wire::ChatCompletions);
        let ask = r.route(Task::Ask);
        assert_eq!(
            (ask.provider.as_str(), ask.wire, ask.key_env),
            ("ollama", Wire::Ollama, None)
        );
    }

    #[test]
    fn a_route_with_another_provider_does_not_inherit_the_default_host() {
        let mut c = config();
        c.base_url = Some("https://example.test/v1".into());
        c.tasks.get_mut("ask").unwrap().provider = Some("openai".into());
        let t = c.for_task("ask");
        assert_eq!(t.base_url, None);
        assert_eq!(
            c.for_task("description").base_url.as_deref(),
            Some("https://example.test/v1")
        );
    }

    #[test]
    fn every_task_name_round_trips() {
        for t in Task::ALL {
            assert_eq!(Task::parse(t.as_str()), Some(t));
        }
    }

    #[test]
    fn a_missing_key_is_an_error_naming_the_variable() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = config();
        c.api_key_env = Some("EKOS_TEST_KEY_THAT_IS_NEVER_SET".into());
        let err = LlmRouter::new(&c, dir.path())
            .provider(Task::Description)
            .err()
            .unwrap();
        assert!(
            err.to_string().contains("EKOS_TEST_KEY_THAT_IS_NEVER_SET"),
            "{err}"
        );
    }
}
