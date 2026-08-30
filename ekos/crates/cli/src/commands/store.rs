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
use ekos_ledger::{
    FactLedger, KnowledgeStore, Ledger, PartitionDimension, PartitionKey, PartitionedLedger,
    TimeBucket,
};
use std::path::{Path, PathBuf};

/// True when `[storage.distributed]` points reads at a Distributed-mode cluster (RFC 0113 B4).
/// Takes precedence over every local backend — the workspace holds no data of its own.
pub fn uses_distributed(config: &EkosConfig) -> bool {
    config.storage.distributed.is_enabled()
}

/// Build the [`DistributedLedger`] gateway from `[storage.distributed]`.
fn build_distributed(config: &EkosConfig) -> Result<ekos_distributed::DistributedLedger> {
    let d = &config.storage.distributed;
    let coordinator = d
        .coordinator
        .clone()
        .ok_or_else(|| anyhow::anyhow!("[storage.distributed] coordinator is not set"))?;
    if d.query_workers.is_empty() {
        anyhow::bail!("[storage.distributed] needs at least one query-workers entry");
    }
    ekos_distributed::DistributedLedger::open(coordinator, d.query_workers.clone())
        .map_err(|e| anyhow::anyhow!("cannot reach the distributed cluster: {e}"))
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

/// Build a [`PartitionedLedger`] from `[storage.partition]` config. `compiler-core` holds the
/// dimension/bucket as strings (it can't depend on `ekos-ledger`); the string→enum translation is
/// this layer's job (the split CLAUDE.md documents for `ArchitectureConfidence`).
pub(crate) fn build_partitioned(
    config: &EkosConfig,
    cwd: &Path,
    read_only: bool,
) -> Result<PartitionedLedger> {
    let root = partitioned_root(config, cwd);
    let p = &config.storage.partition;
    let dim_str = p.dimension.as_deref().unwrap_or("entity-kind");
    let dimension = PartitionDimension::parse(dim_str)
        .ok_or_else(|| anyhow::anyhow!("unknown [storage.partition] dimension: {dim_str:?}"))?;
    if dimension != PartitionDimension::EntityKind {
        anyhow::bail!(
            "[storage.partition] dimension {dim_str:?} needs a source resolver that open_store \
             cannot provide yet (KirObject has no source field) — use \"entity-kind\""
        );
    }
    let bucket_str = p.default_time_bucket();
    let time_bucket = TimeBucket::parse(bucket_str).ok_or_else(|| {
        anyhow::anyhow!("unknown [storage.partition] time-bucket: {bucket_str:?}")
    })?;

    let mut ledger = PartitionedLedger::new(
        &root,
        dimension,
        time_bucket,
        default_root_for(root.clone()),
    )
    .map_err(|e| anyhow::anyhow!("cannot open partitioned ledger at {}: {e}", root.display()))?;

    if let Some(url) = p.segment_backend_url.clone() {
        ledger = with_segment_backend_url(ledger, url)?;
    }

    Ok(if read_only {
        ledger.read_only()
    } else {
        ledger
    })
}

/// Wire `[storage.partition] segment-backend-url` — each partition's sealed segments publish to /
/// fetch from `<url>/<partition-id>`, its local root stays the segment cache. Object storage
/// support is behind `--features distributed`.
#[cfg(feature = "distributed")]
fn with_segment_backend_url(ledger: PartitionedLedger, url: String) -> Result<PartitionedLedger> {
    use std::sync::{Arc, Mutex};
    // Validate the URL once, up front.
    ekos_segment_backend::ObjectStoreBackend::from_url(&url, std::env::temp_dir())
        .map_err(|e| anyhow::anyhow!("[storage.partition] segment-backend-url {url:?}: {e}"))?;

    let cache: Arc<Mutex<std::collections::HashMap<String, Arc<dyn ekos_ledger::SegmentBackend>>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    Ok(ledger.with_segment_backend(move |key, local_root| {
        let pid = format!("{}/{}", key.dimension_value, key.time_bucket);
        let mut map = cache.lock().unwrap();
        if let Some(b) = map.get(&pid) {
            return Some(b.clone());
        }
        // Prefix the store keys by partition id; download into the partition's own local root.
        let per_partition_url = format!("{}/{pid}", url.trim_end_matches('/'));
        match ekos_segment_backend::ObjectStoreBackend::from_url(&per_partition_url, local_root) {
            Ok(b) => {
                let b: Arc<dyn ekos_ledger::SegmentBackend> = Arc::new(b);
                map.insert(pid, b.clone());
                Some(b)
            }
            Err(e) => {
                tracing::error!(%per_partition_url, %e, "cannot build the partition segment backend");
                None
            }
        }
    }))
}

#[cfg(not(feature = "distributed"))]
fn with_segment_backend_url(_ledger: PartitionedLedger, url: String) -> Result<PartitionedLedger> {
    anyhow::bail!(
        "[storage.partition] segment-backend-url = {url:?} needs an `ekos` built with \
         `--features distributed`"
    )
}

/// Open the workspace's knowledge store with backend auto-detection.
pub fn open_store(config: &EkosConfig, cwd: &Path) -> Result<Box<dyn KnowledgeStore>> {
    if uses_distributed(config) {
        return Ok(Box::new(build_distributed(config)?));
    }
    if uses_partitioned(config, cwd) {
        return Ok(Box::new(build_partitioned(config, cwd, false)?));
    }
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

/// Open the workspace's knowledge store for reads only (RFC 0097).
///
/// For the fact engine, this is [`FactLedger::open_read_only`] — it never
/// acquires tantivy's exclusive `IndexWriter` lock, so a long-lived caller
/// (e.g. `ekos mcp serve`, reusing one open handle across many calls) never
/// blocks a concurrent real writer (`ekos build`/`commit` in a separate
/// process) from opening the same store. For the SQLite backend, this is
/// just [`Ledger::open`] — SQLite has no equivalent whole-handle-lifetime
/// exclusive lock, so there's nothing special to do; every write already
/// goes through SQLite's own file-level locking regardless of which
/// `Ledger` opened it.
///
/// A genuinely fresh workspace (neither backend ever written to) is *not*
/// an error — [`open_store`] itself silently creates an empty store for
/// this exact case, and every MCP tool is expected to keep working
/// gracefully against it (empty results, not a "run `ekos build`" error;
/// several real tests pin this). To preserve that without ever caching a
/// writable handle, this bootstraps the empty on-disk store via a
/// short-lived writable open that's opened and immediately dropped —
/// never returned, never held past this function call — then does the
/// real read-only open, which now succeeds since the store exists. The
/// brief writable-open window is no new race: any two processes calling
/// `open_store` on a truly fresh workspace at the same moment already had
/// this exact narrow bootstrap race before this RFC.
pub fn open_store_read_only(config: &EkosConfig, cwd: &Path) -> Result<Box<dyn KnowledgeStore>> {
    if uses_distributed(config) {
        return Ok(Box::new(build_distributed(config)?));
    }
    if uses_partitioned(config, cwd) {
        // `PartitionedLedger::new` is safe on a fresh dir (empty catalog → empty reads); the
        // `.read_only()` handle then opens every partition via `FactLedger::open_read_only`.
        return Ok(Box::new(build_partitioned(config, cwd, true)?));
    }
    if uses_fact_engine(config, cwd) {
        let dir = facts_dir(config, cwd);
        return Ok(Box::new(FactLedger::open_read_only(&dir).map_err(|e| {
            anyhow::anyhow!("cannot open fact ledger at {}: {e}", dir.display())
        })?));
    }

    let sqlite_path = config.ledger_path(cwd);
    if sqlite_path.exists() {
        return Ok(Box::new(Ledger::open(&sqlite_path).map_err(|e| {
            anyhow::anyhow!("cannot open ledger at {}: {e}", sqlite_path.display())
        })?));
    }

    let dir = facts_dir(config, cwd);
    drop(
        FactLedger::open(&dir)
            .map_err(|e| anyhow::anyhow!("cannot create fact ledger at {}: {e}", dir.display()))?,
    );
    Ok(Box::new(FactLedger::open_read_only(&dir).map_err(|e| {
        anyhow::anyhow!("cannot open fact ledger at {}: {e}", dir.display())
    })?))
}

/// Human-readable location of whatever backend [`open_store`] would open right now — mirrors its
/// exact three-way logic so this stays accurate even before a fresh workspace's first
/// `open_store` call has run.
pub fn store_display(config: &EkosConfig, cwd: &Path) -> String {
    if uses_distributed(config) {
        return format!(
            "distributed cluster @ {}",
            config
                .storage
                .distributed
                .coordinator
                .as_deref()
                .unwrap_or("?")
        );
    }
    if uses_partitioned(config, cwd) {
        partitioned_root(config, cwd).display().to_string()
    } else if uses_fact_engine(config, cwd) || !config.ledger_path(cwd).exists() {
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

    #[test]
    fn open_store_read_only_bootstraps_an_empty_store_on_a_never_built_workspace() {
        // Matches open_store's own existing contract: a genuinely fresh
        // workspace is not an error — every MCP tool must keep working
        // gracefully against it (empty results), a real behavior several
        // `crates/cli/src/commands/mcp.rs` tests pin.
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        std::fs::create_dir_all(config.ledger_dir(dir.path())).unwrap();

        let store = open_store_read_only(&config, dir.path())
            .expect("a fresh workspace must bootstrap an empty store, not error");
        assert_eq!(store.object_count().unwrap(), 0);
        // manifest.json itself is written lazily by the fact engine only
        // after a real write happens (see uses_fact_engine's own doc
        // comment and fresh_workspace_defaults_to_the_fact_engine above) —
        // the bootstrap open+drop never writes anything, so `segments/` is
        // the real, always-created signal a store now exists on disk.
        assert!(
            facts_dir(&config, dir.path()).join("segments").exists(),
            "the bootstrap must have created a real on-disk store"
        );
    }

    #[test]
    fn open_store_read_only_reads_a_fact_engine_workspace_built_by_open_store() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        std::fs::create_dir_all(config.ledger_dir(dir.path())).unwrap();
        {
            let store = open_store(&config, dir.path()).unwrap();
            store
                .append_object(&ekos_kir::KirObject::new(
                    "orders",
                    ekos_kir::ObjectKind::Table,
                ))
                .unwrap();
        }

        let reader = open_store_read_only(&config, dir.path()).unwrap();
        assert_eq!(reader.object_count().unwrap(), 1);
    }

    #[test]
    fn open_store_read_only_reads_a_pre_existing_sqlite_workspace() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        std::fs::create_dir_all(config.ledger_dir(dir.path())).unwrap();
        {
            let sqlite = Ledger::open(&config.ledger_path(dir.path())).unwrap();
            sqlite
                .append_object(&ekos_kir::KirObject::new(
                    "orders",
                    ekos_kir::ObjectKind::Table,
                ))
                .unwrap();
        }

        let reader = open_store_read_only(&config, dir.path()).unwrap();
        assert_eq!(reader.object_count().unwrap(), 1);
    }

    /// RFC 0111 groundwork: two `ekos.toml`s each naming a different `[storage]
    /// active-container` must produce two genuinely independent stores on disk — the actual
    /// "different folders simulate different storage containers" property this exists for, proven
    /// end-to-end through `open_store` rather than just at the config-parsing level
    /// (`compiler-core`'s own `config.rs` tests already cover path resolution in isolation).
    #[test]
    fn different_storage_containers_are_genuinely_independent_stores() {
        let container_a = tempdir().unwrap();
        let container_b = tempdir().unwrap();
        // A shared "workspace" cwd — irrelevant to where data actually lands once a container is
        // active, which is exactly the property under test.
        let shared_cwd = tempdir().unwrap();

        let toml_a = format!(
            "[storage]\nactive-container = \"a\"\n[[storage.containers]]\nname = \"a\"\npath = \"{}\"\n",
            container_a.path().display()
        );
        let toml_b = format!(
            "[storage]\nactive-container = \"b\"\n[[storage.containers]]\nname = \"b\"\npath = \"{}\"\n",
            container_b.path().display()
        );
        let config_a: EkosConfig = toml::from_str(&toml_a).unwrap();
        let config_b: EkosConfig = toml::from_str(&toml_b).unwrap();

        {
            let store_a = open_store(&config_a, shared_cwd.path()).unwrap();
            store_a
                .append_object(&ekos_kir::KirObject::new(
                    "orders",
                    ekos_kir::ObjectKind::Table,
                ))
                .unwrap();
        }
        {
            let store_b = open_store(&config_b, shared_cwd.path()).unwrap();
            store_b
                .append_object(&ekos_kir::KirObject::new(
                    "customers",
                    ekos_kir::ObjectKind::Table,
                ))
                .unwrap();
        }

        // Each container's folder holds its own real on-disk store...
        assert!(container_a.path().join("ledger/facts/segments").exists());
        assert!(container_b.path().join("ledger/facts/segments").exists());
        // ...and nothing was ever written under the shared cwd's own .ekos at all.
        assert!(!shared_cwd.path().join(".ekos").exists());

        let reader_a = open_store_read_only(&config_a, shared_cwd.path()).unwrap();
        let reader_b = open_store_read_only(&config_b, shared_cwd.path()).unwrap();
        assert_eq!(reader_a.object_count().unwrap(), 1);
        assert_eq!(reader_b.object_count().unwrap(), 1);
        assert_eq!(reader_a.all_objects().unwrap()[0].name, "orders");
        assert_eq!(reader_b.all_objects().unwrap()[0].name, "customers");
    }

    /// RFC 0111 Phase A: a fresh workspace with `[storage.partition]` enabled is served by
    /// `PartitionedLedger` through `open_store`, and reads back through `open_store_read_only`.
    #[test]
    fn partitioned_workspace_round_trips_through_open_store() {
        let dir = tempdir().unwrap();
        let config: EkosConfig = toml::from_str(
            "[storage.partition]\ndimension = \"entity-kind\"\ntime-bucket = \"monthly\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(config.ledger_dir(dir.path())).unwrap();

        assert!(uses_partitioned(&config, dir.path()));
        {
            let store = open_store(&config, dir.path()).unwrap();
            store
                .append_object(&ekos_kir::KirObject::new(
                    "orders",
                    ekos_kir::ObjectKind::Table,
                ))
                .unwrap();
            store
                .append_relationship(&ekos_kir::KirRelationship::new(
                    ekos_kir::RelationshipKind::DependsOn,
                    ekos_kir::KirId::new(),
                    ekos_kir::KirId::new(),
                ))
                .unwrap();
        }
        assert!(
            partitioned_root(&config, dir.path())
                .join("catalog.json")
                .exists()
        );

        let reader = open_store_read_only(&config, dir.path()).unwrap();
        assert_eq!(reader.object_count().unwrap(), 1);
        assert_eq!(reader.relationship_count().unwrap(), 1);
        assert_eq!(reader.all_objects().unwrap()[0].name, "orders");
        assert_eq!(
            store_display(&config, dir.path()),
            partitioned_root(&config, dir.path()).display().to_string()
        );
    }

    /// RFC 0113 B4: `[storage.partition] segment-backend-url` makes `open_store` build an
    /// `ObjectStoreBackend` per partition (validated up front) without disturbing normal
    /// read/write; a bogus URL is a clear config error. (That sealed segments really live on the
    /// backend is proven at the `FactLedger` level —
    /// `fact_ledger::tests::sealed_segments_are_served_from_the_backend_not_local_disk`.)
    #[cfg(feature = "distributed")]
    #[test]
    fn segment_backend_url_wires_partitions_without_disturbing_reads() {
        let segstore = tempdir().unwrap();

        let bad = tempdir().unwrap();
        std::fs::create_dir_all(EkosConfig::default().ledger_dir(bad.path())).unwrap();
        let bad_cfg: EkosConfig = toml::from_str(
            "[storage.partition]\ndimension = \"entity-kind\"\nsegment-backend-url = \"http://nope\"\n",
        )
        .unwrap();
        let err = match open_store(&bad_cfg, bad.path()) {
            Ok(_) => panic!("a bogus segment-backend-url must be rejected"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("segment-backend-url"), "{err}");

        let dir = tempdir().unwrap();
        std::fs::create_dir_all(EkosConfig::default().ledger_dir(dir.path())).unwrap();
        let config: EkosConfig = toml::from_str(&format!(
            "[storage.partition]\ndimension = \"entity-kind\"\nsegment-backend-url = \"file://{}\"\n",
            segstore.path().display()
        ))
        .unwrap();
        {
            let store = open_store(&config, dir.path()).unwrap();
            store
                .append_object(&ekos_kir::KirObject::new(
                    "orders",
                    ekos_kir::ObjectKind::Table,
                ))
                .unwrap();
        }
        let reader = open_store_read_only(&config, dir.path()).unwrap();
        assert_eq!(reader.object_count().unwrap(), 1);
        assert_eq!(reader.all_objects().unwrap()[0].name, "orders");
    }

    /// An existing fact-engine workspace is **not** switched to partitioned just because the
    /// config flag is set — same guarantee the SQLite→fact switch has.
    #[test]
    fn existing_fact_workspace_is_not_switched_to_partitioned() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(EkosConfig::default().ledger_dir(dir.path())).unwrap();
        // Build a real fact-engine workspace first, with no partition config.
        {
            let store = open_store(&EkosConfig::default(), dir.path()).unwrap();
            store
                .append_object(&ekos_kir::KirObject::new(
                    "orders",
                    ekos_kir::ObjectKind::Table,
                ))
                .unwrap();
        }
        // Now turn the partition flag on.
        let config: EkosConfig =
            toml::from_str("[storage.partition]\ndimension = \"entity-kind\"\n").unwrap();
        assert!(!uses_partitioned(&config, dir.path()));
        assert!(uses_fact_engine(&config, dir.path()));
        let reader = open_store_read_only(&config, dir.path()).unwrap();
        assert_eq!(reader.object_count().unwrap(), 1);
    }
}
