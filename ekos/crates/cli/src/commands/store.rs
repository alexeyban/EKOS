//! Backend selection (RFC 0016): every command opens the knowledge store
//! through here. A migrated workspace — one with a fact-engine store at
//! `.ekos/ledger/facts/` — is served by [`FactLedger`]; otherwise the
//! SQLite [`Ledger`] serves as before. Migration is explicit
//! (`ekos ledger migrate --v3`), never implicit.

use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_ledger::{FactLedger, KnowledgeStore, Ledger};
use std::path::{Path, PathBuf};

/// Where a migrated workspace's fact store lives.
pub fn facts_dir(config: &EkosConfig, cwd: &Path) -> PathBuf {
    config.ledger_dir(cwd).join("facts")
}

/// True when this workspace runs on the fact engine.
pub fn uses_fact_engine(config: &EkosConfig, cwd: &Path) -> bool {
    facts_dir(config, cwd).join("manifest.json").exists()
}

/// Open the workspace's knowledge store with backend auto-detection.
pub fn open_store(config: &EkosConfig, cwd: &Path) -> Result<Box<dyn KnowledgeStore>> {
    if uses_fact_engine(config, cwd) {
        let dir = facts_dir(config, cwd);
        Ok(Box::new(FactLedger::open(&dir).map_err(|e| {
            anyhow::anyhow!("cannot open fact ledger at {}: {e}", dir.display())
        })?))
    } else {
        let path = config.ledger_path(cwd);
        Ok(Box::new(Ledger::open(&path).map_err(|e| {
            anyhow::anyhow!("cannot open ledger at {}: {e}", path.display())
        })?))
    }
}

/// Human-readable location of whatever backend is active (for CLI output).
pub fn store_display(config: &EkosConfig, cwd: &Path) -> String {
    if uses_fact_engine(config, cwd) {
        facts_dir(config, cwd).display().to_string()
    } else {
        config.ledger_path(cwd).display().to_string()
    }
}
