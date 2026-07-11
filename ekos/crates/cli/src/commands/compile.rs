use anyhow::Result;
use ekos_compiler_core::{pass::PassContext, scheduler::FailureMode, EkosConfig};
use ekos_semantic::SemanticCompilerPass;
use std::{path::Path, sync::Arc};

pub async fn run(config: &EkosConfig, cwd: &Path) -> Result<()> {
    let ckm_dir = config.ekos_dir(cwd).join("ckm");

    let mut pass_manager = ekos_compiler_core::pass::PassManager::new();
    pass_manager.register(Box::new(SemanticCompilerPass::new(&ckm_dir)));

    let mut ctx = PassContext::new(Arc::new(config.clone()), cwd.to_path_buf());
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
    let json = std::fs::read_to_string(&model_path)?;
    let model: serde_json::Value = serde_json::from_str(&json)?;
    let obj_count = model["objects"].as_array().map(|a| a.len()).unwrap_or(0);
    let rel_count = model["relationships"].as_array().map(|a| a.len()).unwrap_or(0);

    println!("Compile complete.");
    println!("  Objects:       {obj_count}");
    println!("  Relationships: {rel_count}");
    println!("  CKM written:   {}", model_path.display());
    if ctx.diagnostics.has_warnings() {
        println!("  Warnings:      {} (check logs)", ctx.diagnostics.warning_count());
    }

    Ok(())
}
