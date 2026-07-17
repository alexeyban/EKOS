use anyhow::Result;
use ekos_artifact::{ArtifactStore, PackArtifactStore};
use ekos_compiler_core::{EkosConfig, pass::PassContext, scheduler::FailureMode};
use ekos_semantic::SemanticCompilerPass;
use std::{path::Path, sync::Arc};

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
    let model: serde_json::Value = ekos_common::compress::read_json_auto(&model_path)?;
    let obj_count = model["objects"].as_array().map(|a| a.len()).unwrap_or(0);
    let rel_count = model["relationships"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);
    let written_path = ekos_common::compress::resolve_auto(&model_path).unwrap_or(model_path);

    println!("Compile complete.");
    println!("  Objects:       {obj_count}");
    println!("  Relationships: {rel_count}");
    println!("  CKM written:   {}", written_path.display());
    if ctx.diagnostics.lock().unwrap().has_warnings() {
        println!(
            "  Warnings:      {} (check logs)",
            ctx.diagnostics.lock().unwrap().warning_count()
        );
    }

    Ok(())
}
