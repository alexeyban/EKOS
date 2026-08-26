use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_ledger::Ledger;
use std::path::Path;

pub fn status(config: &EkosConfig, cwd: &Path, storage: bool) -> Result<()> {
    if super::store::uses_fact_engine(config, cwd) {
        let store = super::store::open_store(config, cwd)?;
        println!(
            "Ledger: {} (fact engine, RFC 0016)",
            super::store::store_display(config, cwd)
        );
        println!("  Total entries : {}", store.entry_count()?);
        println!("  Objects       : {}", store.object_count()?);
        if storage {
            let (bytes, files) = dir_size(&super::store::facts_dir(config, cwd));
            println!();
            println!(
                "  Fact store    : {:>10}  ({files} files)",
                human_bytes(bytes)
            );
        }
        return Ok(());
    }

    let path = config.ledger_path(cwd);

    if !path.exists() {
        println!("Ledger not initialised. Run `ekos commit` first.");
        return Ok(());
    }

    let ledger = Ledger::open(&path).map_err(|e| anyhow::anyhow!("cannot open ledger: {e}"))?;

    let entry_count = ledger.entry_count()?;
    let object_count = ledger.object_count()?;

    println!("Ledger: {}", path.display());
    println!("  Total entries : {entry_count}");
    println!("  Objects       : {object_count}");

    if storage {
        print_storage_report(config, cwd, &ledger)?;
    }

    Ok(())
}

/// Migrate the main ledger to the v2 compact format (RFC 0015): zstd
/// payloads with a corpus-trained dictionary, binary ids/signatures,
/// contentless FTS. Preserves full append-only history; leaves the original
/// file as `ledger.db.bak`.
pub fn migrate(config: &EkosConfig, cwd: &Path, v3: bool) -> Result<()> {
    let path = config.ledger_path(cwd);
    if !path.exists() {
        println!("Ledger not initialised. Run `ekos build` first.");
        return Ok(());
    }
    if v3 {
        return migrate_v3(config, cwd, &path);
    }

    println!("Migrating {} to v2 (RFC 0015)...", path.display());
    let report = ekos_ledger::migrate_to_v2(&path)
        .map_err(|e| anyhow::anyhow!("migration failed (original left untouched): {e}"))?;

    let ratio = report.bytes_before as f64 / report.bytes_after.max(1) as f64;
    println!("Migration complete.");
    println!("  Entries migrated : {}", report.entries);
    println!("  Objects          : {}", report.objects);
    println!("  Relationships    : {}", report.relationships);
    if report.dict_bytes > 0 {
        println!(
            "  Dictionary       : {} bytes (trained on this corpus)",
            report.dict_bytes
        );
    } else {
        println!("  Dictionary       : none (corpus too small; plain zstd frames)");
    }
    println!(
        "  Size             : {} -> {} ({ratio:.1}x)",
        human_bytes(report.bytes_before),
        human_bytes(report.bytes_after)
    );
    println!("  Backup           : {}", report.backup_path.display());
    Ok(())
}

/// Migrate the v1/v2 SQLite ledger into the RFC 0016 fact engine. The
/// source is left untouched; the workspace switches backends the moment
/// `.ekos/ledger/facts/manifest.json` exists (see `commands::store`).
fn migrate_v3(config: &EkosConfig, cwd: &Path, src: &Path) -> Result<()> {
    let dest = super::store::facts_dir(config, cwd);
    println!(
        "Migrating {} to the fact engine at {} ...",
        src.display(),
        dest.display()
    );
    let report = ekos_ledger::migrate_to_v3(src, &dest)
        .map_err(|e| anyhow::anyhow!("migration failed (source left untouched): {e}"))?;

    let ratio = report.bytes_before as f64 / report.bytes_after.max(1) as f64;
    println!("Migration complete — every version signature-verified.");
    println!("  Versions migrated : {}", report.versions);
    println!("  Objects           : {}", report.objects);
    println!("  Relationships     : {}", report.relationships);
    println!(
        "  Size              : {} -> {} ({ratio:.1}x)",
        human_bytes(report.bytes_before),
        human_bytes(report.bytes_after)
    );
    println!("  Backend           : fact engine now serves this workspace");
    println!(
        "  Rollback          : delete {} to return to SQLite",
        dest.display()
    );
    Ok(())
}

/// Real repair-tool report (RFC 0105 Phase 2) — surfaces `FactLedger`'s existing crash-recovery
/// primitives, previously only exercised by unit tests, never by any real command (the exact gap
/// RFC 0080's investigation found: `TODO.md` had called ledger recovery "the only option is a
/// full migration rollback," which was still accurate before this). Opening the ledger writable
/// already performs its two free self-heals before this function's own checks even run — a torn
/// active-segment tail is truncated, and stale/unreadable index runs are dropped and rebuilt from
/// the memtable path — both are automatic on every writable open, not new behavior added here.
/// `verify_sealed_report` then checks every sealed segment's hash unconditionally, so this reports
/// every real problem found, not just the first.
///
/// FactLedger-only (RFC 0105's own Non-goals): the SQLite backend has no segment/manifest concept
/// to repair — `PRAGMA integrity_check` already exists and does the analogous job for it.
pub fn repair(config: &EkosConfig, cwd: &Path) -> Result<()> {
    if !super::store::uses_fact_engine(config, cwd) {
        if config.ledger_path(cwd).exists() {
            anyhow::bail!(
                "`ekos ledger repair` only supports the fact engine (RFC 0016) — this workspace \
                 is still on the SQLite backend; SQLite's own `PRAGMA integrity_check` covers the \
                 analogous job there, not this command."
            );
        }
        println!("Ledger not initialised. Run `ekos build` first.");
        return Ok(());
    }
    let dest = super::store::facts_dir(config, cwd);

    println!(
        "Opening {} (self-heals any torn active-segment tail or stale index runs)...",
        dest.display()
    );
    let ledger = ekos_ledger::FactLedger::open(&dest)
        .map_err(|e| anyhow::anyhow!("cannot open ledger for repair: {e}"))?;

    let report = ledger.verify_sealed_report();
    if report.is_empty() {
        println!("No sealed segments yet — nothing to verify.");
        return Ok(());
    }

    let mut bad = 0usize;
    for check in &report {
        if check.ok {
            println!(
                "  segment {:06} [tx {}..{}] OK",
                check.seq, check.tx_min.0, check.tx_max.0
            );
        } else {
            bad += 1;
            println!(
                "  segment {:06} [tx {}..{}] FAILED — {}",
                check.seq, check.tx_min.0, check.tx_max.0, check.detail
            );
        }
    }
    println!();
    println!(
        "Repair report: {} segment(s) checked, {} OK, {bad} failed.",
        report.len(),
        report.len() - bad
    );

    if bad > 0 {
        anyhow::bail!(
            "{bad} sealed segment(s) failed verification — real local corruption with no \
             automatic fix (this format has no redundancy to reconstruct lost bytes from); \
             restore the affected file(s) from a backup if you have one, or knowingly accept the \
             loss for the transaction range(s) reported above."
        );
    }
    println!("All sealed segments verified clean.");
    Ok(())
}

/// Per-component byte report for the whole `.ekos` workspace (RFC 0015).
/// This is the before/after instrument for every storage change.
fn print_storage_report(config: &EkosConfig, cwd: &Path, ledger: &Ledger) -> Result<()> {
    let ekos_dir = config.ekos_dir(cwd);

    println!();
    println!("Storage: {}", ekos_dir.display());

    let mut total = 0u64;
    for (label, dir) in [
        ("artifacts", config.artifact_dir(cwd)),
        ("ledger", config.ledger_dir(cwd)),
        ("snapshots", ekos_dir.join("snapshots")),
        ("ckm", ekos_dir.join("ckm")),
    ] {
        let (bytes, files) = dir_size(&dir);
        total += bytes;
        println!("  {label:<10}: {:>10}  ({files} files)", human_bytes(bytes));
    }
    println!("  {:<10}: {:>10}", "total", human_bytes(total));

    let tables = ledger.storage_stats()?;
    if !tables.is_empty() {
        println!();
        println!("Ledger tables (dbstat):");
        for (name, bytes) in tables {
            println!("  {name:<24}: {:>10}", human_bytes(bytes));
        }
    }

    Ok(())
}

/// Recursive (bytes, file_count) of a directory; (0, 0) if it doesn't exist.
pub(crate) fn dir_size(dir: &Path) -> (u64, u64) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (0, 0);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let (b, f) = dir_size(&path);
            bytes += b;
            files += f;
        } else if let Ok(meta) = entry.metadata() {
            bytes += meta.len();
            files += 1;
        }
    }
    (bytes, files)
}

pub(crate) fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirObject, ObjectKind};
    use tempfile::tempdir;

    /// Real sealed segments (RFC 0105 Phase 2 needs at least one to verify) via a tiny seal
    /// threshold — bypasses `repair()`'s own `FactLedger::open` (default threshold) to write the
    /// fixture, then drops the handle so `repair()` reopens cleanly, the same way a real second
    /// `ekos` invocation would.
    fn seed_sealed_segments(dest: &Path, count: usize) {
        let ledger = ekos_ledger::FactLedger::open_with_seal_threshold(dest, 1).unwrap();
        for i in 0..count {
            ledger
                .append_object(&KirObject::new(format!("table-{i}"), ObjectKind::Table))
                .unwrap();
        }
        // Force the search index's lazy group-commit (`fact_ledger.rs`'s own module doc: "commit
        // lazily on the first query after a write") so its `last_tx` marker actually gets
        // written to disk. Without this, a later reopen finds no search marker at all and — a
        // real, separate mechanism from the index-runs replay this fixture is meant to exercise
        // — falls back to replaying every sealed segment's raw body just to catch the search
        // index up, which would make `repair`'s corruption test fail for the wrong reason (the
        // reopen itself erroring) rather than the one it's meant to exercise (`repair` opening
        // cleanly and its own segment-hash check finding the real problem).
        ledger.find_objects("table").unwrap();
    }

    #[test]
    fn repair_reports_every_segment_ok_on_a_healthy_workspace() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let dest = super::super::store::facts_dir(&config, dir.path());
        seed_sealed_segments(&dest, 3);

        repair(&config, dir.path()).expect("a healthy workspace must report clean, not error");
    }

    #[test]
    fn repair_fails_and_names_exactly_the_corrupted_segment() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let dest = super::super::store::facts_dir(&config, dir.path());
        seed_sealed_segments(&dest, 3);

        // Flip a byte inside the first sealed segment on disk — the same technique
        // `ekos-ledger`'s own `verify_sealed_report` tests use.
        let seg = dest.join("segments/seg-000000.facts");
        let mut bytes = std::fs::read(&seg).unwrap();
        let at = bytes.len() - 3;
        bytes[at] ^= 0xFF;
        std::fs::write(&seg, &bytes).unwrap();

        let err = repair(&config, dir.path())
            .expect_err("a corrupted sealed segment must be reported as a real failure");
        let msg = err.to_string();
        assert!(
            msg.contains("1 sealed segment"),
            "error should name how many segments failed, got: {msg}"
        );
    }

    #[test]
    fn repair_refuses_the_sqlite_backend_with_a_clear_message() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        // A pre-existing SQLite ledger, no fact-engine directory at all.
        ekos_ledger::Ledger::open(&config.ledger_path(dir.path())).unwrap();

        let err = repair(&config, dir.path())
            .expect_err("SQLite backend must be refused, not silently no-op'd");
        assert!(err.to_string().contains("PRAGMA integrity_check"));
    }

    #[test]
    fn repair_on_a_never_built_workspace_is_a_clean_no_op() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        repair(&config, dir.path()).expect("a never-built workspace must not error");
    }
}
