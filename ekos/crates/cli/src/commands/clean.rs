use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use std::path::Path;

pub fn run(config: &EkosConfig, cwd: &Path) -> Result<()> {
    let artifact_dir = config.artifact_dir(cwd);

    if !artifact_dir.exists() {
        println!("Nothing to clean (artifact cache does not exist).");
        return Ok(());
    }

    let mut deleted = 0usize;
    for entry in std::fs::read_dir(&artifact_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            std::fs::remove_file(&path)?;
            deleted += 1;
        } else if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
            deleted += 1;
        }
    }

    println!("Clean complete. {deleted} item(s) removed from artifact cache.");
    println!("Ledger at {} was not modified.", config.ledger_path(cwd).display());
    Ok(())
}
