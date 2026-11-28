//! Which store a workspace uses (RFC 0016 / 0111 / 0113), decided in one place.
//!
//! Moved from the CLI's `commands/store.rs` (RFC 0269 phase 1): the Python package's engine must
//! pick the same backend as `ekos` does without linking the CLI. This module only *decides* and
//! builds the partitioned handle; opening for writes, the read scope and the distributed gateway
//! stay with the caller.

use ekos_compiler_core::EkosConfig;
use ekos_ledger::{PartitionDimension, PartitionKey, PartitionedLedger, TimeBucket};
use std::path::{Path, PathBuf};

/// The store behind a workspace, in the order the checks are made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// `[storage.distributed]`: reads go to a cluster; the workspace holds no data.
    Distributed,
    /// RFC 0111: catalog, index and partitions under this directory.
    Partitioned(PathBuf),
    /// RFC 0016: the fact engine's store in this directory.
    Facts(PathBuf),
    /// A SQLite ledger from before the fact engine became the default.
    Sqlite(PathBuf),
    /// Nothing has been written yet. A writable open creates a fact-engine store in this
    /// directory.
    Fresh(PathBuf),
}

/// The store `config` and `cwd` name. Every opener — the CLI's writable and read-only opens, its
/// status line, and the Python engine — goes through this, so they cannot disagree.
pub fn detect(config: &EkosConfig, cwd: &Path) -> Backend {
    if uses_distributed(config) {
        Backend::Distributed
    } else if uses_partitioned(config, cwd) {
        Backend::Partitioned(partitioned_root(config, cwd))
    } else if uses_fact_engine(config, cwd) {
        Backend::Facts(facts_dir(config, cwd))
    } else if config.ledger_path(cwd).exists() {
        Backend::Sqlite(config.ledger_path(cwd))
    } else {
        Backend::Fresh(facts_dir(config, cwd))
    }
}

/// True when `[storage.distributed]` points reads at a Distributed-mode cluster (RFC 0113 B4).
/// Takes precedence over every local backend — the workspace holds no data of its own.
pub fn uses_distributed(config: &EkosConfig) -> bool {
    config.storage.distributed.is_enabled()
}

/// Where a fact-engine-backed workspace's store lives (migrated or newly created).
pub fn facts_dir(config: &EkosConfig, cwd: &Path) -> PathBuf {
    config.ledger_dir(cwd).join("facts")
}

/// Where a partitioned workspace's catalog + index + partitions live (RFC 0111 Phase A).
pub fn partitioned_root(config: &EkosConfig, cwd: &Path) -> PathBuf {
    config.ledger_dir(cwd).join("partitioned")
}

/// True when this workspace already has a real fact store on disk — either migrated via
/// `ekos ledger migrate --v3`, or previously auto-created as a fresh workspace's default. Doesn't
/// distinguish *how* it got there, only that it's the active backend now.
pub fn uses_fact_engine(config: &EkosConfig, cwd: &Path) -> bool {
    facts_dir(config, cwd).join("manifest.json").exists()
}

/// True when this workspace is served by [`PartitionedLedger`] — either it already has a
/// `partitioned/catalog.json`, or `[storage.partition]` is enabled **and** neither of the other
/// two backends has ever been written to (a genuinely fresh workspace opting in, mirroring the
/// fact-engine default-switch rule — an existing SQLite/fact workspace is never implicitly
/// switched).
pub fn uses_partitioned(config: &EkosConfig, cwd: &Path) -> bool {
    if partitioned_root(config, cwd).join("catalog.json").exists() {
        return true;
    }
    config.storage.partition.is_enabled()
        && !uses_fact_engine(config, cwd)
        && !config.ledger_path(cwd).exists()
}

/// The default on-disk layout for a partition: `<partitioned>/p/<dimension_value>/<time_bucket>`,
/// with `:` and the unit separator (which appear in `"rel:*"` and `Composite` values) sanitized.
fn default_root_for(root: PathBuf) -> impl Fn(&PartitionKey) -> PathBuf + Send + Sync + 'static {
    move |key: &PartitionKey| {
        root.join("p")
            .join(key.dimension_value.replace([':', '\u{1f}'], "_"))
            .join(&key.time_bucket)
    }
}

/// Build a [`PartitionedLedger`] from `[storage.partition]`. `compiler-core` holds the
/// dimension and bucket as strings (it cannot depend on `ekos-ledger`); this is where they become
/// enums. The handle is writable; a reader calls `.read_only()` on it. Safe on a fresh directory
/// (an empty catalog reads as empty).
pub fn open_partitioned(config: &EkosConfig, cwd: &Path) -> Result<PartitionedLedger, String> {
    let root = partitioned_root(config, cwd);
    let p = &config.storage.partition;
    let dim_str = p.dimension.as_deref().unwrap_or("entity-kind");
    let dimension = PartitionDimension::parse(dim_str)
        .ok_or_else(|| format!("unknown [storage.partition] dimension: {dim_str:?}"))?;
    if dimension != PartitionDimension::EntityKind {
        return Err(format!(
            "[storage.partition] dimension {dim_str:?} needs a source resolver that open_store \
             cannot provide yet (KirObject has no source field) — use \"entity-kind\""
        ));
    }
    let bucket_str = p.default_time_bucket();
    let time_bucket = TimeBucket::parse(bucket_str)
        .ok_or_else(|| format!("unknown [storage.partition] time-bucket: {bucket_str:?}"))?;

    PartitionedLedger::new(
        &root,
        dimension,
        time_bucket,
        default_root_for(root.clone()),
    )
    .map_err(|e| format!("cannot open partitioned ledger at {}: {e}", root.display()))
}
