//! `ekos eval run` (RFC 0138) — runs an `ekos-evals` scenario dataset against a real,
//! already-built workspace and prints the `EKOS EVALUATION` report. Owns opening the store and
//! building the `LlmProvider`, exactly like `ask.rs` — `ekos-evals` itself never touches
//! configuration or credentials.

use super::recover::build_llm_provider;
use super::store::open_store_read_only;
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_evals::report::{self, GateThresholds};
use ekos_evals::schema::load_dataset;
use ekos_runtime::{AiRuntime, Runtime};
use std::path::{Path, PathBuf};

pub struct EvalRunOpts<'a> {
    pub dataset: Option<&'a str>,
    pub datasets_dir: Option<PathBuf>,
    pub category: Option<&'a str>,
    /// Overrides `config.llm.provider` for this run only — `claude`/`anthropic` selects the
    /// default branch of `build_llm_provider`, `ollama`/`openai` select those explicitly.
    pub agent: Option<&'a str>,
    pub limit: Option<usize>,
    pub json: bool,
    pub output: Option<PathBuf>,
}

fn agent_label(config: &EkosConfig) -> String {
    match config.llm.provider.as_deref() {
        Some("ollama") => format!(
            "ollama ({})",
            config.llm.model.as_deref().unwrap_or("default")
        ),
        Some("openai") => "openai".to_string(),
        _ => "claude".to_string(),
    }
}

pub async fn run(config: &EkosConfig, cwd: &Path, opts: EvalRunOpts<'_>) -> Result<()> {
    let datasets_dir = opts
        .datasets_dir
        .clone()
        .unwrap_or_else(|| cwd.join("evals").join("datasets"));
    let (dataset_name, mut scenarios) = load_dataset(opts.dataset, &datasets_dir)
        .map_err(|e| anyhow::anyhow!("loading dataset from {}: {e}", datasets_dir.display()))?;

    if let Some(category) = opts.category {
        scenarios.retain(|s| s.category == category);
    }
    if let Some(limit) = opts.limit {
        scenarios.truncate(limit);
    }
    if scenarios.is_empty() {
        anyhow::bail!(
            "no scenarios matched (dataset {dataset_name:?}, category {:?})",
            opts.category
        );
    }

    let mut run_config = config.clone();
    match opts.agent {
        Some("claude") | Some("anthropic") => run_config.llm.provider = None,
        Some("ollama") => run_config.llm.provider = Some("ollama".to_string()),
        Some("openai") => run_config.llm.provider = Some("openai".to_string()),
        Some(other) => anyhow::bail!("unknown --agent {other:?} (want claude/ollama/openai)"),
        None => {}
    }

    let artifact_dir = run_config.artifact_dir(cwd);
    let llm = build_llm_provider(&run_config, &artifact_dir);
    let ledger = open_store_read_only(&run_config, cwd)?;
    let runtime = Runtime::over(&*ledger);
    let ai = AiRuntime::new(&runtime, llm, super::ask::ai_config(&run_config));

    let outcomes = ekos_evals::run_all(&ai, &runtime, &*ledger, &scenarios).await;
    let report = report::build(
        &dataset_name,
        &agent_label(&run_config),
        "local",
        &outcomes,
        GateThresholds::default(),
    );

    if let Some(output) = &opts.output {
        std::fs::write(output, serde_json::to_string_pretty(&report)?)?;
    } else {
        let default_dir = cwd.join("evals").join("reports");
        if default_dir.is_dir() {
            let filename = format!(
                "{}-{}.json",
                report.generated_at.format("%Y%m%dT%H%M%SZ"),
                dataset_name
            );
            std::fs::write(
                default_dir.join(filename),
                serde_json::to_string_pretty(&report)?,
            )
            .ok();
        }
    }

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("{}", report::render_text(&report));
    }

    if !report.metrics.status_pass {
        std::process::exit(1);
    }
    Ok(())
}

pub struct EvalHistoryOpts {
    pub reports_dir: Option<PathBuf>,
    pub limit: Option<usize>,
    pub json: bool,
}

pub fn history(cwd: &Path, opts: EvalHistoryOpts) -> Result<()> {
    let reports_dir = opts
        .reports_dir
        .unwrap_or_else(|| cwd.join("evals").join("reports"));
    let mut runs = ekos_evals::history::load_all(&reports_dir)
        .map_err(|e| anyhow::anyhow!("reading run history from {}: {e}", reports_dir.display()))?;
    if let Some(limit) = opts.limit {
        // Newest last (oldest-first ordering) — a "last N" limit means keep the tail.
        let start = runs.len().saturating_sub(limit);
        runs.drain(..start);
    }

    if opts.json {
        let reports: Vec<_> = runs.iter().map(|(_, r)| r).collect();
        println!("{}", serde_json::to_string_pretty(&reports)?);
    } else {
        print!("{}", ekos_evals::history::render_table(&runs));
    }
    Ok(())
}
