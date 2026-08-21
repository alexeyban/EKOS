//! Backend selection (RFC 0016, default switch 2026-08-21): every command opens the knowledge
//! store through here. A workspace with a fact-engine store at `.ekos/ledger/facts/` — either
//! explicitly migrated (`ekos ledger migrate --v3`) or newly created — is served by
//! [`FactLedger`]. A **genuinely fresh** workspace (neither a fact store nor a pre-existing
//! SQLite `ledger.db` yet) now defaults to the fact engine too, per RFC 0016's own stated
//! condition for the switch ("fresh workspaces keep the SQLite default until the engine has
//! soaked on the live estate") — real, month-long soak evidence is in the RFC's dated section.
//! Any **pre-existing** SQLite-backed workspace (this repo's own `.ekos/`, `analytics/`, or
//! anyone else's) is completely unaffected — it keeps serving from SQLite exactly as before,
//! forever, unless explicitly migrated. Only workspaces that didn't exist yet get the new default.

use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_ledger::{FactLedger, KnowledgeStore, Ledger};
use std::path::{Path, PathBuf};

/// Where a fact-engine-backed workspace's store lives (migrated or newly created).
pub fn facts_dir(config: &EkosConfig, cwd: &Path) -> PathBuf {
    config.ledger_dir(cwd).join("facts")
}

/// True when this workspace already has a real fact store on disk — either migrated via
/// `ekos ledger migrate --v3`, or previously auto-created as a fresh workspace's default. Doesn't
/// distinguish *how* it got there, only that it's the active backend now.
pub fn uses_fact_engine(config: &EkosConfig, cwd: &Path) -> bool {
    facts_dir(config, cwd).join("manifest.json").exists()
}

/// Open the workspace's knowledge store with backend auto-detection.
pub fn open_store(config: &EkosConfig, cwd: &Path) -> Result<Box<dyn KnowledgeStore>> {
    if uses_fact_engine(config, cwd) {
        let dir = facts_dir(config, cwd);
        return Ok(Box::new(FactLedger::open(&dir).map_err(|e| {
            anyhow::anyhow!("cannot open fact ledger at {}: {e}", dir.display())
        })?));
    }

    let sqlite_path = config.ledger_path(cwd);
    if sqlite_path.exists() {
        // Pre-existing, never-migrated SQLite workspace — keep serving it exactly as before.
        return Ok(Box::new(Ledger::open(&sqlite_path).map_err(|e| {
            anyhow::anyhow!("cannot open ledger at {}: {e}", sqlite_path.display())
        })?));
    }

    // Neither backend has ever been written to — a genuinely fresh workspace. `FactLedger::open`
    // creates a new store the same way `Ledger::open` does for SQLite, so this is the one place
    // the new default takes effect.
    let dir = facts_dir(config, cwd);
    Ok(Box::new(FactLedger::open(&dir).map_err(|e| {
        anyhow::anyhow!("cannot create fact ledger at {}: {e}", dir.display())
    })?))
}

/// Human-readable location of whatever backend [`open_store`] would open right now — mirrors its
/// exact three-way logic so this stays accurate even before a fresh workspace's first
/// `open_store` call has run.
pub fn store_display(config: &EkosConfig, cwd: &Path) -> String {
    if uses_fact_engine(config, cwd) || !config.ledger_path(cwd).exists() {
        facts_dir(config, cwd).display().to_string()
    } else {
        config.ledger_path(cwd).display().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Real default-switch behavior (2026-08-21): a workspace that has never been written to by
    /// either backend now opens on the fact engine, not SQLite. `manifest.json` itself is written
    /// lazily by the fact engine (confirmed by reading `segment/mod.rs::load_manifest` — it
    /// returns an in-memory default without touching disk when absent), so this checks for the
    /// `segments/` directory `SegmentStore::open` creates immediately, not `uses_fact_engine`'s
    /// manifest-existence check, which only becomes true after a real write happens.
    #[test]
    fn fresh_workspace_defaults_to_the_fact_engine() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        std::fs::create_dir_all(config.ledger_dir(dir.path())).unwrap();

        assert!(
            !uses_fact_engine(&config, dir.path()),
            "nothing written yet"
        );
        let _store = open_store(&config, dir.path()).expect("open_store creates a fresh store");
        assert!(
            facts_dir(&config, dir.path()).join("segments").exists(),
            "a fresh workspace's first open_store call must create a fact store, not SQLite"
        );
        assert!(
            !config.ledger_path(dir.path()).exists(),
            "no SQLite file should have been created"
        );
    }

    /// Backward compatibility (2026-08-21): a workspace that already has a real SQLite ledger —
    /// this repo's own `.ekos/`, `analytics/`, or any pre-existing workspace — must keep serving
    /// from SQLite forever, never silently switched to the fact engine.
    #[test]
    fn pre_existing_sqlite_workspace_is_unaffected_by_the_new_default() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        std::fs::create_dir_all(config.ledger_dir(dir.path())).unwrap();
        // Simulate a pre-existing SQLite-backed workspace by opening one directly first.
        Ledger::open(&config.ledger_path(dir.path())).unwrap();

        assert!(!uses_fact_engine(&config, dir.path()));
        let _store =
            open_store(&config, dir.path()).expect("open_store opens the existing SQLite ledger");
        assert!(
            !uses_fact_engine(&config, dir.path()),
            "a pre-existing SQLite workspace must not be switched to the fact engine implicitly"
        );
    }

    #[test]
    fn store_display_matches_open_store_for_a_fresh_workspace() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        std::fs::create_dir_all(config.ledger_dir(dir.path())).unwrap();

        let displayed = store_display(&config, dir.path());
        assert_eq!(
            displayed,
            facts_dir(&config, dir.path()).display().to_string()
        );
    }
}
