use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_ledger::{merge_branch, Ledger};
use std::path::Path;

pub fn create(config: &EkosConfig, cwd: &Path, name: &str) -> Result<()> {
    let branch_path = config.branch_ledger_path(cwd, name);
    if branch_path.exists() {
        anyhow::bail!("branch '{name}' already exists at {}", branch_path.display());
    }

    let ledger_path = config.ledger_path(cwd);
    let ledger = Ledger::open(&ledger_path).map_err(|e| {
        anyhow::anyhow!("cannot open ledger at {}: {e}\nRun `ekos build` first.", ledger_path.display())
    })?;

    ledger.vacuum_into(&branch_path)?;
    println!("Created branch '{name}' at {}", branch_path.display());
    Ok(())
}

pub fn list(config: &EkosConfig, cwd: &Path) -> Result<()> {
    let ledger_dir = config.ledger_dir(cwd);
    if !ledger_dir.exists() {
        println!("No ledger found. Run `ekos build` first.");
        return Ok(());
    }

    println!("main");
    for entry in std::fs::read_dir(&ledger_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("db") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        if stem == "ledger" {
            continue; // main ledger, already printed above
        }
        println!("{stem}");
    }
    Ok(())
}

pub fn merge(config: &EkosConfig, cwd: &Path, name: &str) -> Result<()> {
    let branch_path = config.branch_ledger_path(cwd, name);
    if !branch_path.exists() {
        anyhow::bail!("branch '{name}' not found at {}", branch_path.display());
    }

    let main_ledger = Ledger::open(&config.ledger_path(cwd))?;
    let branch_ledger = Ledger::open(&branch_path)?;

    let report = merge_branch(&main_ledger, &branch_ledger)?;

    println!("Merge complete.");
    println!("  Objects merged:       {}", report.objects_merged);
    println!("  Relationships merged: {}", report.relationships_merged);
    if !report.conflicts.is_empty() {
        println!("  Conflicts ({}):", report.conflicts.len());
        for c in &report.conflicts {
            println!("    {}: {}", c.object_id, c.reason);
        }
    }
    Ok(())
}

pub fn delete(config: &EkosConfig, cwd: &Path, name: &str) -> Result<()> {
    let branch_path = config.branch_ledger_path(cwd, name);
    if !branch_path.exists() {
        anyhow::bail!("branch '{name}' not found at {}", branch_path.display());
    }
    std::fs::remove_file(&branch_path)?;
    println!("Deleted branch '{name}'");
    Ok(())
}
