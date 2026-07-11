use anyhow::Result;
use chrono::{DateTime, Utc};
use ekos_compiler_core::EkosConfig;
use ekos_ledger::{diff_ledger, Ledger};
use std::path::Path;

pub fn run(config: &EkosConfig, cwd: &Path, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<()> {
    let ledger_path = config.ledger_path(cwd);
    let ledger = Ledger::open(&ledger_path).map_err(|e| {
        anyhow::anyhow!("cannot open ledger at {}: {e}\nRun `ekos build` first.", ledger_path.display())
    })?;

    let diff = diff_ledger(&ledger, from, to)?;

    println!("Diff {} .. {}", from.to_rfc3339(), to.to_rfc3339());
    println!("  Added:     {}", diff.added.len());
    for id in &diff.added {
        println!("    entry #{}", id.0);
    }
    println!("  Unchanged: {}", diff.unchanged);

    Ok(())
}
