use super::store::open_store;
use anyhow::Result;
use chrono::{DateTime, Utc};
use ekos_compiler_core::EkosConfig;
use ekos_kir::KirId;
use ekos_ledger::KnowledgeStore;
use std::path::Path;
use std::str::FromStr;

/// How many touched entities to name before collapsing the rest into a count.
const MAX_LISTED: usize = 50;

pub fn run(config: &EkosConfig, cwd: &Path, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<()> {
    let ledger = open_store(config, cwd)?;

    let diff = ledger.diff(from, to)?;

    println!("Diff {} .. {}", from.to_rfc3339(), to.to_rfc3339());
    println!("  Versions written:  {}", diff.added.len());
    println!("  Entities touched:   {}", diff.touched.len());
    println!("  Unchanged:          {}", diff.unchanged);

    // `added` is a list of opaque per-backend entry ids (SQLite rowids / per-partition tx
    // numbers) — not useful to print. `touched` is the set of real logical ids; resolve each to a
    // name so the output is readable.
    if !diff.touched.is_empty() {
        println!();
        for id_str in diff.touched.iter().take(MAX_LISTED) {
            println!("    {}", label_for(&*ledger, id_str));
        }
        if diff.touched.len() > MAX_LISTED {
            println!("    … and {} more", diff.touched.len() - MAX_LISTED);
        }
    }

    Ok(())
}

/// Best-effort human label for a touched id: object name+kind, else relationship kind+endpoints,
/// else the raw id.
fn label_for(ledger: &dyn KnowledgeStore, id_str: &str) -> String {
    let Ok(id) = KirId::from_str(id_str) else {
        return id_str.to_string();
    };
    if let Ok(Some(obj)) = ledger.get_object(&id) {
        return format!("{}  ({})  [{id_str}]", obj.name, obj.kind);
    }
    if let Ok(Some(rel)) = ledger.get_relationship(&id) {
        return format!("{:?}: {} → {}  [{id_str}]", rel.kind, rel.from, rel.to);
    }
    id_str.to_string()
}
