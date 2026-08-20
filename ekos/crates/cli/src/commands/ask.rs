use super::recover::build_llm_provider;
use super::store::open_store;
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_runtime::{AiRuntime, AiRuntimeConfig, Runtime};
use std::path::Path;

pub async fn run(config: &EkosConfig, cwd: &Path, question: &str, json: bool) -> Result<()> {
    let ai_config = ai_config(config);
    let artifact_dir = config.artifact_dir(cwd);
    let llm = build_llm_provider(config, &artifact_dir);

    let ledger = open_store(config, cwd)?;
    let runtime = Runtime::over(&*ledger);
    let ai = AiRuntime::new(&runtime, llm, ai_config);

    let answer = ai.ask(question).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&answer)?);
        return Ok(());
    }

    println!("{}", answer.answer);

    if !answer.evidence_refs.is_empty() {
        println!("\nSources:");
        for id in &answer.evidence_refs {
            if let Some(ev) = ledger.get_evidence(id)? {
                println!(
                    "  [{:.0}%] {} — \"{}\"",
                    ev.confidence * 100.0,
                    ev.location.path,
                    ev.fragment
                );
            }
        }
    }

    for diag in &answer.diagnostics {
        eprintln!("warning: {}", diag.message);
    }

    Ok(())
}

/// Shared with `docs.rs`'s `--prose` tier (RFC 0035 Phase 5) and RFC 0045's demo server —
/// same `[ai]` config resolution, so prose generation and the hosted `/ask` endpoint both
/// honor the same model/provider settings `ekos ask` already does.
pub fn ai_config(config: &EkosConfig) -> AiRuntimeConfig {
    let default = AiRuntimeConfig::default();
    AiRuntimeConfig {
        model: config.ai.model.clone().unwrap_or(default.model),
        max_matches: config.ai.max_matches.unwrap_or(default.max_matches),
        neighborhood_depth: config
            .ai
            .neighborhood_depth
            .unwrap_or(default.neighborhood_depth),
        max_tokens: config.ai.max_tokens.unwrap_or(default.max_tokens),
        system_prompt: config
            .ai
            .system_prompt
            .clone()
            .unwrap_or(default.system_prompt),
        max_context_chars: config
            .ai
            .max_context_chars
            .unwrap_or(default.max_context_chars),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_compiler_core::config::LlmConfig;
    use tempfile::tempdir;

    /// `ekos ask` must honor `config.llm.provider` the same way `ekos recover`
    /// does — it calls the exact same `build_llm_provider` rather than
    /// constructing `AnthropicProvider` itself, so this proves the shared
    /// selection logic is reachable from `ask.rs`, not duplicated/diverged.
    #[test]
    fn ask_selects_ollama_provider_when_configured() {
        let dir = tempdir().unwrap();
        let mut config = EkosConfig::default();
        config.llm = LlmConfig {
            provider: Some("ollama".to_string()),
            api_key_env: None,
            model: None,
        };
        let artifact_dir = config.artifact_dir(dir.path());
        let provider = build_llm_provider(&config, &artifact_dir);
        assert_eq!(provider.model_name(), "llama3.1:8b");
    }
}
