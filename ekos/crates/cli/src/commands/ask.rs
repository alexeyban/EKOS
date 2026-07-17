use super::store::open_store;
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_recovery::{anthropic::AnthropicProvider, llm::LlmProvider};
use ekos_runtime::{AiRuntime, AiRuntimeConfig, Runtime};
use std::{path::Path, sync::Arc};

pub async fn run(config: &EkosConfig, cwd: &Path, question: &str, json: bool) -> Result<()> {
    let key_env = config
        .llm
        .api_key_env
        .as_deref()
        .unwrap_or("ANTHROPIC_API_KEY");
    let ai_config = ai_config(config);

    let llm: Arc<dyn LlmProvider> = match std::env::var(key_env) {
        Ok(api_key) => Arc::new(AnthropicProvider::new(ai_config.model.clone(), api_key)),
        Err(_) => {
            eprintln!(
                "No LLM provider configured. Set {key_env} and provider = 'claude' in ekos.toml."
            );
            std::process::exit(1);
        }
    };

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

fn ai_config(config: &EkosConfig) -> AiRuntimeConfig {
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
    }
}
