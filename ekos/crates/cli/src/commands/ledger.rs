use anyhow::Result;
use chrono::{DateTime, Datelike, Utc};
use ekos_compiler_core::EkosConfig;
use ekos_ledger::Ledger;
use serde::Serialize;
use std::path::Path;

pub fn status(config: &EkosConfig, cwd: &Path, storage: bool, json: bool) -> Result<()> {
    // RFC 0127 R2: `--json` is a pure alternate presentation — it shares the same backend opener
    // as the text path below and adds zero side effects, so `ekos status` and `ekos status --json`
    // can never disagree, and RFC 0116's `ekos status` == `ekos ledger status` byte-identity for
    // the text form is preserved (the text body below is untouched).
    if json {
        let s = build_status_json(config, cwd)?;
        println!("{}", serde_json::to_string_pretty(&s)?);
        return Ok(());
    }

    // RFC 0111/0113 — a partitioned or distributed workspace is served through `open_store`, not a
    // single ledger file. `uses_fact_engine` only checks for a `facts/manifest.json`, which a
    // partitioned store doesn't have, so this branch must come first.
    if config.storage.distributed.is_enabled() || super::store::uses_partitioned(config, cwd) {
        let store = super::store::open_store(config, cwd)?;
        let kind = if config.storage.distributed.is_enabled() {
            "distributed cluster, RFC 0113"
        } else {
            "partitioned, RFC 0111"
        };
        println!(
            "Ledger: {} ({kind})",
            super::store::store_display(config, cwd)
        );
        println!("  Total entries : {}", store.entry_count()?);
        println!("  Objects       : {}", store.object_count()?);
        println!("  Relationships : {}", store.relationship_count()?);
        if storage && !config.storage.distributed.is_enabled() {
            let (bytes, files) = dir_size(&super::store::partitioned_root(config, cwd));
            println!();
            println!(
                "  Partition store: {:>10}  ({files} files)",
                human_bytes(bytes)
            );
        }
        return Ok(());
    }

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

/// RFC 0127 R2 — the machine-readable form of `ekos status`. One flat object plus a nested
/// storage breakdown; same field set on every backend so a consumer never has to branch.
#[derive(Debug, Serialize)]
pub struct StatusJson {
    pub schema_version: u32,
    pub workspace: String,
    /// `"sqlite-v1"` / `"sqlite-v2"` / `"fact-segment"` / `"partitioned"` / `"distributed"`.
    pub backend: &'static str,
    pub entries: usize,
    pub objects: usize,
    pub relationships: usize,
    /// `null` on the distributed gateway until a fan-out `evidence_count` RPC exists (RFC 0113).
    pub evidence: Option<usize>,
    /// Always `"unchecked"` in R2 — a real integrity pass (`verify_sealed_report` /
    /// `PRAGMA integrity_check`) is seconds-to-minutes and `status` must stay instant. A future
    /// `--verify` will populate this.
    pub integrity: &'static str,
    /// Newest mtime under the store root — a metadata-only proxy for "last write", not read from
    /// the ledger itself. `null` on a distributed workspace (no local store) or one never built.
    pub last_write: Option<DateTime<Utc>>,
    pub storage: StorageJson,
}

#[derive(Debug, Serialize)]
pub struct StorageJson {
    pub total_bytes: u64,
    pub components: Vec<StorageComponent>,
}

#[derive(Debug, Serialize)]
pub struct StorageComponent {
    pub name: &'static str,
    pub bytes: u64,
    pub files: u64,
}

/// Builds [`StatusJson`] without touching stdout — the testable core of `status --json`.
pub fn build_status_json(config: &EkosConfig, cwd: &Path) -> Result<StatusJson> {
    let workspace = cwd
        .canonicalize()
        .unwrap_or_else(|_| cwd.to_path_buf())
        .display()
        .to_string();
    let distributed = config.storage.distributed.is_enabled();
    let partitioned = super::store::uses_partitioned(config, cwd);
    let fact = super::store::uses_fact_engine(config, cwd);

    // Same opener the text path uses, so the two can't disagree about the backend.
    let (backend, entries, objects, relationships, evidence): (
        &'static str,
        usize,
        usize,
        usize,
        Option<usize>,
    ) = if distributed || partitioned {
        let store = super::store::open_store(config, cwd)?;
        let backend = if distributed {
            "distributed"
        } else {
            "partitioned"
        };
        (
            backend,
            store.entry_count()?,
            store.object_count()?,
            store.relationship_count()?,
            store.evidence_count().ok(),
        )
    } else if fact {
        let store = super::store::open_store(config, cwd)?;
        (
            "fact-segment",
            store.entry_count()?,
            store.object_count()?,
            store.relationship_count()?,
            store.evidence_count().ok(),
        )
    } else {
        let path = config.ledger_path(cwd);
        if !path.exists() {
            // Never-built SQLite-style workspace: honest zeros, no file to open.
            return Ok(StatusJson {
                schema_version: 1,
                workspace,
                backend: "fact-segment",
                entries: 0,
                objects: 0,
                relationships: 0,
                evidence: Some(0),
                integrity: "unchecked",
                last_write: None,
                storage: storage_json(config, cwd, distributed, partitioned, fact),
            });
        }
        let ledger = Ledger::open(&path).map_err(|e| anyhow::anyhow!("cannot open ledger: {e}"))?;
        (
            ledger.format_tag(),
            ledger.entry_count()?,
            ledger.object_count()?,
            ledger.relationship_count()?,
            Some(ledger.evidence_count()?),
        )
    };

    Ok(StatusJson {
        schema_version: 1,
        workspace,
        backend,
        entries,
        objects,
        relationships,
        evidence,
        integrity: "unchecked",
        last_write: last_write(config, cwd, distributed, partitioned, fact),
        storage: storage_json(config, cwd, distributed, partitioned, fact),
    })
}

/// Per-component byte/file breakdown, one shape per backend (RFC 0127 §5).
fn storage_json(
    config: &EkosConfig,
    cwd: &Path,
    distributed: bool,
    partitioned: bool,
    fact: bool,
) -> StorageJson {
    let ekos_dir = config.ekos_dir(cwd);
    let dirs: Vec<(&'static str, std::path::PathBuf)> = if distributed {
        vec![]
    } else if partitioned {
        vec![
            ("partitioned", super::store::partitioned_root(config, cwd)),
            ("artifacts", config.artifact_dir(cwd)),
        ]
    } else if fact || !config.ledger_path(cwd).exists() {
        vec![
            ("facts", super::store::facts_dir(config, cwd)),
            ("artifacts", config.artifact_dir(cwd)),
        ]
    } else {
        vec![
            ("ledger", config.ledger_dir(cwd)),
            ("artifacts", config.artifact_dir(cwd)),
            ("snapshots", ekos_dir.join("snapshots")),
            ("ckm", ekos_dir.join("ckm")),
        ]
    };

    let mut total = 0u64;
    let components = dirs
        .into_iter()
        .map(|(name, dir)| {
            let (bytes, files) = dir_size(&dir);
            total += bytes;
            StorageComponent { name, bytes, files }
        })
        .collect();
    StorageJson {
        total_bytes: total,
        components,
    }
}

/// Newest mtime among the *durable-write* files of the store — cheap, metadata-only. Deliberately
/// narrower than the whole store root: a fact/partitioned store rewrites its tantivy `search/`
/// meta on every *read-only* open too, so pointing at `segments/` keeps this a real "last write"
/// rather than "last opened". `None` for a distributed workspace or one never built.
fn last_write(
    config: &EkosConfig,
    cwd: &Path,
    distributed: bool,
    partitioned: bool,
    fact: bool,
) -> Option<DateTime<Utc>> {
    if distributed {
        return None;
    }
    let root = if partitioned {
        super::store::partitioned_root(config, cwd)
    } else if fact || !config.ledger_path(cwd).exists() {
        super::store::facts_dir(config, cwd).join("segments")
    } else {
        config.ledger_path(cwd)
    };
    newest_mtime(&root).map(DateTime::<Utc>::from)
}

/// The newest file-modification time at or below `path` (which may itself be a file). `None` if
/// nothing exists there yet.
fn newest_mtime(path: &Path) -> Option<std::time::SystemTime> {
    if path.is_file() {
        return std::fs::metadata(path).ok()?.modified().ok();
    }
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
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

// ── RFC 0129 R6 — `ekos ledger timeline --json` ───────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bucket {
    Day,
    Week,
    Month,
}

impl Bucket {
    fn parse(s: &str) -> Result<Self> {
        match s {
            "day" => Ok(Self::Day),
            "week" => Ok(Self::Week),
            "month" => Ok(Self::Month),
            other => anyhow::bail!("unknown --bucket {other:?} (expected day, week, or month)"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
        }
    }

    /// The label for the bucket that contains `t` (`YYYY-MM-DD` for day/week — week uses its
    /// Monday — and `YYYY-MM` for month).
    fn label(self, t: DateTime<Utc>) -> String {
        let d = t.date_naive();
        match self {
            Self::Day => d.format("%Y-%m-%d").to_string(),
            Self::Week => d
                .week(chrono::Weekday::Mon)
                .first_day()
                .format("%Y-%m-%d")
                .to_string(),
            Self::Month => d.format("%Y-%m").to_string(),
        }
    }

    /// Exclusive upper bound of the bucket named `label` — the instant the next bucket starts.
    fn end_of(self, label: &str) -> DateTime<Utc> {
        use chrono::NaiveDate;
        let start = match self {
            Self::Day | Self::Week => {
                NaiveDate::parse_from_str(label, "%Y-%m-%d").expect("own label format")
            }
            Self::Month => NaiveDate::parse_from_str(&format!("{label}-01"), "%Y-%m-%d")
                .expect("own label format"),
        };
        let next = match self {
            Self::Day => start.succ_opt().expect("in range"),
            Self::Week => start + chrono::Duration::days(7),
            Self::Month => {
                let (y, m) = (start.year(), start.month());
                let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
                NaiveDate::from_ymd_opt(ny, nm, 1).expect("valid first-of-month")
            }
        };
        next.and_hms_opt(0, 0, 0).expect("midnight").and_utc()
    }
}

#[derive(Debug, Serialize)]
pub struct Timeline {
    pub schema_version: u32,
    pub bucket: &'static str,
    /// Cumulative running totals as of the **end** of each bucket, oldest first. Empty buckets are
    /// omitted — a consumer carries the previous point's values forward. `created_at` on the
    /// compiled object/relationship is the timestamp used (set when the analyzer mints it, which
    /// is the same compile run that commits it — so it tracks "when this knowledge came to be").
    pub points: Vec<TimelinePoint>,
}

#[derive(Debug, Serialize)]
pub struct TimelinePoint {
    pub t: String,
    pub objects: usize,
    pub relationships: usize,
}

/// RFC 0129 R6. Backend-agnostic: one `all_objects` + one `all_relationships` pass (the same
/// dashboard-tier cost as `ekos graph export`), bucketed by `created_at`. No new `KnowledgeStore`
/// method, and it works identically on every backend.
pub fn timeline(config: &EkosConfig, cwd: &Path, bucket: &str, since: Option<&str>) -> Result<()> {
    let bucket = Bucket::parse(bucket)?;
    let since: Option<DateTime<Utc>> = since
        .map(|s| {
            DateTime::parse_from_rfc3339(s)
                .map(|d| d.with_timezone(&Utc))
                .map_err(|e| anyhow::anyhow!("--since is not an RFC 3339 timestamp: {e}"))
        })
        .transpose()?;

    let store = super::store::open_store_read_only(config, cwd)?;
    let out = build_timeline(store.as_ref(), bucket, since)?;
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

/// The stdout-free core of `timeline` — one `all_objects` + one `all_relationships` pass, bucketed
/// by `created_at` into cumulative running totals.
fn build_timeline(
    store: &dyn ekos_ledger::KnowledgeStore,
    bucket: Bucket,
    since: Option<DateTime<Utc>>,
) -> Result<Timeline> {
    let mut objs: Vec<DateTime<Utc>> = store.all_objects()?.iter().map(|o| o.created_at).collect();
    let mut rels: Vec<DateTime<Utc>> = store
        .all_relationships()?
        .iter()
        .map(|r| r.created_at)
        .collect();
    objs.sort_unstable();
    rels.sort_unstable();

    // Distinct bucket labels present in either series, filtered by `--since`. The cumulative
    // counts still reach back to the start of history — `--since` only trims which buckets show.
    let mut labels = std::collections::BTreeSet::new();
    for t in objs.iter().chain(rels.iter()) {
        if since.is_none_or(|s| *t >= s) {
            labels.insert(bucket.label(*t));
        }
    }

    let points = labels
        .into_iter()
        .map(|label| {
            let end = bucket.end_of(&label);
            TimelinePoint {
                t: label,
                objects: objs.iter().take_while(|t| **t < end).count(),
                relationships: rels.iter().take_while(|t| **t < end).count(),
            }
        })
        .collect();

    Ok(Timeline {
        schema_version: 1,
        bucket: bucket.as_str(),
        points,
    })
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
    use ekos_kir::{KirEvidence, KirObject, ObjectKind, SourceLocation};
    use tempfile::tempdir;

    fn ev(fragment: &str) -> KirEvidence {
        KirEvidence::new(SourceLocation::at("schema.sql", 1), fragment)
    }

    #[test]
    fn status_json_on_a_fresh_workspace_is_the_fact_backend_with_honest_zeros() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let s = build_status_json(&config, dir.path()).unwrap();
        assert_eq!(s.schema_version, 1);
        assert_eq!(s.backend, "fact-segment");
        assert_eq!(s.objects, 0);
        assert_eq!(s.relationships, 0);
        assert_eq!(s.evidence, Some(0));
        assert_eq!(s.integrity, "unchecked");
        assert!(s.last_write.is_none());
    }

    #[test]
    fn status_json_reports_the_sqlite_backend_tag_and_evidence_count() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        {
            let l = Ledger::open(&config.ledger_path(dir.path())).unwrap();
            l.append_object(&KirObject::new("orders", ObjectKind::Table))
                .unwrap();
            l.append_evidence(&ev("CREATE TABLE orders")).unwrap();
            l.append_evidence(&ev("CREATE TABLE customers")).unwrap();
            assert_eq!(l.evidence_count().unwrap(), 2);
            assert_eq!(l.format_tag(), "sqlite-v2");
        }
        let s = build_status_json(&config, dir.path()).unwrap();
        assert_eq!(s.backend, "sqlite-v2");
        assert_eq!(s.objects, 1);
        assert_eq!(s.evidence, Some(2));
        assert!(s.last_write.is_some());
        assert!(s.storage.components.iter().any(|c| c.name == "ledger"));
    }

    #[test]
    fn status_json_reports_the_fact_backend_evidence_count() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        {
            let store = super::super::store::open_store(&config, dir.path()).unwrap();
            store
                .append_object(&KirObject::new("orders", ObjectKind::Table))
                .unwrap();
            store.append_evidence(&ev("CREATE TABLE orders")).unwrap();
        }
        let s = build_status_json(&config, dir.path()).unwrap();
        assert_eq!(s.backend, "fact-segment");
        assert_eq!(s.evidence, Some(1));
    }

    #[test]
    fn status_json_reports_a_partitioned_backend() {
        let dir = tempdir().unwrap();
        let mut config = EkosConfig::default();
        config.storage.partition.dimension = Some("entity-kind".into());
        {
            let store = super::super::store::build_partitioned(&config, dir.path(), false).unwrap();
            store
                .append_object(&KirObject::new("orders", ObjectKind::Table))
                .unwrap();
        }
        let s = build_status_json(&config, dir.path()).unwrap();
        assert_eq!(s.backend, "partitioned");
        assert_eq!(s.objects, 1);
        assert_eq!(s.integrity, "unchecked");
    }

    #[test]
    fn json_run_has_no_side_effects_on_the_text_output() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        {
            let store = super::super::store::open_store(&config, dir.path()).unwrap();
            store
                .append_object(&KirObject::new("orders", ObjectKind::Table))
                .unwrap();
        }
        // A `--json` build in between must not change what the plain path computes.
        let before = build_status_json(&config, dir.path()).unwrap();
        status(&config, dir.path(), false, true).unwrap();
        let after = build_status_json(&config, dir.path()).unwrap();
        assert_eq!(before.objects, after.objects);
        assert_eq!(before.entries, after.entries);
    }

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

    /// Regression: `ekos status` / `ekos ledger status` printed "Ledger not initialised" on any
    /// `[storage.partition]` workspace because `uses_fact_engine` only checks for
    /// `facts/manifest.json` — which a partitioned store doesn't have.
    #[test]
    fn status_reports_a_partitioned_ledger_instead_of_claiming_it_is_uninitialised() {
        let dir = tempdir().unwrap();
        let mut config = EkosConfig::default();
        config.storage.partition.dimension = Some("entity-kind".into());

        {
            let store = super::super::store::build_partitioned(&config, dir.path(), false).unwrap();
            store
                .append_object(&KirObject::new("customers", ObjectKind::Table))
                .unwrap();
        }

        // The load-bearing check is that it doesn't fall through to the "not initialised" branch;
        // it prints to stdout, so we assert on the counts via the store directly too.
        status(&config, dir.path(), false, false)
            .expect("status must not error on a partitioned ledger");
        let store = super::super::store::open_store(&config, dir.path()).unwrap();
        assert_eq!(store.object_count().unwrap(), 1);
    }

    // ── RFC 0129 R6 — `ekos ledger timeline` ────────────────────────────────

    #[test]
    fn bucket_parse_rejects_anything_but_day_week_month() {
        assert_eq!(Bucket::parse("day").unwrap(), Bucket::Day);
        assert_eq!(Bucket::parse("week").unwrap(), Bucket::Week);
        assert_eq!(Bucket::parse("month").unwrap(), Bucket::Month);
        assert!(Bucket::parse("year").is_err());
    }

    #[test]
    fn bucket_labels_and_bounds() {
        let t = DateTime::parse_from_rfc3339("2026-08-26T15:04:05Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(Bucket::Day.label(t), "2026-08-26");
        assert_eq!(Bucket::Month.label(t), "2026-08");
        // 2026-08-26 is a Wednesday; its ISO week starts Monday 2026-08-24.
        assert_eq!(Bucket::Week.label(t), "2026-08-24");

        assert_eq!(
            Bucket::Day.end_of("2026-08-26").to_rfc3339(),
            "2026-08-27T00:00:00+00:00"
        );
        assert_eq!(
            Bucket::Week.end_of("2026-08-24").to_rfc3339(),
            "2026-08-31T00:00:00+00:00"
        );
        // Month rollover across the year boundary.
        assert_eq!(
            Bucket::Month.end_of("2026-12").to_rfc3339(),
            "2027-01-01T00:00:00+00:00"
        );
    }

    fn obj_at(name: &str, when: &str) -> KirObject {
        let mut o = KirObject::new(name, ObjectKind::Table);
        o.created_at = DateTime::parse_from_rfc3339(when)
            .unwrap()
            .with_timezone(&Utc);
        o
    }

    #[test]
    fn timeline_is_cumulative_per_bucket_and_since_only_trims_display() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        {
            let store = super::super::store::open_store(&config, dir.path()).unwrap();
            store
                .append_object(&obj_at("a", "2026-08-10T09:00:00Z"))
                .unwrap();
            store
                .append_object(&obj_at("b", "2026-08-10T18:00:00Z"))
                .unwrap();
            store
                .append_object(&obj_at("c", "2026-09-02T12:00:00Z"))
                .unwrap();
        }
        let store = super::super::store::open_store_read_only(&config, dir.path()).unwrap();

        let daily = build_timeline(store.as_ref(), Bucket::Day, None).unwrap();
        assert_eq!(daily.bucket, "day");
        assert_eq!(daily.points.len(), 2);
        assert_eq!(daily.points[0].t, "2026-08-10");
        assert_eq!(daily.points[0].objects, 2);
        // The second bucket's total includes everything before it — cumulative, not per-bucket.
        assert_eq!(daily.points[1].t, "2026-09-02");
        assert_eq!(daily.points[1].objects, 3);

        // `--since` hides the August bucket but the September total still counts all 3.
        let since = DateTime::parse_from_rfc3339("2026-09-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let trimmed = build_timeline(store.as_ref(), Bucket::Day, Some(since)).unwrap();
        assert_eq!(trimmed.points.len(), 1);
        assert_eq!(trimmed.points[0].objects, 3);
    }

    #[test]
    fn timeline_on_an_empty_store_is_no_points() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let store = super::super::store::open_store_read_only(&config, dir.path()).unwrap();
        let tl = build_timeline(store.as_ref(), Bucket::Month, None).unwrap();
        assert!(tl.points.is_empty());
    }
}
