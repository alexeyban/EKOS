use anyhow::Result;
use ekos_artifact::PackArtifactStore;
use ekos_compiler_core::EkosConfig;
use std::path::Path;

use super::ledger::{dir_size, human_bytes};

/// Migrate loose artifact files into pack segments (RFC 0015). Every
/// artifact is verified to read back identically before its loose file is
/// removed; ArtifactIds are unchanged, so nothing referencing an id notices.
pub fn repack(config: &EkosConfig, cwd: &Path) -> Result<()> {
    let dir = config.artifact_dir(cwd);
    if !dir.exists() {
        println!(
            "No artifact store at {}. Run `ekos build` first.",
            dir.display()
        );
        return Ok(());
    }

    let (bytes_before, files_before) = dir_size(&dir);
    println!(
        "Repacking {} ({} files, {})...",
        dir.display(),
        files_before,
        human_bytes(bytes_before)
    );

    let store = PackArtifactStore::open(&dir)
        .map_err(|e| anyhow::anyhow!("cannot open artifact store: {e}"))?;
    let (migrated, already_packed) = store
        .repack_loose()
        .map_err(|e| anyhow::anyhow!("repack failed: {e}"))?;
    drop(store);

    let (bytes_after, files_after) = dir_size(&dir);
    let ratio = bytes_before as f64 / bytes_after.max(1) as f64;
    println!("Repack complete.");
    println!("  Artifacts packed : {migrated}");
    if already_packed > 0 {
        println!("  Already packed   : {already_packed} (loose duplicates removed)");
    }
    println!("  Files            : {files_before} -> {files_after}");
    println!(
        "  Size             : {} -> {} ({ratio:.1}x)",
        human_bytes(bytes_before),
        human_bytes(bytes_after)
    );
    Ok(())
}
