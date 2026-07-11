use anyhow::Result;
use ekos_artifact::{ArtifactId, ArtifactStore, FileSystemArtifactStore};
use ekos_compiler_core::{pass::PassContext, scheduler::FailureMode, EkosConfig};
use ekos_recovery::{
    anthropic::AnthropicProvider, cache::CachedLlmProvider, llm::LlmProvider,
    GitAnalyzerPass, MockLlmProvider, SqlAnalyzerPass,
};
use std::{path::Path, sync::Arc};
use walkdir::WalkDir;

pub async fn run(config: &EkosConfig, cwd: &Path) -> Result<()> {
    let artifact_dir = config.artifact_dir(cwd);
    let artifact_store = FileSystemArtifactStore::new(&artifact_dir);

    // ── LLM provider selection ────────────────────────────────────────────
    let llm: Arc<dyn LlmProvider> = build_llm_provider(config, &artifact_dir);

    // ── Build PassContext ─────────────────────────────────────────────────
    let mut pass_manager = ekos_compiler_core::pass::PassManager::new();

    // ── SQL files ─────────────────────────────────────────────────────────
    let observe_paths: Vec<std::path::PathBuf> = if config.observe.paths.is_empty() {
        vec![cwd.to_path_buf()]
    } else {
        config.observe.paths.iter().map(|p| cwd.join(p)).collect()
    };

    let ignore = &config.observe.ignore_patterns;
    let mut sql_count = 0usize;

    for base in &observe_paths {
        for entry in WalkDir::new(base)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                if e.file_type().is_dir() {
                    let name = e.file_name().to_str().unwrap_or("");
                    return !ignore.iter().any(|p| name == p.as_str());
                }
                true
            })
        {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let is_sql = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("sql"))
                .unwrap_or(false);
            if !is_sql {
                continue;
            }

            let sql = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("cannot read {}: {e}", path.display());
                    continue;
                }
            };

            let rel = path.strip_prefix(base).unwrap_or(path);
            let pass = SqlAnalyzerPass::new(
                rel.to_string_lossy().as_ref(),
                sql,
                llm.clone(),
            );
            pass_manager.register(Box::new(pass));
            sql_count += 1;
        }
    }

    // ── Git commit artifacts ─────────────────────────────────────────────
    let (commit_ids, repo_id) = collect_git_artifact_ids(&artifact_store);
    let git_count = commit_ids.len();
    if !commit_ids.is_empty() {
        let git_pass = GitAnalyzerPass::new(
            cwd.file_name().unwrap_or_default().to_string_lossy().as_ref(),
            commit_ids,
            repo_id,
        );
        pass_manager.register(Box::new(git_pass));
    }

    if pass_manager.is_empty() {
        println!("Nothing to recover (no SQL files and no git artifacts found).");
        return Ok(());
    }

    // ── Run passes ────────────────────────────────────────────────────────
    let mut ctx = PassContext::new(Arc::new(config.clone()), cwd.to_path_buf());
    let report = pass_manager
        .run_all(&mut ctx, FailureMode::Collect)
        .await
        .map_err(|e| anyhow::anyhow!("scheduler error: {e}"))?;

    let errors: Vec<_> = report.error_outcomes().collect();

    println!("Recover complete.");
    println!("  SQL files analysed: {sql_count}");
    println!("  Git commits analysed: {git_count}");
    println!("  Passes run: {}", report.passes_run());
    if !errors.is_empty() {
        println!("  Errors ({}):", errors.len());
        for o in &errors {
            if let Err(e) = &o.result {
                println!("    {}: {e}", o.pass_name);
            }
        }
    }
    if ctx.diagnostics.has_errors() {
        anyhow::bail!("recovery completed with errors");
    }
    Ok(())
}

/// Collect ArtifactIds for all git commit and repo artifacts currently in the store.
fn collect_git_artifact_ids(
    store: &FileSystemArtifactStore,
) -> (Vec<ArtifactId>, Option<ArtifactId>) {
    let all_ids = match store.list() {
        Ok(ids) => ids,
        Err(_) => return (vec![], None),
    };

    let mut commit_ids = vec![];
    let mut repo_id = None;

    for id in all_ids {
        if let Ok(Some(json)) = store.read(&id) {
            let connector = json["connector_name"].as_str().unwrap_or("");
            let target = json["target"].as_str().unwrap_or("");
            if connector == "git" {
                if target == "repo" {
                    repo_id = Some(id);
                } else {
                    commit_ids.push(id);
                }
            }
        }
    }

    (commit_ids, repo_id)
}

/// Choose LLM provider: Anthropic with cache if API key present, mock otherwise.
fn build_llm_provider(
    config: &EkosConfig,
    artifact_dir: &Path,
) -> Arc<dyn LlmProvider> {
    let cache_dir = artifact_dir.parent().unwrap_or(artifact_dir).join("llm-cache");
    std::fs::create_dir_all(&cache_dir).ok();

    let key_env = config
        .llm
        .api_key_env
        .as_deref()
        .unwrap_or("ANTHROPIC_API_KEY");

    match AnthropicProvider::from_env_var(key_env) {
        Ok(provider) => {
            tracing::info!("using Anthropic provider with disk cache");
            Arc::new(CachedLlmProvider::new(provider, cache_dir))
        }
        Err(_) => {
            tracing::warn!(
                "{key_env} not set — using structural analysis only (LLM enrichment skipped)"
            );
            Arc::new(MockLlmProvider::new(
                r#"{"entities":[],"relationships":[]}"#,
            ))
        }
    }
}
