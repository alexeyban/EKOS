use anyhow::Result;
use ekos_artifact::{ArtifactId, ArtifactStore, PackArtifactStore};
use ekos_compiler_core::{
    EkosConfig,
    pass::{CompilerPass, PassContext},
    scheduler::FailureMode,
};
use ekos_recovery::{
    ConfluenceAnalyzerPass, CryptoAnalyzerPass, DependencyAnalyzerPass, DialectRule,
    DocumentSemanticsAnalyzerPass, DocumentSemanticsStats, GitAnalyzerPass, GitHubAnalyzerPass,
    LocalDocAnalyzerPass, MockLlmProvider, OllamaProvider, PentahoAnalyzerPass, PentahoStats,
    SqlAnalyzerPass, SqlTransformAnalyzerPass, SqlTransformStats, anthropic::AnthropicProvider,
    build_dialect_registry, cache::CachedLlmProvider, llm::LlmProvider, resolve_dialect_name,
};
use std::collections::HashMap;
use std::{path::Path, sync::Arc};
use walkdir::WalkDir;

pub async fn run(config: &EkosConfig, cwd: &Path, parallel: bool) -> Result<()> {
    let artifact_dir = config.artifact_dir(cwd);
    // Shared with the pass context below — two pack stores over the same
    // segments would go stale on each other's appends (RFC 0015).
    let artifact_store: Arc<dyn ArtifactStore> = Arc::new(
        PackArtifactStore::open(&artifact_dir)
            .map_err(|e| anyhow::anyhow!("cannot open artifact store: {e}"))?,
    );

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
    let mut sql_transform_stats_handles: Vec<Arc<std::sync::Mutex<SqlTransformStats>>> = Vec::new();

    // ── SQL dialect resolution (RFC 0031) ─────────────────────────────────
    // Registry is compile-time (mirrors the Observer plugin pattern) — a new dialect means a
    // new crate implementing `SqlDialectParser`, registered here, not a runtime plugin load.
    let dialect_registry = build_dialect_registry();
    let dialect_rules: Vec<DialectRule> = config
        .recover
        .sql
        .dialect_rules
        .iter()
        .map(|r| DialectRule {
            path_glob: r.path_glob.clone(),
            dialect: r.dialect.clone(),
        })
        .collect();
    let default_dialect = &config.recover.sql.default_dialect;

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

            // Workspace-relative, not base-relative: with multiple observe
            // paths, two projects can hold the same base-relative SQL path
            // (e.g. `schema.sql`), and pass names must be unique.
            let rel = path.strip_prefix(cwd).unwrap_or(path);
            let rel_str = rel.to_string_lossy().into_owned();

            // Resolve which dialect this file parses with (RFC 0031): first matching
            // `[[recover.sql.dialect-rules]]` wins, else `default-dialect` ("generic" — the
            // ANSI baseline — if the whole `[recover.sql]` section is omitted).
            let dialect_name = resolve_dialect_name(&dialect_rules, default_dialect, &rel_str);
            let dialect_parser = match dialect_registry.get(dialect_name) {
                Some(parser) => parser.as_ref(),
                None => {
                    tracing::warn!(
                        "unknown dialect {dialect_name:?} configured for {rel_str}; falling back to generic"
                    );
                    dialect_registry
                        .get("generic")
                        .expect("generic dialect always registered")
                        .as_ref()
                }
            };

            let pass = SqlAnalyzerPass::new(&rel_str, sql.clone(), llm.clone(), dialect_parser);
            pass_manager.register(Box::new(pass));

            // ── Transformation IR extraction (RFC 0027 Phase 2) ───────────
            // Pure structural, no LLM — same SQL text, a different concern
            // (transformation logic, not schema/entities). Preprocessed the same way
            // `SqlAnalyzerPass` preprocesses internally (e.g. MySQL `DELIMITER` stripping),
            // so both passes see identically-normalized SQL text for this file.
            let preprocessed_sql = dialect_parser.preprocess(&sql);
            let transform_pass = SqlTransformAnalyzerPass::new(
                &rel_str,
                preprocessed_sql,
                dialect_name,
                dialect_parser.sqlparser_dialect(),
            );
            sql_transform_stats_handles.push(transform_pass.stats_handle());
            pass_manager.register(Box::new(transform_pass));

            sql_count += 1;
        }
    }

    // ── Source files for dependency-fact extraction (RFC 0019) ───────────
    // Pattern/regex-based only (no AST/call-graph) — one pass batching every
    // matched file so technology objects dedup within the batch before
    // append_object's content-addressed idempotency takes over across runs.
    const DEP_SCAN_EXTENSIONS: &[&str] = &["py", "js", "ts", "java", "go", "rb"];
    let mut dep_files: Vec<(String, String)> = Vec::new();
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
            let is_candidate = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| {
                    DEP_SCAN_EXTENSIONS
                        .iter()
                        .any(|ext| e.eq_ignore_ascii_case(ext))
                })
                .unwrap_or(false);
            if !is_candidate {
                continue;
            }

            let content = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("cannot read {}: {e}", path.display());
                    continue;
                }
            };

            let rel = path.strip_prefix(cwd).unwrap_or(path);
            dep_files.push((rel.to_string_lossy().replace('\\', "/"), content));
        }
    }
    let dep_file_count = dep_files.len();
    if !dep_files.is_empty() {
        let dep_pass = DependencyAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            dep_files,
        );
        pass_manager.register(Box::new(dep_pass));
    }

    // ── Git commit artifacts ─────────────────────────────────────────────
    let (commit_ids, repo_id) = collect_git_artifact_ids(&*artifact_store);
    let git_count = commit_ids.len();
    if !commit_ids.is_empty() {
        let git_pass = GitAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            commit_ids,
            repo_id,
        );
        pass_manager.register(Box::new(git_pass));
    }

    // ── Crypto export artifacts (RFC 0017) ───────────────────────────────
    let crypto_artifact_ids = collect_crypto_artifact_ids(&*artifact_store);
    let crypto_batch_count = crypto_artifact_ids.len();
    if !crypto_artifact_ids.is_empty() {
        let crypto_pass = CryptoAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            crypto_artifact_ids,
        );
        pass_manager.register(Box::new(crypto_pass));
    }

    // ── GitHub issue/PR artifacts (RFC 0020) ─────────────────────────────
    let github_artifact_ids = collect_github_artifact_ids(&*artifact_store);
    let github_item_count = github_artifact_ids.len();
    if !github_artifact_ids.is_empty() {
        let github_pass = GitHubAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            github_artifact_ids,
        );
        pass_manager.register(Box::new(github_pass));
    }

    // ── Confluence page artifacts (RFC 0022) ─────────────────────────────
    let confluence_artifact_ids = collect_confluence_artifact_ids(&*artifact_store);
    let confluence_page_count = confluence_artifact_ids.len();
    if !confluence_artifact_ids.is_empty() {
        let confluence_pass = ConfluenceAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            confluence_artifact_ids,
        );
        pass_manager.register(Box::new(confluence_pass));
    }

    // ── Local document artifacts (RFC 0023) ──────────────────────────────
    let localdocs_artifact_ids = collect_localdocs_artifact_ids(&*artifact_store);
    let localdocs_count = localdocs_artifact_ids.len();
    let mut docsem_stats: Option<Arc<std::sync::Mutex<DocumentSemanticsStats>>> = None;
    if !localdocs_artifact_ids.is_empty() {
        let localdocs_pass = LocalDocAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            localdocs_artifact_ids,
        );

        // ── LLM document-semantics extraction (RFC 0026) ─────────────────
        // Opt-in: one LLM call per document section, so it stays off unless
        // asked for. Reuses the provider already selected above.
        if should_register_document_semantics(config, localdocs_count) {
            let docsem_pass =
                DocumentSemanticsAnalyzerPass::new(localdocs_pass.name(), llm.clone())
                    .with_max_sections(config.document_semantics.max_sections);
            docsem_stats = Some(docsem_pass.stats_handle());
            pass_manager.register(Box::new(docsem_pass));
        }

        pass_manager.register(Box::new(localdocs_pass));
    }

    // ── Pentaho .ktr/.kjb artifacts (RFC 0027 Phase 3) ────────────────────
    let pentaho_artifact_ids = collect_pentaho_artifact_ids(&*artifact_store);
    let pentaho_count = pentaho_artifact_ids.len();
    let mut pentaho_stats: Option<Arc<std::sync::Mutex<PentahoStats>>> = None;
    if !pentaho_artifact_ids.is_empty() {
        let pentaho_pass = PentahoAnalyzerPass::new(
            cwd.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            pentaho_artifact_ids,
        );
        pentaho_stats = Some(pentaho_pass.stats_handle());
        pass_manager.register(Box::new(pentaho_pass));
    }

    if pass_manager.is_empty() {
        println!(
            "Nothing to recover (no SQL files, git artifacts, crypto batches, dependency-scan source files, GitHub items, Confluence pages, local documents, or Pentaho jobs found)."
        );
        return Ok(());
    }

    // ── Run passes ────────────────────────────────────────────────────────
    let mut ctx = PassContext::new(Arc::new(config.clone()), cwd.to_path_buf())
        .with_artifact_store(artifact_store);
    let report = if parallel {
        pass_manager
            .run_all_parallel(&ctx, FailureMode::Collect)
            .await
            .map_err(|e| anyhow::anyhow!("scheduler error: {e}"))?
    } else {
        pass_manager
            .run_all(&mut ctx, FailureMode::Collect)
            .await
            .map_err(|e| anyhow::anyhow!("scheduler error: {e}"))?
    };

    let errors: Vec<_> = report.error_outcomes().collect();

    println!("Recover complete.");
    println!("  SQL files analysed: {sql_count}");
    println!("  Git commits analysed: {git_count}");
    if crypto_batch_count > 0 {
        println!("  Crypto batches analysed: {crypto_batch_count}");
    }
    if dep_file_count > 0 {
        println!("  Source files scanned for dependencies: {dep_file_count}");
    }
    if github_item_count > 0 {
        println!("  GitHub issues/PRs analysed: {github_item_count}");
    }
    if confluence_page_count > 0 {
        println!("  Confluence pages analysed: {confluence_page_count}");
    }
    if localdocs_count > 0 {
        println!("  Local documents analysed: {localdocs_count}");
    }
    if let Some(stats) = &docsem_stats {
        let s = *stats.lock().unwrap();
        println!(
            "  Document concepts extracted: {} sections processed, {} concepts, {} relationships",
            s.sections_processed, s.concepts, s.relationships
        );
    }
    if pentaho_count > 0 {
        println!("  Pentaho jobs analysed: {pentaho_count}");
    }
    if let Some(stats) = &pentaho_stats {
        let s = *stats.lock().unwrap();
        println!(
            "  Transformation IR nodes (Pentaho): {} total, {:.0}% mapped (non-Unmapped)",
            s.nodes_total,
            s.coverage_percent()
        );
    }
    if !sql_transform_stats_handles.is_empty() {
        let mut nodes_total = 0usize;
        let mut nodes_mapped = 0usize;
        for handle in &sql_transform_stats_handles {
            let s = handle.lock().unwrap();
            nodes_total += s.nodes_total;
            nodes_mapped += s.nodes_mapped;
        }
        let coverage = if nodes_total == 0 {
            0.0
        } else {
            100.0 * nodes_mapped as f32 / nodes_total as f32
        };
        println!(
            "  Transformation IR nodes (SQL): {nodes_total} total, {coverage:.0}% mapped (non-Unmapped)"
        );
    }
    println!("  Passes run: {}", report.passes_run());
    if report.passes_skipped() > 0 {
        println!("  Passes skipped (cached): {}", report.passes_skipped());
    }
    if parallel {
        println!("  Mode: parallel");
    }
    if !errors.is_empty() {
        println!("  Errors ({}):", errors.len());
        for o in &errors {
            if let Err(e) = &o.result {
                println!("    {}: {e}", o.pass_name);
            }
        }
    }
    if ctx.diagnostics.lock().unwrap().has_errors() {
        anyhow::bail!("recovery completed with errors");
    }
    Ok(())
}

/// Collect ArtifactIds for all git commit and repo artifacts currently in the store.
fn collect_git_artifact_ids(store: &dyn ArtifactStore) -> (Vec<ArtifactId>, Option<ArtifactId>) {
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

/// Collect ArtifactIds for every crypto export-batch artifact currently in the store.
fn collect_crypto_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    let all_ids = match store.list() {
        Ok(ids) => ids,
        Err(_) => return Vec::new(),
    };

    // Dedup by target (batch_id): content-addressing already makes re-processing an
    // unchanged batch a no-op downstream, but there's no reason to pass duplicate ids.
    let mut by_batch: HashMap<String, ArtifactId> = HashMap::new();
    for id in all_ids {
        if let Ok(Some(json)) = store.read(&id)
            && json["connector_name"].as_str() == Some("crypto")
        {
            let batch_id = json["target"].as_str().unwrap_or_default().to_string();
            by_batch.insert(batch_id, id);
        }
    }
    let mut ids: Vec<ArtifactId> = by_batch.into_values().collect();
    ids.sort_by_key(|id| id.to_string());
    ids
}

/// Collect ArtifactIds for every GitHub issue/PR artifact currently in the store (RFC 0020).
fn collect_github_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    let all_ids = match store.list() {
        Ok(ids) => ids,
        Err(_) => return Vec::new(),
    };

    let mut ids: Vec<ArtifactId> = all_ids
        .into_iter()
        .filter(|id| {
            store
                .read(id)
                .ok()
                .flatten()
                .is_some_and(|json| json["connector_name"].as_str() == Some("github"))
        })
        .collect();
    ids.sort_by_key(|id| id.to_string());
    ids
}

/// Collect ArtifactIds for every Confluence page artifact currently in the store (RFC 0022).
fn collect_confluence_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    let all_ids = match store.list() {
        Ok(ids) => ids,
        Err(_) => return Vec::new(),
    };

    let mut ids: Vec<ArtifactId> = all_ids
        .into_iter()
        .filter(|id| {
            store
                .read(id)
                .ok()
                .flatten()
                .is_some_and(|json| json["connector_name"].as_str() == Some("confluence"))
        })
        .collect();
    ids.sort_by_key(|id| id.to_string());
    ids
}

/// Collect ArtifactIds for every local-document artifact currently in the store (RFC 0023).
fn collect_localdocs_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    let all_ids = match store.list() {
        Ok(ids) => ids,
        Err(_) => return Vec::new(),
    };

    let mut ids: Vec<ArtifactId> = all_ids
        .into_iter()
        .filter(|id| {
            store
                .read(id)
                .ok()
                .flatten()
                .is_some_and(|json| json["connector_name"].as_str() == Some("localdocs"))
        })
        .collect();
    ids.sort_by_key(|id| id.to_string());
    ids
}

/// Collect ArtifactIds for every Pentaho `.ktr`/`.kjb` artifact currently in
/// the store (RFC 0027 Phase 3).
fn collect_pentaho_artifact_ids(store: &dyn ArtifactStore) -> Vec<ArtifactId> {
    let all_ids = match store.list() {
        Ok(ids) => ids,
        Err(_) => return Vec::new(),
    };

    let mut ids: Vec<ArtifactId> = all_ids
        .into_iter()
        .filter(|id| {
            store
                .read(id)
                .ok()
                .flatten()
                .is_some_and(|json| json["connector_name"].as_str() == Some("pentaho"))
        })
        .collect();
    ids.sort_by_key(|id| id.to_string());
    ids
}

/// RFC 0026's opt-in gate: the document-semantics pass costs one LLM call per
/// document section, so it registers only when explicitly enabled *and* there is
/// local-document output for it to read.
fn should_register_document_semantics(config: &EkosConfig, localdocs_count: usize) -> bool {
    config.document_semantics.enabled && localdocs_count > 0
}

/// Choose LLM provider (RFC 0021): `[llm] provider = "ollama"` in
/// `ekos.toml` routes to a local Ollama daemon (no key required —
/// unreachability surfaces as an ordinary error on first use, not here);
/// anything else tries Anthropic with cache if an API key is present, mock
/// otherwise.
pub(crate) fn build_llm_provider(config: &EkosConfig, artifact_dir: &Path) -> Arc<dyn LlmProvider> {
    let cache_dir = artifact_dir
        .parent()
        .unwrap_or(artifact_dir)
        .join("llm-cache");
    std::fs::create_dir_all(&cache_dir).ok();

    if config.llm.provider.as_deref() == Some("ollama") {
        tracing::info!("using local Ollama provider with disk cache");
        return Arc::new(CachedLlmProvider::new(
            OllamaProvider::from_env(),
            cache_dir,
        ));
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_compiler_core::config::{DocumentSemanticsConfig, LlmConfig};
    use tempfile::tempdir;

    /// RFC 0026 acceptance criterion: zero LLM calls unless the user opted in.
    /// A config that never mentions `[document-semantics]` must leave the pass
    /// unregistered even when there are local documents to analyse.
    #[test]
    fn document_semantics_pass_not_registered_when_config_absent() {
        let config = EkosConfig::default();
        assert!(!config.document_semantics.enabled);
        assert!(!should_register_document_semantics(&config, 12));
    }

    #[test]
    fn document_semantics_pass_not_registered_when_explicitly_disabled() {
        let config = EkosConfig {
            document_semantics: DocumentSemanticsConfig {
                enabled: false,
                max_sections: Some(10),
            },
            ..Default::default()
        };
        assert!(!should_register_document_semantics(&config, 12));
    }

    /// Enabled but nothing to read is still no registration — the pass would
    /// otherwise declare a dependency on a `LocalDocAnalyzerPass` that was never
    /// registered, which the scheduler rejects as an unknown dependency.
    #[test]
    fn document_semantics_pass_not_registered_without_local_documents() {
        let mut config = EkosConfig::default();
        config.document_semantics.enabled = true;
        assert!(!should_register_document_semantics(&config, 0));
    }

    #[test]
    fn document_semantics_pass_registered_when_enabled_with_local_documents() {
        let mut config = EkosConfig::default();
        config.document_semantics.enabled = true;
        assert!(should_register_document_semantics(&config, 1));
    }

    /// RFC 0021: `provider = "ollama"` must route here without ever
    /// attempting a network call — `model_name()` alone proves which
    /// provider was actually selected.
    #[test]
    fn ollama_provider_selected_when_configured() {
        let dir = tempdir().unwrap();
        let mut config = EkosConfig::default();
        config.llm = LlmConfig {
            provider: Some("ollama".to_string()),
            api_key_env: None,
            model: None,
        };
        let provider = build_llm_provider(&config, dir.path());
        assert_eq!(provider.model_name(), "llama3.1:8b");
    }

    /// Anything other than "ollama" falls through to the existing
    /// Anthropic/Mock chain untouched.
    #[test]
    fn non_ollama_provider_falls_back_to_existing_chain() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let provider = build_llm_provider(&config, dir.path());
        // Without ANTHROPIC_API_KEY set in the test environment this lands
        // on the mock; either way it must not be the Ollama default model.
        assert_ne!(provider.model_name(), "llama3.1:8b");
    }
}
