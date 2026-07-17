use super::store::open_store;
use anyhow::Result;
use chrono::{DateTime, Utc};
use ekos_compiler_core::EkosConfig;
use std::path::Path;

pub fn run(config: &EkosConfig, cwd: &Path, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<()> {
    let ledger = open_store(config, cwd)?;

    let diff = ledger.diff(from, to)?;

    println!("Diff {} .. {}", from.to_rfc3339(), to.to_rfc3339());
    println!("  Added:     {}", diff.added.len());
    for id in &diff.added {
        println!("    entry #{}", id.0);
    }
    println!("  Unchanged: {}", diff.unchanged);

    Ok(())
}
