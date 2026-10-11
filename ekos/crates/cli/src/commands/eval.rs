//! `ekos eval run` (RFC 0138) — runs an `ekos-evals` scenario dataset against a real,
//! already-built workspace and prints the `EKOS EVALUATION` report. Owns opening the store and
//! building the `LlmProvider`, exactly like `ask.rs` — `ekos-evals` itself never touches
//! configuration or credentials.

use super::recover::{build_llm_provider, resolved_key_env};
use super::store::open_store_read_only;
use anyhow::Result;
use ekos_artifact::PackArtifactStore;
use ekos_compiler_core::EkosConfig;
use ekos_evals::report::{self, GateThresholds};
use ekos_evals::schema::load_dataset;
use ekos_recovery::{LlmProvider, MOCK_MODEL_NAME};
use ekos_runtime::{AiRuntime, Runtime};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    /// Persist each scenario's answer text and the evidence it was shown into the saved report
    /// (RFC 0139 §1). Off by default because a 101-scenario suite's transcripts are ~1MB of JSON;
    /// on, a saved report can be re-graded offline instead of re-run against the LLM.
    pub save_answers: bool,
    /// RFC 0177 §5 — measure retrieval and evidence assembly only; no model is called.
    pub evidence_only: bool,
    /// Search the corpus as it is, including documents that quote a scenario's question.
    pub keep_question_echoes: bool,
}

/// Human label for what actually answered. Records the resolved model for every provider, not just
/// ollama — a report that says only `"claude"` cannot be compared against a later run on a
/// different model (RFC 0139 §5).
fn agent_label(config: &EkosConfig) -> String {
    let model = config.llm.model.as_deref().unwrap_or("default");
    match config.llm.provider.as_deref() {
        Some("ollama") => format!("ollama ({model})"),
        Some("openai") => format!("openai ({model})"),
        _ => format!("claude ({model})"),
    }
}

/// `config` with `--agent` applied. `[llm] model` and `base-url` name a model and an endpoint of
/// the *configured* provider, so they are dropped when the run switches to another one: with
/// `provider = "openai"`, `model = "deepseek-…"`, `--agent ollama` used to ask the local Ollama
/// for the cloud model's name and fail every scenario.
fn with_agent(config: &EkosConfig, agent: Option<&str>) -> Result<EkosConfig> {
    let mut run_config = config.clone();
    let provider = match agent {
        None => return Ok(run_config),
        Some("claude") | Some("anthropic") => None,
        Some("ollama") => Some("ollama".to_string()),
        Some("openai") => Some("openai".to_string()),
        Some(other) => anyhow::bail!("unknown --agent {other:?} (want claude/ollama/openai)"),
    };
    if provider != run_config.llm.provider {
        run_config.llm.model = None;
        run_config.llm.base_url = None;
    }
    run_config.llm.provider = provider;
    Ok(run_config)
}

/// RFC 0138 Phase 4: `build_llm_provider` silently degrades to the stub `MockLlmProvider` when
/// the configured provider's API key isn't set — a reasonable default for `recover`/`commit`
/// (structural analysis without LLM enrichment is a legitimate degraded mode there), but for
/// `ekos eval` it would produce a fully-formed, publishable report scoring canned stub answers as
/// if a real model generated them. There is no `--agent mock` option (`run`'s own match above
/// only accepts claude/anthropic/ollama/openai), so reaching the mock here can only mean the
/// fallback fired, never a deliberate choice.
fn check_not_mock(llm: &dyn LlmProvider, agent: &str, key_env: &str) -> Result<()> {
    if llm.model_name() != MOCK_MODEL_NAME {
        return Ok(());
    }
    anyhow::bail!(
        "no usable LLM provider is configured for agent {agent:?} — refusing to run `ekos eval` \
         against the stub MockLlmProvider, which would silently produce a fully-formed, \
         publishable report scoring answers no model actually generated.\n\nSet {key_env} in the \
         environment, or pass `--agent ollama` to grade against a local model instead. A local \
         model needs a genuinely powerful server to keep pace with this suite — this repo's own \
         measured P95 is 36s/scenario on `llama3:latest` — so a published reference baseline \
         should still come from a cloud model."
    )
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

    let run_config = with_agent(config, opts.agent)?;

    let artifact_dir = run_config.artifact_dir(cwd);
    let llm: Arc<dyn LlmProvider> = if opts.evidence_only {
        // Never asked: the runner skips the model call.
        Arc::new(ekos_recovery::llm::MockLlmProvider::new(String::new()))
    } else {
        let llm = build_llm_provider(&run_config, &artifact_dir);
        check_not_mock(
            llm.as_ref(),
            &agent_label(&run_config),
            resolved_key_env(&run_config),
        )?;
        llm
    };
    let ledger = open_store_read_only(&run_config, cwd)?;
    // RFC 0177 §3 D — a document that quotes a scenario's question outranks the object that
    // answers it. Found once from the dataset itself and left out of search for the whole run.
    let echoes = if opts.keep_question_echoes {
        Default::default()
    } else {
        ekos_evals::echoes::question_echoes(&ledger.all_objects()?, &scenarios)
    };
    let echo_count = echoes.len();
    let runtime = Runtime::over(&*ledger).excluding_from_search(echoes);
    let mut ai = AiRuntime::new(&runtime, llm, super::ask::ai_config(&run_config));
    // RFC 0140 §3 — same best-effort wiring as `ask.rs`: grading must not depend on the artifact
    // store being present, only benefit from it when it is.
    if let Ok(store) = PackArtifactStore::open(&artifact_dir) {
        ai = ai.with_artifact_store(Arc::new(store));
    }

    // RFC 0173 — graded answers mark disputed claims exactly as `ekos ask` does.
    if run_config.conflicts.enabled {
        ai = ai.with_disputes(super::conflicts::dispute_lookup(&run_config, cwd, &*ledger));
    }

    let outcomes =
        ekos_evals::run_all_with(&ai, &runtime, &*ledger, &scenarios, opts.evidence_only).await;
    let agent = if opts.evidence_only {
        "none (evidence only)".to_string()
    } else {
        agent_label(&run_config)
    };
    let mut report = report::build_with_transcripts(
        &dataset_name,
        &agent,
        "local",
        &outcomes,
        GateThresholds::default(),
        opts.save_answers,
    );
    report.evidence_only = opts.evidence_only;
    report.question_echoes_excluded = echo_count;

    if let Some(output) = &opts.output {
        tokio::fs::write(output, serde_json::to_string_pretty(&report)?).await?;
    } else if !opts.evidence_only {
        // An evidence-only run graded no answers, so it is not a row in `ekos eval history`.
        let default_dir = cwd.join("evals").join("reports");
        if default_dir.is_dir() {
            let filename = format!(
                "{}-{}.json",
                report.generated_at.format("%Y%m%dT%H%M%SZ"),
                dataset_name
            );
            tokio::fs::write(
                default_dir.join(filename),
                serde_json::to_string_pretty(&report)?,
            )
            .await
            .ok();
        }
    }

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("{}", report::render_text(&report));
    }

    if opts.evidence_only {
        if report.metrics.errored > 0 {
            std::process::exit(1);
        }
        return Ok(());
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

pub struct EvalRegradeOpts {
    pub report: PathBuf,
    pub datasets_dir: Option<PathBuf>,
    pub json: bool,
    pub output: Option<PathBuf>,
}

/// Re-score a saved report under the *current* evaluators, with no LLM calls (RFC 0139 §2).
///
/// This is how a grading change gets published honestly: run it over the same saved answers before
/// and after, and whatever moves is the ruler rather than the system. Without it, a ruler change
/// and a real improvement are indistinguishable in the trend table.
pub fn regrade(cwd: &Path, config: &EkosConfig, opts: EvalRegradeOpts) -> Result<()> {
    let raw = std::fs::read_to_string(&opts.report)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", opts.report.display()))?;
    let saved: report::Report = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("parsing {}: {e}", opts.report.display()))?;

    let datasets_dir = opts
        .datasets_dir
        .clone()
        .unwrap_or_else(|| cwd.join("evals").join("datasets"));
    // Grade against the dataset as it stands now: a dataset edit is a ruler change too, and this
    // command exists to measure exactly that.
    let (_, scenarios) = load_dataset(Some(saved.dataset.as_str()), &datasets_dir)
        .or_else(|_| load_dataset(None, &datasets_dir))
        .map_err(|e| anyhow::anyhow!("loading datasets from {}: {e}", datasets_dir.display()))?;

    let ledger = open_store_read_only(config, cwd)?;
    let regraded = ekos_evals::regrade::regrade(&saved, &scenarios, &*ledger)?;

    if let Some(output) = &opts.output {
        std::fs::write(output, serde_json::to_string_pretty(&regraded)?)?;
    }
    if opts.json {
        println!("{}", serde_json::to_string_pretty(&regraded)?);
    } else {
        println!(
            "Re-graded {} under ruler v{} (was v{}) — no LLM calls.\n",
            opts.report.display(),
            regraded.ruler_version,
            saved.ruler_version
        );
        println!("{}", report::render_text(&regraded));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_recovery::MockLlmProvider;

    #[test]
    fn switching_agent_drops_the_configured_providers_model_and_endpoint() {
        let mut config = EkosConfig::default();
        config.llm.provider = Some("openai".into());
        config.llm.model = Some("deepseek-v4-flash".into());
        config.llm.base_url = Some("https://example.invalid/v1".into());

        let local = with_agent(&config, Some("ollama")).unwrap();
        assert_eq!(local.llm.provider.as_deref(), Some("ollama"));
        assert_eq!(local.llm.model, None);
        assert_eq!(local.llm.base_url, None);

        // The same provider, or no override, keeps what the config says.
        for agent in [None, Some("openai")] {
            let same = with_agent(&config, agent).unwrap();
            assert_eq!(same.llm.model.as_deref(), Some("deepseek-v4-flash"));
            assert!(same.llm.base_url.is_some());
        }
        assert!(with_agent(&config, Some("gpt")).is_err());
    }

    #[test]
    fn a_real_provider_passes_the_mock_check() {
        // Any `model_name()` other than the mock's fixed value must pass — a real provider's
        // actual model name is never known statically here (it depends on config/env), so this
        // stands in for "anything that isn't the stub".
        let provider = MockLlmProvider {
            model: "claude-opus-5".to_string(),
            response: String::new(),
        };
        assert!(check_not_mock(&provider, "claude (default)", "ANTHROPIC_API_KEY").is_ok());
    }

    #[test]
    fn the_stub_mock_provider_is_refused_with_an_actionable_message() {
        let provider = MockLlmProvider::new("{}");
        let err = check_not_mock(&provider, "openai (gpt-4o-mini)", "OPENAI_API_KEY")
            .expect_err("the fixed mock model name must be refused");
        let msg = err.to_string();
        assert!(
            msg.contains("OPENAI_API_KEY"),
            "must name the actual key the caller's provider needs, not a hardcoded one: {msg}"
        );
        assert!(
            msg.contains("--agent ollama"),
            "must point at a real way out: {msg}"
        );
    }

    #[test]
    fn resolved_key_env_defaults_per_provider_not_one_shared_default() {
        let mut config = EkosConfig::default();
        assert_eq!(resolved_key_env(&config), "ANTHROPIC_API_KEY");

        config.llm.provider = Some("openai".to_string());
        assert_eq!(
            resolved_key_env(&config),
            "OPENAI_API_KEY",
            "an openai-configured workspace with no explicit api-key-env override must check \
             OPENAI_API_KEY, not silently fall back to ANTHROPIC_API_KEY"
        );

        config.llm.api_key_env = Some("CUSTOM_KEY".to_string());
        assert_eq!(
            resolved_key_env(&config),
            "CUSTOM_KEY",
            "an explicit [llm] api-key-env override must always win"
        );
    }
}
