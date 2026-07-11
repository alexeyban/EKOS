use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_ledger::Ledger;
use std::path::Path;

pub fn status(config: &EkosConfig, cwd: &Path) -> Result<()> {
    let path = config.ledger_path(cwd);

    if !path.exists() {
        println!("Ledger not initialised. Run `ekos commit` first.");
        return Ok(());
    }

    let ledger = Ledger::open(&path)
        .map_err(|e| anyhow::anyhow!("cannot open ledger: {e}"))?;

    let entry_count = ledger.entry_count()?;
    let object_count = ledger.object_count()?;

    println!("Ledger: {}", path.display());
    println!("  Total entries : {entry_count}");
    println!("  Objects       : {object_count}");

    Ok(())
}
