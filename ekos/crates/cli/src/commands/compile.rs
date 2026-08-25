use anyhow::Result;
use ekos_artifact::{ArtifactStore, PackArtifactStore};
use ekos_compiler_core::{EkosConfig, pass::PassContext, scheduler::FailureMode};
use ekos_kir::ObjectKind;
use ekos_semantic::{CkModel, SemanticCompilerPass};
use std::{collections::HashSet, path::Path, sync::Arc};

/// Ids of every knowledge artifact currently in the store — the semantic
/// compiler's actual inputs, declared so the Phase 13 cache invalidates when
/// recover output changes.
fn knowledge_artifact_ids(store: &dyn ArtifactStore) -> Vec<String> {
    let Ok(ids) = store.list() else {
        return Vec::new();
    };
    ids.into_iter()
        .filter(|id| {
            matches!(
                store.read(id),
                Ok(Some(json)) if json["artifact_type"].as_str() == Some("knowledge")
            )
        })
        .map(|id| id.to_string())
        .collect()
}

pub async fn run(config: &EkosConfig, cwd: &Path) -> Result<()> {
    let ckm_dir = config.ekos_dir(cwd).join("ckm");

    // One store instance shared with the pass context: two pack stores over
    // the same segments would go stale on each other's appends (RFC 0015).
    let store: Arc<dyn ArtifactStore> = Arc::new(
        PackArtifactStore::open(config.artifact_dir(cwd))
            .map_err(|e| anyhow::anyhow!("cannot open artifact store: {e}"))?,
    );
    let mut pass_manager = ekos_compiler_core::pass::PassManager::new();
    pass_manager.register(Box::new(
        SemanticCompilerPass::new(&ckm_dir).with_cache_inputs(knowledge_artifact_ids(&*store)),
    ));

    let mut ctx =
        PassContext::new(Arc::new(config.clone()), cwd.to_path_buf()).with_artifact_store(store);
    let report = pass_manager
        .run_all(&mut ctx, FailureMode::FailFast)
        .await
        .map_err(|e| anyhow::anyhow!("compile scheduler error: {e}"))?;

    if report.has_errors() {
        for o in report.error_outcomes() {
            if let Err(e) = &o.result {
                eprintln!("  {}: {e}", o.pass_name);
            }
        }
        anyhow::bail!("semantic compilation failed");
    }

    // Read back and summarise.
    let model_path = ckm_dir.join("model.json");
    let model: CkModel = ekos_common::compress::read_json_auto(&model_path)?;
    let obj_count = model.objects.len();
    let rel_count = model.relationships.len();
    let written_path = ekos_common::compress::resolve_auto(&model_path).unwrap_or(model_path);

    println!("Compile complete.");
    println!("  Objects:       {obj_count}");
    println!("  Relationships: {rel_count}");
    println!("  CKM written:   {}", written_path.display());
    if ctx.diagnostics.lock().unwrap().has_warnings() {
        let warning_count = ctx.diagnostics.lock().unwrap().warning_count();
        // RFC 0076: "(check logs)" used to point nowhere real — diagnostics only ever logged at
        // `tracing::debug!`, invisible at this project's own default `log-level = "info"`. Now a
        // real file, and the message says so.
        match super::diagnostics_log::write_diagnostics_log(&config.ekos_dir(cwd), "compile", &ctx)
        {
            Ok(Some(path)) => println!(
                "  Warnings:      {}",
                describe_warnings(warning_count, &model, config, cwd, &path)
            ),
            Ok(None) => println!(
                "  Warnings:      {}",
                describe_warnings_without_log(warning_count, &model, config, cwd)
            ),
            Err(e) => println!("  Warnings:      {warning_count} (could not write log: {e})"),
        }
    }

    Ok(())
}

/// SEM002 ("unknown from/to-id") fires for *every* relationship pointing at a `File` object,
/// structurally — `File`s are written straight to the ledger by `ekos build`, never through a
/// `KnowledgeArtifact` this compile stage reads (`CkModel::dangling_relationship_target_ids`'s own
/// doc comment has the full explanation), so they're always outside the CKM's own object set even
/// though they resolve correctly once `ekos commit` runs. Left unexplained, this looked like a
/// real, unresolved discrepancy between `ekos resolve`'s 0-conflict report and this stage's own
/// warning count (found live, 2026-08-25, `devlog_101`/`devlog_104`: 1252 SEM002 warnings on a
/// ledger `resolve` reported zero conflicts for). Cross-references the dangling ids against the
/// ledger's real `File` objects (already written by the time `compile` runs) so the count actually
/// shown distinguishes this expected, by-design gap from a genuinely dangling reference.
fn classify_dangling_ids(model: &CkModel, config: &EkosConfig, cwd: &Path) -> (usize, usize) {
    let dangling = model.dangling_relationship_target_ids();
    if dangling.is_empty() {
        return (0, 0);
    }
    let Ok(store) = super::store::open_store(config, cwd) else {
        return (0, dangling.len());
    };
    let Ok(objects) = store.all_objects() else {
        return (0, dangling.len());
    };
    let known_file_ids: HashSet<_> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::File)
        .map(|o| o.id)
        .collect();
    let expected = dangling
        .iter()
        .filter(|id| known_file_ids.contains(id))
        .count();
    (expected, dangling.len() - expected)
}

fn describe_warnings(
    warning_count: usize,
    model: &CkModel,
    config: &EkosConfig,
    cwd: &Path,
    log_path: &Path,
) -> String {
    let (expected, real) = classify_dangling_ids(model, config, cwd);
    if expected == 0 {
        return format!("{warning_count} (see {})", log_path.display());
    }
    format!(
        "{warning_count} ({expected} are expected File-object references — see \
         CkModel::dangling_relationship_target_ids' doc comment — resolve correctly after `ekos \
         commit`; {real} other(s), see {})",
        log_path.display()
    )
}

fn describe_warnings_without_log(
    warning_count: usize,
    model: &CkModel,
    config: &EkosConfig,
    cwd: &Path,
) -> String {
    let (expected, real) = classify_dangling_ids(model, config, cwd);
    if expected == 0 {
        return warning_count.to_string();
    }
    format!("{warning_count} ({expected} expected File-object references, {real} other(s))")
}
