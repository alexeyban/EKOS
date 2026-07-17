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
