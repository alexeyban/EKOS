use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_ledger::{FactLedger, KnowledgeStore, merge_stores};
use std::path::{Path, PathBuf};

use super::store::{open_store, uses_fact_engine};

/// Branch location for the active backend: a `.db` file for SQLite, a
/// directory under `branches/` for the fact engine.
fn branch_path(config: &EkosConfig, cwd: &Path, name: &str) -> PathBuf {
    if uses_fact_engine(config, cwd) {
        config.ledger_dir(cwd).join("branches").join(name)
    } else {
        config.branch_ledger_path(cwd, name)
    }
}

fn open_branch(config: &EkosConfig, cwd: &Path, path: &Path) -> Result<Box<dyn KnowledgeStore>> {
    if uses_fact_engine(config, cwd) {
        Ok(Box::new(FactLedger::open(path)?))
    } else {
        Ok(Box::new(ekos_ledger::Ledger::open(path)?))
    }
}

pub fn create(config: &EkosConfig, cwd: &Path, name: &str) -> Result<()> {
    let branch_path = branch_path(config, cwd, name);
    if branch_path.exists() {
        anyhow::bail!(
            "branch '{name}' already exists at {}",
            branch_path.display()
        );
    }

    let ledger =
        open_store(config, cwd).map_err(|e| anyhow::anyhow!("{e}\nRun `ekos build` first."))?;
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
    if uses_fact_engine(config, cwd) {
        let branches = ledger_dir.join("branches");
        if branches.exists() {
            for entry in std::fs::read_dir(&branches)? {
                let path = entry?.path();
                if path.is_dir()
                    && let Some(name) = path.file_name().and_then(|s| s.to_str())
                {
                    println!("{name}");
                }
            }
        }
        return Ok(());
    }
    for entry in std::fs::read_dir(&ledger_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("db") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem == "ledger" {
            continue; // main ledger, already printed above
        }
        println!("{stem}");
    }
    Ok(())
}

pub fn merge(config: &EkosConfig, cwd: &Path, name: &str) -> Result<()> {
    let branch_path = branch_path(config, cwd, name);
    if !branch_path.exists() {
        anyhow::bail!("branch '{name}' not found at {}", branch_path.display());
    }

    let main_ledger = open_store(config, cwd)?;
    let branch_ledger = open_branch(config, cwd, &branch_path)?;

    let report = merge_stores(&*main_ledger, &*branch_ledger)?;

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
    let branch_path = branch_path(config, cwd, name);
    if !branch_path.exists() {
        anyhow::bail!("branch '{name}' not found at {}", branch_path.display());
    }
    if branch_path.is_dir() {
        std::fs::remove_dir_all(&branch_path)?;
    } else {
        std::fs::remove_file(&branch_path)?;
    }
    println!("Deleted branch '{name}'");
    Ok(())
}
