//! RFC 0111 Phase A: real, tested partitioning and fan-out for `KirObject`s, keyed by
//! `PartitionDimension::EntityKind` + a configurable time bucket, with a **persisted catalog and
//! entity index** so partitions — and each entity's partition set — survive process restarts.
//!
//! **Scope, stated precisely rather than overclaimed:** only object reads/writes are implemented
//! (`append_object`, `get_object`, `object_history`, `objects_in_kind`, `all_objects`,
//! `object_count`). Relationships/events/evidence and the rest of `KnowledgeStore`'s surface
//! (`diff`, `vacuum_into`, full-text search, …) are out of scope for this slice —
//! `PartitionedLedger` is **not** a `KnowledgeStore` yet and cannot be opened through
//! `open_store`. All three `PartitionDimension`s route; `SourceScope` and `Composite` need a
//! caller-supplied source resolver ([`PartitionedLedger::with_source_resolver`]) because
//! `KirObject` has no explicit source field yet — a `None` from the resolver under a source-based
//! dimension is a [`PartitionError::UnresolvedSource`], never a silent misroute.
//!
//! Each partition is an ordinary, unmodified [`FactLedger`] — this module only adds routing and
//! fan-out above it, exactly RFC 0111's own "no format/invariant change, purely an access-path
//! layer" principle. A partition's root directory is resolved by a caller-supplied closure, which
//! is where this project's `[storage]` container config (RFC 0111 groundwork, `compiler-core`)
//! plugs in — different partitions can be routed into different configured container folders,
//! which is the concrete mechanism for testing "distributed storage" locally with plain folders.
//!
//! ## Persisted catalog + run-file index (RFC 0111 §5, Architecture Review + 2026-08-29 amendment)
//!
//! [`PartitionedLedger::new`] reads (and later writes) two things under `catalog_root`:
//!
//! - **`catalog.json`** — a small list of every partition ever created
//!   (`(dimension_value, time_bucket) → on-disk root` + [`Tier`]), one entry per partition, **not**
//!   per entity. Rewritten atomically (temp + rename) only when a partition is registered or
//!   changes tier.
//! - **`index/run-*.jsonl`** — an append-only, AEVT-style run-file index whose lines are
//!   `{k, id, p}` (`k` ∈ obj / rel / endpoint — amendment §2). `new` folds every run into three
//!   in-memory `id → partitions` maps, marking each loaded entry authoritative, so after a reopen
//!   `get_object`/`object_history`, `get_relationship`/`relationship_history`, and
//!   `relationships_for` all resolve with **zero partition scans**. A line is appended
//!   (buffered→OS, not fsync'd) only the first time an id lands in a partition; most appends add
//!   none. When the run count crosses [`COMPACT_AT`], `new` merges every run into one fresh sorted
//!   run and deletes the rest (`merge_runs`-style).
//!
//! A brand-new `PartitionedLedger` opened at the same `catalog_root` therefore sees every
//! partition *and* every id's partition set immediately — pruned/broad reads and point/history
//! reads all work with no prior write this process.
//!
//! ## Known limits this slice does not close
//!
//! - **Crash consistency of the index is best-effort.** Run lines are flushed to the OS, not
//!   fsync'd — same durability profile as the `FactLedger` segment they mirror (RFC 0104). An id
//!   *absent* from the loaded index is re-derived by a one-time catalog scan on first read (and
//!   the discovered pairs appended, self-healing). An id whose *partition-crossing* line was lost
//!   to an OS/power crash needs [`PartitionedLedger::rebuild_entity_index`] (the `ekos ledger
//!   repair`-style full re-derive — rebuilds obj/rel/endpoint alike).
//! - **Cold tiering is a policy flag, not yet a format change.** [`PartitionedLedger::mark_cold_before`]
//!   demotes aged partitions ([`Tier::Cold`]) — evicting the open handle and marking them "eligible
//!   to relocate to cheaper storage" — and any read promotes one back to hot. The RFC §3
//!   search-index drop + zstd recompression need `FactLedger` support and land with the
//!   `KnowledgeStore`/`SegmentBackend` work. The **`SegmentBackend` seam (§4)** itself (object
//!   storage) is Phase B.
//! - **`Composite` scoped queries** match an exact `"<source>\u{1f}<kind>"` value — prefix scoping
//!   (all kinds for one source, or vice versa) is a later refinement.
//!
//! ## Concurrency (RFC 0111 §1: "N partitions admit N concurrent writers")
//!
//! Each open partition is held as an `Arc<FactLedger>`. The maps of open partitions / the catalog
//! are each guarded by a `Mutex` held only long enough to look up (or register) and clone one
//! `Arc` — never for the duration of a read or write, and never two at once (no lock nesting).
//! `FactLedger` is itself internally synchronized (`Mutex<Inner>`), so two threads writing to
//! **different** partitions genuinely proceed in parallel, while two threads writing to the
//! **same** partition serialize on that partition's own lock — the single-writer-per-partition
//! invariant RFC 0104's `write.lock` also enforces cross-process.

use crate::FactLedger;
use crate::LedgerError;
use chrono::{DateTime, Utc};
use ekos_kir::{KirId, KirObject, KirRelationship};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Merge every index run into one once this many accumulate (`merge_runs`-style bound).
pub const COMPACT_AT: usize = 16;

/// The loaded in-memory index maps plus the on-disk run state:
/// `(maps, next unused run number, run files read)`.
type LoadedIndex = (IndexMaps, u32, Vec<PathBuf>);

/// Resolves a *new* partition's on-disk root from its key (container-config routing hook).
type RootResolver = Box<dyn Fn(&PartitionKey) -> PathBuf + Send + Sync>;
/// Resolves a `KirObject`'s originating source/connector for `SourceScope`/`Composite` routing.
type SourceResolver = Box<dyn Fn(&KirObject) -> Option<String> + Send + Sync>;

#[derive(Debug, Error)]
pub enum PartitionError {
    #[error("ledger error in partition {key:?}: {source}")]
    Ledger {
        key: PartitionKey,
        #[source]
        source: LedgerError,
    },
    #[error("partition catalog I/O error at {path}: {source}")]
    Catalog {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("partition catalog at {path} is corrupt: {source}")]
    CatalogParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "SourceScope/Composite routing needs a source for entity {entity}, but the source \
         resolver returned None — set one with PartitionedLedger::with_source_resolver"
    )]
    UnresolvedSource { entity: KirId },
    #[error(
        "this ledger's partitions were created with {field} = {stored:?}, but it is being opened \
         with {requested:?} — routing/tiering config cannot change after partitions exist"
    )]
    DimensionMismatch {
        field: &'static str,
        stored: String,
        requested: String,
    },
}

/// RFC 0111 §1's routing dimension. All three variants route; `SourceScope` and `Composite`
/// require a source resolver ([`PartitionedLedger::with_source_resolver`]) since a `KirObject`
/// carries no explicit source field yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartitionDimension {
    /// Partition by `ObjectKind` — e.g. `Table`, `File`, `Custom("Risk")`.
    EntityKind,
    /// Partition by the object's originating source/connector — e.g. `sql`, `git`,
    /// `discord:#governance` — as returned by the source resolver.
    SourceScope,
    /// Partition by `source` + `kind` together (`"<source>\u{1f}<kind>"`); more partitions, so a
    /// scoped query prunes to an exact composite value (prefix scoping is a later refinement).
    Composite,
}

impl PartitionDimension {
    pub fn as_str(&self) -> &'static str {
        match self {
            PartitionDimension::EntityKind => "entity-kind",
            PartitionDimension::SourceScope => "source-scope",
            PartitionDimension::Composite => "composite",
        }
    }

    /// Parse the `ekos.toml` `[storage.partition] dimension` string.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "entity-kind" => Some(PartitionDimension::EntityKind),
            "source-scope" => Some(PartitionDimension::SourceScope),
            "composite" => Some(PartitionDimension::Composite),
            _ => None,
        }
    }
}

/// RFC 0111 §1's time-bucket granularity. Labels are chosen so that **lexical order equals
/// chronological order** — [`PartitionKey`]'s derived `Ord` relies on this to merge partitions in
/// the correct order in [`PartitionedLedger::object_history`] without a separate timestamp
/// comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TimeBucket {
    Daily,
    Weekly,
    #[default]
    Monthly,
}

impl TimeBucket {
    /// The bucket label for a timestamp — the string that becomes [`PartitionKey::time_bucket`].
    pub fn label(&self, at: DateTime<Utc>) -> String {
        match self {
            // ISO-8601 forms, all lexically == chronologically ordered.
            TimeBucket::Daily => at.format("%Y-%m-%d").to_string(),
            TimeBucket::Weekly => at.format("%G-W%V").to_string(),
            TimeBucket::Monthly => at.format("%Y-%m").to_string(),
        }
    }

    /// Parse the `ekos.toml` `[storage.partition] time-bucket` string. `None` for an unknown value
    /// (caller decides whether to warn-and-default or error).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "daily" => Some(TimeBucket::Daily),
            "weekly" => Some(TimeBucket::Weekly),
            "monthly" => Some(TimeBucket::Monthly),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TimeBucket::Daily => "daily",
            TimeBucket::Weekly => "weekly",
            TimeBucket::Monthly => "monthly",
        }
    }
}

/// `(time_bucket, dimension_value)` — field order matters: it makes the derived `Ord` sort
/// chronologically first, which [`PartitionedLedger::object_history`] relies on to merge partitions
/// in the correct order without needing a separate timestamp comparison.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PartitionKey {
    /// e.g. `"2026-08"` (monthly), `"2026-W35"` (weekly), `"2026-08-27"` (daily).
    pub time_bucket: String,
    /// e.g. `"Table"`, `"File"` (an `ObjectKind`'s `Display` output) for `EntityKind` routing.
    pub dimension_value: String,
}

/// RFC 0111 §3 hot/cold tier. **Hot**: kept in the open-handle cache, eligible for indexing, lives
/// on local disk. **Cold**: an aged-out, sealed partition — its handle is evicted and it is
/// flagged "eligible to relocate to cheaper storage"; reads still work (they open it transiently
/// and promote it back to Hot). Search-index drop + recompression (RFC §3) need `FactLedger`
/// support and land with the `KnowledgeStore`/`SegmentBackend` work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    #[default]
    Hot,
    Cold,
}

impl Tier {
    fn is_hot(&self) -> bool {
        matches!(self, Tier::Hot)
    }
}

/// One persisted catalog row: a partition's key, where its [`FactLedger`] lives on disk, and its
/// [`Tier`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PartitionEntry {
    pub key: PartitionKey,
    pub root: PathBuf,
    #[serde(default, skip_serializing_if = "Tier::is_hot")]
    pub tier: Tier,
}

/// RFC 0111 §5's `PartitionCatalog`, persisted as `<catalog_root>/catalog.json`. One entry per
/// partition (never per entity), kept sorted for a deterministic file. `dimension`/`time_bucket`
/// are recorded on first write and then frozen — reopening with a different value is an error
/// (`DimensionMismatch`), since the on-disk partition names encode both.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PartitionCatalog {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimension: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_bucket: Option<String>,
    pub partitions: Vec<PartitionEntry>,
}

/// `id → partitions` cache entry. `complete` is true when the set is known to be the full
/// membership — either loaded from the persisted index on open, or filled in by a one-time catalog
/// scan (see the module doc's "Known limits").
#[derive(Default)]
struct Sites {
    partitions: BTreeSet<PartitionKey>,
    complete: bool,
}

/// What a run-file line's `id` refers to (RFC 0111 amendment 2026-08-29 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum IndexKind {
    /// object entity id → an object partition it has a version in
    #[default]
    Obj,
    /// relationship id → the relationship partition it lives in
    Rel,
    /// an endpoint entity id (`from`/`to`) → a relationship partition it participates in
    Endpoint,
}

/// One line of an `index/run-*.jsonl` file: `{k, id, p}`. `k` is omitted for the common `obj`
/// case (serde default), keeping object lines compact.
#[derive(Serialize, Deserialize)]
struct IndexLine {
    #[serde(default, skip_serializing_if = "is_obj_kind")]
    k: IndexKind,
    id: KirId,
    p: PartitionKey,
}

fn is_obj_kind(k: &IndexKind) -> bool {
    matches!(k, IndexKind::Obj)
}

/// A relationship partition's `dimension_value` is `"rel:<RelationshipKind>"` (RFC 0111 amendment
/// §1) — always disjoint from object partitions.
fn is_relationship_partition(key: &PartitionKey) -> bool {
    key.dimension_value.starts_with("rel:")
}

/// Wrap a `LedgerError` from a partition read where the exact key isn't threaded through.
fn ledger_err(source: LedgerError) -> PartitionError {
    PartitionError::Ledger {
        key: PartitionKey {
            time_bucket: String::new(),
            dimension_value: String::new(),
        },
        source,
    }
}

/// Deduplicate current-state relationships fanned in from multiple partitions — one row per id,
/// last (newest partition) wins. Input must already be ordered oldest→newest partition.
fn dedup_current_rels(rels: Vec<KirRelationship>) -> Vec<KirRelationship> {
    let mut by_id: HashMap<KirId, KirRelationship> = HashMap::new();
    for rel in rels {
        by_id.insert(rel.id, rel);
    }
    by_id.into_values().collect()
}

/// Insert `key` into `map[id]`'s partition set, creating a `complete` entry if the id is new this
/// process (absent from the loaded index → its write-time partition is its whole membership).
/// Returns whether this was a new `(id, key)` pair.
fn insert_site(map: &Mutex<HashMap<KirId, Sites>>, id: KirId, key: &PartitionKey) -> bool {
    map.lock()
        .unwrap()
        .entry(id)
        .or_insert_with(|| Sites {
            partitions: BTreeSet::new(),
            complete: true,
        })
        .partitions
        .insert(key.clone())
}

/// The append target for this process's new `(entity, partition)` pairs — a single run file,
/// created lazily the first time a pair is recorded.
struct IndexWriter {
    next_run: u32,
    current: Option<(PathBuf, std::fs::File)>,
}

impl IndexWriter {
    fn file(&mut self, dir: &Path) -> Result<&mut std::fs::File, PartitionError> {
        if self.current.is_none() {
            let path = dir.join(format!("run-{:07}.jsonl", self.next_run));
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|source| PartitionError::Catalog {
                    path: path.clone(),
                    source,
                })?;
            self.current = Some((path, file));
        }
        Ok(&mut self.current.as_mut().unwrap().1)
    }
}

/// The three `id → Sites` maps the run-file index resolves into, by [`IndexKind`].
#[derive(Default)]
struct IndexMaps {
    obj: HashMap<KirId, Sites>,
    rel: HashMap<KirId, Sites>,
    endpoint: HashMap<KirId, Sites>,
}

impl IndexMaps {
    fn map(&self, k: IndexKind) -> &HashMap<KirId, Sites> {
        match k {
            IndexKind::Obj => &self.obj,
            IndexKind::Rel => &self.rel,
            IndexKind::Endpoint => &self.endpoint,
        }
    }
    fn mark_all_complete(&mut self) {
        for m in [&mut self.obj, &mut self.rel, &mut self.endpoint] {
            for s in m.values_mut() {
                s.complete = true;
            }
        }
    }
}

/// Write the whole index to `run-<num>.jsonl` atomically (temp + rename), lines sorted by
/// `(kind, id, partition key)` — the AEVT-style ordering compaction/rebuild produce.
fn write_run(dir: &Path, num: u32, maps: &IndexMaps) -> Result<(), PartitionError> {
    let mut lines: Vec<(IndexKind, KirId, &PartitionKey)> = Vec::new();
    for k in [IndexKind::Obj, IndexKind::Rel, IndexKind::Endpoint] {
        for (id, sites) in maps.map(k) {
            for p in &sites.partitions {
                lines.push((k, *id, p));
            }
        }
    }
    lines.sort_by(|a, b| {
        (a.0 as u8)
            .cmp(&(b.0 as u8))
            .then_with(|| a.1.as_str().cmp(&b.1.as_str()))
            .then_with(|| a.2.cmp(b.2))
    });

    let mut buf = String::new();
    for (k, id, p) in lines {
        let line = serde_json::to_string(&IndexLine {
            k,
            id,
            p: p.clone(),
        })
        .map_err(|source| PartitionError::CatalogParse {
            path: dir.to_path_buf(),
            source,
        })?;
        buf.push_str(&line);
        buf.push('\n');
    }

    let path = dir.join(format!("run-{num:07}.jsonl"));
    let tmp = dir.join(format!("run-{num:07}.jsonl.tmp"));
    std::fs::write(&tmp, &buf).map_err(|source| PartitionError::Catalog {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, &path).map_err(|source| PartitionError::Catalog { path, source })?;
    Ok(())
}

/// Routes `KirObject` reads/writes across multiple [`FactLedger`] partitions by a configurable
/// [`PartitionDimension`] + [`TimeBucket`] (RFC 0111 §1), with the `entity_id → Set<PartitionKey>`
/// correctness fix (RFC 0111 §2), a persisted catalog and a persisted AEVT-style entity index
/// (§5, Architecture Review).
pub struct PartitionedLedger {
    dimension: PartitionDimension,
    time_bucket: TimeBucket,
    catalog_root: PathBuf,
    index_dir: PathBuf,
    root_for: RootResolver,
    source_of: SourceResolver,
    catalog: Mutex<PartitionCatalog>,
    open: Mutex<HashMap<PartitionKey, Arc<FactLedger>>>,
    /// object entity id → object partitions (`IndexKind::Obj`)
    obj_sites: Mutex<HashMap<KirId, Sites>>,
    /// relationship id → relationship partitions (`IndexKind::Rel`)
    rel_sites: Mutex<HashMap<KirId, Sites>>,
    /// endpoint entity id → relationship partitions it participates in (`IndexKind::Endpoint`)
    endpoint_rels: Mutex<HashMap<KirId, Sites>>,
    index: Mutex<IndexWriter>,
}

impl PartitionedLedger {
    /// Opens the ledger, loading `<catalog_root>/catalog.json` if it exists (creating the directory
    /// otherwise). `root_for` resolves a *new* partition's on-disk root from its key — the hook
    /// point for container-config routing; partitions already in the catalog use their recorded
    /// root instead, so the catalog stays the source of truth for where data lives.
    pub fn new(
        catalog_root: impl Into<PathBuf>,
        dimension: PartitionDimension,
        time_bucket: TimeBucket,
        root_for: impl Fn(&PartitionKey) -> PathBuf + Send + Sync + 'static,
    ) -> Result<Self, PartitionError> {
        let catalog_root = catalog_root.into();
        std::fs::create_dir_all(&catalog_root).map_err(|source| PartitionError::Catalog {
            path: catalog_root.clone(),
            source,
        })?;
        let path = catalog_root.join("catalog.json");
        let catalog: PartitionCatalog = if path.exists() {
            let bytes = std::fs::read(&path).map_err(|source| PartitionError::Catalog {
                path: path.clone(),
                source,
            })?;
            serde_json::from_slice(&bytes)
                .map_err(|source| PartitionError::CatalogParse { path, source })?
        } else {
            PartitionCatalog::default()
        };

        // A ledger's routing/tiering config is frozen once its partitions exist — the on-disk
        // partition names encode both the dimension and the time bucket.
        if let Some(stored) = &catalog.dimension
            && stored != dimension.as_str()
        {
            return Err(PartitionError::DimensionMismatch {
                field: "dimension",
                stored: stored.clone(),
                requested: dimension.as_str().to_string(),
            });
        }
        if let Some(stored) = &catalog.time_bucket
            && stored != time_bucket.as_str()
        {
            return Err(PartitionError::DimensionMismatch {
                field: "time-bucket",
                stored: stored.clone(),
                requested: time_bucket.as_str().to_string(),
            });
        }

        let index_dir = catalog_root.join("index");
        let (mut maps, mut next_run, run_files) = Self::load_index(&index_dir)?;
        let nonempty = !maps.obj.is_empty() || !maps.rel.is_empty() || !maps.endpoint.is_empty();
        if run_files.len() >= COMPACT_AT && nonempty {
            write_run(&index_dir, next_run, &maps)?;
            for old in &run_files {
                let _ = std::fs::remove_file(old);
            }
            next_run += 1;
        }
        maps.mark_all_complete();

        Ok(Self {
            dimension,
            time_bucket,
            catalog_root,
            index_dir,
            root_for: Box::new(root_for),
            source_of: Box::new(|_| None),
            catalog: Mutex::new(catalog),
            open: Mutex::new(HashMap::new()),
            obj_sites: Mutex::new(maps.obj),
            rel_sites: Mutex::new(maps.rel),
            endpoint_rels: Mutex::new(maps.endpoint),
            index: Mutex::new(IndexWriter {
                next_run,
                current: None,
            }),
        })
    }

    /// Supply the resolver `PartitionDimension::SourceScope` / `Composite` route by: given a
    /// `KirObject`, return its originating source/connector (`"sql"`, `"git"`, …) or `None`.
    /// `None` at write time under a source-based dimension is a [`PartitionError::UnresolvedSource`].
    /// Not needed for `PartitionDimension::EntityKind`.
    pub fn with_source_resolver(
        mut self,
        source_of: impl Fn(&KirObject) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        self.source_of = Box::new(source_of);
        self
    }

    /// Fold every `index/run-*.jsonl` into fresh maps (not yet marked `complete`), also returning
    /// the next unused run number and the run files read. A line that fails to parse ends that file
    /// (torn tail) rather than aborting the open.
    fn load_index(dir: &Path) -> Result<LoadedIndex, PartitionError> {
        std::fs::create_dir_all(dir).map_err(|source| PartitionError::Catalog {
            path: dir.to_path_buf(),
            source,
        })?;
        let mut runs: Vec<(u32, PathBuf)> = Vec::new();
        for entry in std::fs::read_dir(dir).map_err(|source| PartitionError::Catalog {
            path: dir.to_path_buf(),
            source,
        })? {
            let entry = entry.map_err(|source| PartitionError::Catalog {
                path: dir.to_path_buf(),
                source,
            })?;
            let p = entry.path();
            if let Some(num) = p
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix("run-"))
                .and_then(|n| n.strip_suffix(".jsonl"))
                .and_then(|n| n.parse::<u32>().ok())
            {
                runs.push((num, p));
            }
        }
        runs.sort();

        let mut maps = IndexMaps::default();
        for (_num, p) in &runs {
            let content = std::fs::read_to_string(p).map_err(|source| PartitionError::Catalog {
                path: p.clone(),
                source,
            })?;
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<IndexLine>(line) {
                    Ok(l) => {
                        let m = match l.k {
                            IndexKind::Obj => &mut maps.obj,
                            IndexKind::Rel => &mut maps.rel,
                            IndexKind::Endpoint => &mut maps.endpoint,
                        };
                        m.entry(l.id).or_default().partitions.insert(l.p);
                    }
                    Err(_) => break,
                }
            }
        }
        let next_run = runs.last().map(|(n, _)| n + 1).unwrap_or(1);
        Ok((maps, next_run, runs.into_iter().map(|(_, p)| p).collect()))
    }

    /// Append `(kind, id, key)` membership lines to this process's run file (created on first use).
    /// Flushed to the OS, not fsync'd — see the module doc's "Known limits".
    fn record(
        &self,
        kind: IndexKind,
        id: KirId,
        keys: &[PartitionKey],
    ) -> Result<(), PartitionError> {
        if keys.is_empty() {
            return Ok(());
        }
        let mut writer = self.index.lock().unwrap();
        let dir = self.index_dir.clone();
        let file = writer.file(&dir)?;
        for key in keys {
            let line = serde_json::to_string(&IndexLine {
                k: kind,
                id,
                p: key.clone(),
            })
            .map_err(|source| PartitionError::CatalogParse {
                path: dir.clone(),
                source,
            })?;
            writeln!(file, "{line}").map_err(|source| PartitionError::Catalog {
                path: dir.clone(),
                source,
            })?;
        }
        file.flush()
            .map_err(|source| PartitionError::Catalog { path: dir, source })?;
        Ok(())
    }

    /// `ekos ledger repair`-style full rebuild: re-derive the entire index (objects **and**
    /// relationships/endpoints) by scanning every partition, replace the in-memory maps, and
    /// collapse the on-disk runs to one. Use after a crash that may have lost a pair line.
    pub fn rebuild_entity_index(&self) -> Result<(), PartitionError> {
        let mut fresh = IndexMaps::default();
        for (key, ledger) in self.catalog_snapshot(None)? {
            for obj in ledger
                .all_objects()
                .map_err(|source| PartitionError::Ledger {
                    key: key.clone(),
                    source,
                })?
            {
                fresh
                    .obj
                    .entry(obj.id)
                    .or_default()
                    .partitions
                    .insert(key.clone());
            }
            if is_relationship_partition(&key) {
                for rel in ledger
                    .all_relationships()
                    .map_err(|source| PartitionError::Ledger {
                        key: key.clone(),
                        source,
                    })?
                {
                    fresh
                        .rel
                        .entry(rel.id)
                        .or_default()
                        .partitions
                        .insert(key.clone());
                    fresh
                        .endpoint
                        .entry(rel.from)
                        .or_default()
                        .partitions
                        .insert(key.clone());
                    fresh
                        .endpoint
                        .entry(rel.to)
                        .or_default()
                        .partitions
                        .insert(key.clone());
                }
            }
        }
        fresh.mark_all_complete();

        {
            let mut writer = self.index.lock().unwrap();
            let run = writer.next_run;
            write_run(&self.index_dir, run, &fresh)?;
            if let Ok(entries) = std::fs::read_dir(&self.index_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    let keep = p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n == format!("run-{run:07}.jsonl"))
                        .unwrap_or(false);
                    if !keep && p.extension().is_some_and(|e| e == "jsonl") {
                        let _ = std::fs::remove_file(&p);
                    }
                }
            }
            writer.next_run = run + 1;
            writer.current = None;
        }
        *self.obj_sites.lock().unwrap() = fresh.obj;
        *self.rel_sites.lock().unwrap() = fresh.rel;
        *self.endpoint_rels.lock().unwrap() = fresh.endpoint;
        Ok(())
    }

    fn catalog_path(&self) -> PathBuf {
        self.catalog_root.join("catalog.json")
    }

    /// Atomically rewrite `catalog.json`. Called only while holding the catalog lock, and only when
    /// a new partition is registered.
    fn persist_catalog(&self, catalog: &PartitionCatalog) -> Result<(), PartitionError> {
        let path = self.catalog_path();
        let tmp = self.catalog_root.join("catalog.json.tmp");
        let json =
            serde_json::to_vec_pretty(catalog).map_err(|source| PartitionError::CatalogParse {
                path: path.clone(),
                source,
            })?;
        std::fs::write(&tmp, &json).map_err(|source| PartitionError::Catalog {
            path: tmp.clone(),
            source,
        })?;
        std::fs::rename(&tmp, &path).map_err(|source| PartitionError::Catalog { path, source })?;
        Ok(())
    }

    fn key_for(&self, obj: &KirObject) -> Result<PartitionKey, PartitionError> {
        let dimension_value = match self.dimension {
            PartitionDimension::EntityKind => obj.kind.to_string(),
            PartitionDimension::SourceScope => {
                (self.source_of)(obj).ok_or(PartitionError::UnresolvedSource { entity: obj.id })?
            }
            PartitionDimension::Composite => {
                let source = (self.source_of)(obj)
                    .ok_or(PartitionError::UnresolvedSource { entity: obj.id })?;
                format!("{source}\u{1f}{}", obj.kind)
            }
        };
        Ok(PartitionKey {
            time_bucket: self.time_bucket.label(obj.created_at),
            dimension_value,
        })
    }

    /// Relationships always route by `"rel:"+<RelationshipKind>` + time bucket, independent of the
    /// object dimension (RFC 0111 amendment 2026-08-29 §1) — they have no clean source, and their
    /// kind is the query axis for impact/neighborhood analysis.
    fn relationship_key_for(&self, rel: &KirRelationship) -> PartitionKey {
        PartitionKey {
            time_bucket: self.time_bucket.label(rel.created_at),
            dimension_value: format!("rel:{}", rel.kind),
        }
    }

    /// The partition for `key`, opening it (and creating it on disk) on demand. With
    /// `register = true`, a not-yet-known partition is added to the catalog and persisted; with
    /// `register = false` (all read paths) an unknown key still opens but is not recorded — read
    /// paths only ever pass keys taken from the catalog, so that branch is effectively unreachable.
    /// No two of the three locks are ever held at once.
    fn partition(
        &self,
        key: &PartitionKey,
        register: bool,
    ) -> Result<Arc<FactLedger>, PartitionError> {
        if let Some(ledger) = self.open.lock().unwrap().get(key) {
            return Ok(Arc::clone(ledger));
        }
        let root = {
            let mut catalog = self.catalog.lock().unwrap();
            let mut changed = false;
            let root = match catalog.partitions.iter_mut().find(|e| &e.key == key) {
                Some(entry) => {
                    // RFC 0111 §3: any access to a cold partition promotes it back to hot.
                    if entry.tier == Tier::Cold {
                        entry.tier = Tier::Hot;
                        changed = true;
                    }
                    entry.root.clone()
                }
                None => {
                    let root = (self.root_for)(key);
                    if register {
                        catalog
                            .dimension
                            .get_or_insert_with(|| self.dimension.as_str().to_string());
                        catalog
                            .time_bucket
                            .get_or_insert_with(|| self.time_bucket.as_str().to_string());
                        catalog.partitions.push(PartitionEntry {
                            key: key.clone(),
                            root: root.clone(),
                            tier: Tier::Hot,
                        });
                        catalog.partitions.sort();
                        changed = true;
                    }
                    root
                }
            };
            if changed {
                self.persist_catalog(&catalog)?;
            }
            root
        };
        let ledger =
            Arc::new(
                FactLedger::open(&root).map_err(|source| PartitionError::Ledger {
                    key: key.clone(),
                    source,
                })?,
            );
        self.open
            .lock()
            .unwrap()
            .insert(key.clone(), Arc::clone(&ledger));
        Ok(ledger)
    }

    /// Every catalog partition (optionally pruned to one dimension value — RFC 0111 §1: broad reads
    /// "fan out only to partitions whose dimension value … could match"), opened lazily, in
    /// ascending key order (oldest time bucket first).
    fn catalog_snapshot(
        &self,
        scope: Option<&str>,
    ) -> Result<Vec<(PartitionKey, Arc<FactLedger>)>, PartitionError> {
        self.catalog_snapshot_where(|k| scope.is_none_or(|s| k.dimension_value == s))
    }

    /// Like [`Self::catalog_snapshot`] but with an arbitrary key predicate (used to scope to all
    /// `"rel:*"` relationship partitions, which no single exact `dimension_value` covers).
    fn catalog_snapshot_where(
        &self,
        pred: impl Fn(&PartitionKey) -> bool,
    ) -> Result<Vec<(PartitionKey, Arc<FactLedger>)>, PartitionError> {
        let mut keys: Vec<PartitionKey> = {
            let catalog = self.catalog.lock().unwrap();
            catalog
                .partitions
                .iter()
                .filter(|e| pred(&e.key))
                .map(|e| e.key.clone())
                .collect()
        };
        keys.sort();
        let mut out = Vec::with_capacity(keys.len());
        for key in keys {
            let ledger = self.partition(&key, false)?;
            out.push((key, ledger));
        }
        Ok(out)
    }

    /// Resolve an `id`'s partition set from one of the in-memory index maps. Fast path: the map
    /// (loaded on open) or a prior resolution already marked it `complete` — no partition touched.
    /// Slow path: scan the catalog with `present` once, then append the discovered pairs to the
    /// index (self-healing). Returned ascending; the greatest element is the newest partition.
    fn sites_map(&self, kind: IndexKind) -> &Mutex<HashMap<KirId, Sites>> {
        match kind {
            IndexKind::Obj => &self.obj_sites,
            IndexKind::Rel => &self.rel_sites,
            IndexKind::Endpoint => &self.endpoint_rels,
        }
    }

    fn resolve_sites(
        &self,
        kind: IndexKind,
        id: &KirId,
        scan: impl FnOnce() -> Result<Vec<(PartitionKey, Arc<FactLedger>)>, PartitionError>,
        present: impl Fn(&Arc<FactLedger>, &KirId) -> Result<bool, PartitionError>,
    ) -> Result<BTreeSet<PartitionKey>, PartitionError> {
        {
            let map = self.sites_map(kind).lock().unwrap();
            if let Some(entry) = map.get(id)
                && entry.complete
            {
                return Ok(entry.partitions.clone());
            }
        }
        let mut found = BTreeSet::new();
        for (key, ledger) in scan()? {
            if present(&ledger, id)? {
                found.insert(key);
            }
        }
        let to_record: Vec<PartitionKey> = {
            let map = self.sites_map(kind).lock().unwrap();
            let known = map.get(id).map(|e| &e.partitions);
            found
                .iter()
                .filter(|k| known.is_none_or(|kn| !kn.contains(k)))
                .cloned()
                .collect()
        };
        self.record(kind, *id, &to_record)?;

        let mut map = self.sites_map(kind).lock().unwrap();
        let entry = map.entry(*id).or_default();
        entry.partitions.extend(found);
        entry.complete = true;
        Ok(entry.partitions.clone())
    }

    /// Every object partition this entity has a version in (RFC 0111 §2).
    fn sites(&self, id: &KirId) -> Result<BTreeSet<PartitionKey>, PartitionError> {
        self.resolve_sites(
            IndexKind::Obj,
            id,
            || self.catalog_snapshot(None),
            |ledger, id| Ok(ledger.get_object(id).map_err(ledger_err)?.is_some()),
        )
    }

    /// Every relationship partition holding a version of relationship `rel_id`.
    fn rel_id_sites(&self, rel_id: &KirId) -> Result<BTreeSet<PartitionKey>, PartitionError> {
        self.resolve_sites(
            IndexKind::Rel,
            rel_id,
            || self.catalog_snapshot_where(is_relationship_partition),
            |ledger, id| Ok(ledger.get_relationship(id).map_err(ledger_err)?.is_some()),
        )
    }

    /// Every relationship partition in which entity `id` is an endpoint (`from` or `to`).
    fn endpoint_sites(&self, id: &KirId) -> Result<BTreeSet<PartitionKey>, PartitionError> {
        self.resolve_sites(
            IndexKind::Endpoint,
            id,
            || self.catalog_snapshot_where(is_relationship_partition),
            |ledger, id| Ok(!ledger.relationships_for(id).map_err(ledger_err)?.is_empty()),
        )
    }

    pub fn append_object(&self, obj: &KirObject) -> Result<bool, PartitionError> {
        let key = self.key_for(obj)?;
        let ledger = self.partition(&key, true)?;
        let result = ledger
            .append_object(obj)
            .map_err(|source| PartitionError::Ledger {
                key: key.clone(),
                source,
            })?;
        let is_new_pair = insert_site(&self.obj_sites, obj.id, &key);
        if is_new_pair {
            self.record(IndexKind::Obj, obj.id, std::slice::from_ref(&key))?;
        }
        Ok(result)
    }

    /// Append a relationship. Routes to `"rel:"+kind` + time bucket (RFC 0111 amendment §1);
    /// records `rel`-id and `endpoint`-`from`/`to` index lines (§2).
    pub fn append_relationship(&self, rel: &KirRelationship) -> Result<bool, PartitionError> {
        let key = self.relationship_key_for(rel);
        let ledger = self.partition(&key, true)?;
        let result = ledger
            .append_relationship(rel)
            .map_err(|source| PartitionError::Ledger {
                key: key.clone(),
                source,
            })?;
        if insert_site(&self.rel_sites, rel.id, &key) {
            self.record(IndexKind::Rel, rel.id, std::slice::from_ref(&key))?;
        }
        for endpoint in [rel.from, rel.to] {
            if insert_site(&self.endpoint_rels, endpoint, &key) {
                self.record(IndexKind::Endpoint, endpoint, std::slice::from_ref(&key))?;
            }
        }
        Ok(result)
    }

    /// Current state: routes to exactly one partition — the entity's most recent (RFC 0111 §2:
    /// current state always lives in the newest partition, so no fan-out here even though the
    /// entity may span more than one partition historically). `None` for an id no partition holds.
    pub fn get_object(&self, id: &KirId) -> Result<Option<KirObject>, PartitionError> {
        let Some(newest) = self.sites(id)?.into_iter().next_back() else {
            return Ok(None);
        };
        let ledger = self.partition(&newest, false)?;
        ledger
            .get_object(id)
            .map_err(|source| PartitionError::Ledger {
                key: newest,
                source,
            })
    }

    /// Full history: fans out to every partition this entity has ever been written to (RFC 0111
    /// §2's correctness fix), oldest partition first. Correct without a separate timestamp
    /// comparison because partitions are strictly time-ordered by construction — writes always
    /// route by the *current* time bucket, so an entity's partition set only ever grows into newer
    /// buckets, never backfills an older one — and `PartitionKey`'s derived `Ord` sorts by
    /// `time_bucket` first (`sites` returns a `BTreeSet`, iterated ascending).
    pub fn object_history(&self, id: &KirId) -> Result<Vec<KirObject>, PartitionError> {
        let mut history = Vec::new();
        for key in self.sites(id)? {
            let ledger = self.partition(&key, false)?;
            history.extend(
                ledger
                    .object_history(id)
                    .map_err(|source| PartitionError::Ledger {
                        key: key.clone(),
                        source,
                    })?,
            );
        }
        Ok(history)
    }

    /// Current state of relationship `id` — resolves to its newest relationship partition only.
    pub fn get_relationship(&self, id: &KirId) -> Result<Option<KirRelationship>, PartitionError> {
        let Some(newest) = self.rel_id_sites(id)?.into_iter().next_back() else {
            return Ok(None);
        };
        let ledger = self.partition(&newest, false)?;
        ledger
            .get_relationship(id)
            .map_err(|source| PartitionError::Ledger {
                key: newest,
                source,
            })
    }

    /// Full version history of relationship `id`, oldest partition first.
    pub fn relationship_history(&self, id: &KirId) -> Result<Vec<KirRelationship>, PartitionError> {
        let mut history = Vec::new();
        for key in self.rel_id_sites(id)? {
            let ledger = self.partition(&key, false)?;
            history.extend(ledger.relationship_history(id).map_err(|source| {
                PartitionError::Ledger {
                    key: key.clone(),
                    source,
                }
            })?);
        }
        Ok(history)
    }

    /// Every relationship touching entity `id` — **pruned** to the relationship partitions `id`
    /// participates in via the `endpoint` index (RFC 0111 amendment §2), never a full fan-out.
    pub fn relationships_for(&self, id: &KirId) -> Result<Vec<KirRelationship>, PartitionError> {
        let mut out = Vec::new();
        for key in self.endpoint_sites(id)? {
            let ledger = self.partition(&key, false)?;
            out.extend(
                ledger
                    .relationships_for(id)
                    .map_err(|source| PartitionError::Ledger {
                        key: key.clone(),
                        source,
                    })?,
            );
        }
        Ok(dedup_current_rels(out))
    }

    /// Every current-state relationship across every relationship partition, deduplicated.
    pub fn all_relationships(&self) -> Result<Vec<KirRelationship>, PartitionError> {
        let mut out = Vec::new();
        for (key, ledger) in self.catalog_snapshot_where(is_relationship_partition)? {
            out.extend(
                ledger
                    .all_relationships()
                    .map_err(|source| PartitionError::Ledger { key, source })?,
            );
        }
        Ok(dedup_current_rels(out))
    }

    /// Distinct current-state relationship count.
    pub fn relationship_count(&self) -> Result<usize, PartitionError> {
        Ok(self.all_relationships()?.len())
    }

    /// Deduplicate current-state objects fanned in from multiple partitions: an entity that spans
    /// K partitions appears once per partition (a stale version in each older one). Iterating
    /// oldest→newest and letting later inserts win keeps exactly the newest-partition version.
    fn dedup_current(rows: Vec<(PartitionKey, Vec<KirObject>)>) -> Vec<KirObject> {
        let mut by_id: HashMap<KirId, KirObject> = HashMap::new();
        for (_key, objs) in rows {
            for obj in objs {
                by_id.insert(obj.id, obj);
            }
        }
        by_id.into_values().collect()
    }

    /// Broad read, **pruned** to one dimension value (RFC 0111 §1): touches only the catalog
    /// partitions whose key matches `dimension_value`, deduplicated to current state.
    pub fn objects_in_kind(&self, dimension_value: &str) -> Result<Vec<KirObject>, PartitionError> {
        let mut rows = Vec::new();
        for (key, ledger) in self.catalog_snapshot(Some(dimension_value))? {
            let objs = ledger
                .all_objects()
                .map_err(|source| PartitionError::Ledger {
                    key: key.clone(),
                    source,
                })?;
            rows.push((key, objs));
        }
        Ok(Self::dedup_current(rows))
    }

    /// Every current-state object across every catalog partition — the unscoped-query worst case
    /// RFC 0111 names: always fans out to everything, never pruned. Prefer [`Self::objects_in_kind`]
    /// whenever the query is scoped to one entity kind.
    pub fn all_objects(&self) -> Result<Vec<KirObject>, PartitionError> {
        let mut rows = Vec::new();
        for (key, ledger) in self.catalog_snapshot(None)? {
            let objs = ledger
                .all_objects()
                .map_err(|source| PartitionError::Ledger {
                    key: key.clone(),
                    source,
                })?;
            rows.push((key, objs));
        }
        Ok(Self::dedup_current(rows))
    }

    /// Distinct current-state object count across all partitions (an entity spanning K partitions
    /// counts once, not K times).
    pub fn object_count(&self) -> Result<usize, PartitionError> {
        Ok(self.all_objects()?.len())
    }

    /// The partitions currently open in this process — test/introspection use (proving lazy
    /// opening), not part of the read/write surface. See [`Self::catalog_partition_keys`] for
    /// every *known* partition.
    pub fn partition_keys(&self) -> Vec<PartitionKey> {
        self.open.lock().unwrap().keys().cloned().collect()
    }

    /// Every partition in the persisted catalog, whether or not it is open in this process.
    pub fn catalog_partition_keys(&self) -> Vec<PartitionKey> {
        let mut keys: Vec<PartitionKey> = self
            .catalog
            .lock()
            .unwrap()
            .partitions
            .iter()
            .map(|e| e.key.clone())
            .collect();
        keys.sort();
        keys
    }

    /// The catalog partitions a scoped query for `dimension_value` would touch — a strict subset of
    /// [`Self::catalog_partition_keys`] whenever more than one dimension value is present.
    pub fn partition_keys_in_scope(&self, dimension_value: &str) -> Vec<PartitionKey> {
        self.catalog
            .lock()
            .unwrap()
            .partitions
            .iter()
            .filter(|e| e.key.dimension_value == dimension_value)
            .map(|e| e.key.clone())
            .collect()
    }

    /// RFC 0111 §3: demote every hot partition whose time bucket falls entirely before `cutoff` to
    /// [`Tier::Cold`] — evicting its open handle and flagging it "eligible to relocate". Because
    /// writes only ever route to the *current* bucket, a past-bucket partition is structurally
    /// sealed. Returns how many were demoted. A subsequent read of a cold partition promotes it
    /// back to hot. Idempotent.
    pub fn mark_cold_before(&self, cutoff: DateTime<Utc>) -> Result<usize, PartitionError> {
        let cutoff_bucket = self.time_bucket.label(cutoff);
        let demoted: Vec<PartitionKey> = {
            let mut catalog = self.catalog.lock().unwrap();
            let mut demoted = Vec::new();
            for entry in catalog.partitions.iter_mut() {
                if entry.tier == Tier::Hot && entry.key.time_bucket < cutoff_bucket {
                    entry.tier = Tier::Cold;
                    demoted.push(entry.key.clone());
                }
            }
            if !demoted.is_empty() {
                self.persist_catalog(&catalog)?;
            }
            demoted
        };
        if !demoted.is_empty() {
            let mut open = self.open.lock().unwrap();
            for key in &demoted {
                open.remove(key);
            }
        }
        Ok(demoted.len())
    }

    /// The tier a partition is recorded at, or `None` if the key isn't in the catalog.
    pub fn partition_tier(&self, key: &PartitionKey) -> Option<Tier> {
        self.catalog
            .lock()
            .unwrap()
            .partitions
            .iter()
            .find(|e| &e.key == key)
            .map(|e| e.tier)
    }

    /// Every partition currently recorded as [`Tier::Cold`], sorted.
    pub fn cold_partition_keys(&self) -> Vec<PartitionKey> {
        let mut keys: Vec<PartitionKey> = self
            .catalog
            .lock()
            .unwrap()
            .partitions
            .iter()
            .filter(|e| e.tier == Tier::Cold)
            .map(|e| e.key.clone())
            .collect();
        keys.sort();
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{ObjectKind, RelationshipKind};
    use tempfile::tempdir;

    fn rel(from: KirId, to: KirId, kind: RelationshipKind) -> KirRelationship {
        KirRelationship::new(kind, from, to)
    }

    fn ledger_with_root(dir: &Path) -> PartitionedLedger {
        ledger_with_bucket(dir, TimeBucket::Monthly)
    }

    fn ledger_with_bucket(dir: &Path, bucket: TimeBucket) -> PartitionedLedger {
        let root = dir.to_path_buf();
        let part_root = root.clone();
        PartitionedLedger::new(root, PartitionDimension::EntityKind, bucket, move |key| {
            part_root.join(&key.dimension_value).join(&key.time_bucket)
        })
        .unwrap()
    }

    #[test]
    fn different_entity_kinds_route_to_different_partitions() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());

        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        ledger
            .append_object(&KirObject::new("main.rs", ObjectKind::File))
            .unwrap();

        let mut keys: Vec<String> = ledger
            .partition_keys()
            .into_iter()
            .map(|k| k.dimension_value)
            .collect();
        keys.sort();
        assert_eq!(keys, vec!["File".to_string(), "Table".to_string()]);
        let this_month = chrono::Utc::now().format("%Y-%m").to_string();
        assert!(
            dir.path()
                .join("Table")
                .join(&this_month)
                .join("segments")
                .exists()
        );
        assert!(
            dir.path()
                .join("File")
                .join(&this_month)
                .join("segments")
                .exists()
        );
    }

    #[test]
    fn point_read_routes_to_a_single_partition_no_fan_out() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());
        let obj = KirObject::new("orders", ObjectKind::Table);
        ledger.append_object(&obj).unwrap();

        let fetched = ledger.get_object(&obj.id).unwrap();
        assert_eq!(fetched.unwrap().name, "orders");
        // Only the one partition this write actually routed to was ever opened.
        assert_eq!(ledger.partition_keys().len(), 1);
    }

    #[test]
    fn unknown_id_returns_none_without_touching_any_partition() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());
        assert!(ledger.get_object(&KirId::new()).unwrap().is_none());
        assert!(ledger.partition_keys().is_empty());
    }

    #[test]
    fn all_objects_fans_out_across_every_partition() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());
        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        ledger
            .append_object(&KirObject::new("main.rs", ObjectKind::File))
            .unwrap();

        assert_eq!(ledger.object_count().unwrap(), 2);
        let mut names: Vec<String> = ledger
            .all_objects()
            .unwrap()
            .into_iter()
            .map(|o| o.name)
            .collect();
        names.sort();
        assert_eq!(names, vec!["main.rs".to_string(), "orders".to_string()]);
    }

    /// RFC 0111 §1's scoped-query fast path: a broad read scoped to one entity kind touches only
    /// that kind's partitions, never the others.
    #[test]
    fn scoped_broad_read_is_pruned_to_matching_partitions() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());
        for i in 0..3 {
            ledger
                .append_object(&KirObject::new(format!("t{i}"), ObjectKind::Table))
                .unwrap();
        }
        for i in 0..5 {
            ledger
                .append_object(&KirObject::new(format!("f{i}"), ObjectKind::File))
                .unwrap();
        }

        assert_eq!(ledger.catalog_partition_keys().len(), 2);
        assert_eq!(ledger.partition_keys_in_scope("Table").len(), 1);
        assert!(
            ledger.partition_keys_in_scope("Table").len() < ledger.catalog_partition_keys().len()
        );

        let tables = ledger.objects_in_kind("Table").unwrap();
        assert_eq!(tables.len(), 3);
        assert!(tables.iter().all(|o| o.kind == ObjectKind::Table));

        assert_eq!(ledger.objects_in_kind("File").unwrap().len(), 5);
        // A scope that matches no partition reads nothing.
        assert!(ledger.objects_in_kind("Module").unwrap().is_empty());
    }

    /// The RFC 0111 §2 correctness property: force one entity's two writes into two different
    /// time-bucket partitions and confirm `get_object` still resolves to a single (the newest)
    /// partition while `object_history` fans out to both, in chronological order.
    #[test]
    fn entity_spanning_two_time_buckets_gets_single_partition_point_reads_and_full_fan_out_history()
    {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());

        let id = KirId::new();
        let mut v1 = KirObject::new("orders", ObjectKind::Table);
        v1.id = id;
        v1.created_at = "2026-07-15T00:00:00Z".parse().unwrap();
        let mut v2 = KirObject::new("orders_renamed", ObjectKind::Table);
        v2.id = id;
        v2.created_at = "2026-08-15T00:00:00Z".parse().unwrap();

        ledger.append_object(&v1).unwrap();
        ledger.append_object(&v2).unwrap();

        assert_eq!(ledger.catalog_partition_keys().len(), 2);

        let current = ledger.get_object(&id).unwrap().unwrap();
        assert_eq!(current.name, "orders_renamed");

        let history = ledger.object_history(&id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].name, "orders");
        assert_eq!(history[1].name, "orders_renamed");
    }

    /// Time-bucket granularity is configurable (RFC 0111 §1): `Daily` splits partitions by day.
    #[test]
    fn daily_time_bucket_splits_partitions_by_day() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_bucket(dir.path(), TimeBucket::Daily);

        let mut a = KirObject::new("a", ObjectKind::Table);
        a.created_at = "2026-08-27T09:00:00Z".parse().unwrap();
        let mut b = KirObject::new("b", ObjectKind::Table);
        b.created_at = "2026-08-27T21:00:00Z".parse().unwrap();
        let mut c = KirObject::new("c", ObjectKind::Table);
        c.created_at = "2026-08-28T01:00:00Z".parse().unwrap();

        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();
        ledger.append_object(&c).unwrap();

        let mut buckets: Vec<String> = ledger
            .catalog_partition_keys()
            .into_iter()
            .map(|k| k.time_bucket)
            .collect();
        buckets.sort();
        buckets.dedup();
        assert_eq!(
            buckets,
            vec!["2026-08-27".to_string(), "2026-08-28".to_string()]
        );
        assert_eq!(ledger.object_count().unwrap(), 3);
    }

    #[test]
    fn time_bucket_parses_config_strings() {
        assert_eq!(TimeBucket::parse("daily"), Some(TimeBucket::Daily));
        assert_eq!(TimeBucket::parse("  Weekly "), Some(TimeBucket::Weekly));
        assert_eq!(TimeBucket::parse("MONTHLY"), Some(TimeBucket::Monthly));
        assert_eq!(TimeBucket::parse("hourly"), None);
        assert_eq!(TimeBucket::default(), TimeBucket::Monthly);
    }

    /// RFC 0111 §1 / Acceptance Criteria: "N partitions admit N concurrent writers instead of one
    /// global `SegmentStore`."
    #[test]
    fn concurrent_writers_across_two_partitions() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());
        const N: usize = 60;

        std::thread::scope(|s| {
            s.spawn(|| {
                for i in 0..N {
                    ledger
                        .append_object(&KirObject::new(format!("t{i}"), ObjectKind::Table))
                        .unwrap();
                }
            });
            s.spawn(|| {
                for i in 0..N {
                    ledger
                        .append_object(&KirObject::new(format!("f{i}"), ObjectKind::File))
                        .unwrap();
                }
            });
        });

        assert_eq!(ledger.object_count().unwrap(), 2 * N);
        assert_eq!(ledger.objects_in_kind("Table").unwrap().len(), N);
        assert_eq!(ledger.objects_in_kind("File").unwrap().len(), N);
        assert_eq!(ledger.catalog_partition_keys().len(), 2);

        let this_month = chrono::Utc::now().format("%Y-%m").to_string();
        for kind in ["Table", "File"] {
            assert!(
                dir.path()
                    .join(kind)
                    .join(&this_month)
                    .join("segments")
                    .exists()
            );
        }
    }

    /// RFC 0111 §5: the catalog **and** the entity index are persisted, so a brand-new
    /// `PartitionedLedger` at the same root sees every partition and resolves any entity with no
    /// partition scan at all.
    #[test]
    fn catalog_and_entities_survive_a_reopen() {
        let dir = tempdir().unwrap();
        let legacy_id = KirId::new();
        {
            let ledger = ledger_with_root(dir.path());
            ledger
                .append_object(&KirObject::new("orders", ObjectKind::Table))
                .unwrap();
            ledger
                .append_object(&KirObject::new("main.rs", ObjectKind::File))
                .unwrap();
            // one entity with history across two older time-bucket partitions
            let mut v1 = KirObject::new("legacy", ObjectKind::Table);
            v1.id = legacy_id;
            v1.created_at = "2026-05-10T00:00:00Z".parse().unwrap();
            let mut v2 = KirObject::new("legacy_v2", ObjectKind::Table);
            v2.id = legacy_id;
            v2.created_at = "2026-06-10T00:00:00Z".parse().unwrap();
            ledger.append_object(&v1).unwrap();
            ledger.append_object(&v2).unwrap();
        }
        assert!(dir.path().join("catalog.json").exists());
        assert!(
            std::fs::read_dir(dir.path().join("index"))
                .unwrap()
                .any(|e| e.unwrap().file_name().to_string_lossy().starts_with("run-")),
            "index run file written"
        );

        // fresh handle, same root, zero writes
        let reopened = ledger_with_root(dir.path());
        assert!(
            reopened.partition_keys().is_empty(),
            "nothing open until a read touches it"
        );
        // Table/2026-05, Table/2026-06, Table/<now>, File/<now>
        assert_eq!(reopened.catalog_partition_keys().len(), 4);

        // Point read: resolved from the persisted entity index — opens ONLY the entity's newest
        // partition, never scans the other three.
        assert_eq!(
            reopened.get_object(&legacy_id).unwrap().unwrap().name,
            "legacy_v2"
        );
        assert_eq!(
            reopened.partition_keys().len(),
            1,
            "no catalog scan — only the entity's newest partition was opened"
        );

        // Full history: opens exactly the two partitions the entity spans, still no scan.
        let hist: Vec<String> = reopened
            .object_history(&legacy_id)
            .unwrap()
            .into_iter()
            .map(|o| o.name)
            .collect();
        assert_eq!(hist, vec!["legacy".to_string(), "legacy_v2".to_string()]);
        assert_eq!(reopened.partition_keys().len(), 2);

        // pruned broad read works off the persisted catalog, deduplicated to current state
        let mut table_names: Vec<String> = reopened
            .objects_in_kind("Table")
            .unwrap()
            .into_iter()
            .map(|o| o.name)
            .collect();
        table_names.sort();
        assert_eq!(
            table_names,
            vec!["legacy_v2".to_string(), "orders".to_string()]
        );
        assert_eq!(reopened.objects_in_kind("File").unwrap().len(), 1);
        assert!(reopened.objects_in_kind("Module").unwrap().is_empty());

        // an unknown id resolves cleanly (scans, finds nothing, caches)
        assert!(reopened.get_object(&KirId::new()).unwrap().is_none());

        // the reopened handle's own writes still route + register correctly
        reopened
            .append_object(&KirObject::new(
                "new_svc",
                ObjectKind::Custom("Service".into()),
            ))
            .unwrap();
        assert_eq!(reopened.catalog_partition_keys().len(), 5);
    }

    /// The entity index tolerates a lost partition-crossing pair line: after open, the affected
    /// entity's history is short one partition until `rebuild_entity_index` re-derives it from the
    /// partitions themselves.
    #[test]
    fn rebuild_entity_index_repairs_a_dropped_pair_line() {
        let dir = tempdir().unwrap();
        let id = KirId::new();
        {
            let ledger = ledger_with_root(dir.path());
            let mut v1 = KirObject::new("svc", ObjectKind::Table);
            v1.id = id;
            v1.created_at = "2026-03-10T00:00:00Z".parse().unwrap();
            let mut v2 = KirObject::new("svc_v2", ObjectKind::Table);
            v2.id = id;
            v2.created_at = "2026-04-10T00:00:00Z".parse().unwrap();
            ledger.append_object(&v1).unwrap();
            ledger.append_object(&v2).unwrap();
        }

        // Simulate a lost crossing line: drop the newer partition's pair from every run file.
        let idx = dir.path().join("index");
        for entry in std::fs::read_dir(&idx).unwrap() {
            let p = entry.unwrap().path();
            let kept: String = std::fs::read_to_string(&p)
                .unwrap()
                .lines()
                .filter(|l| !l.contains("2026-04"))
                .map(|l| format!("{l}\n"))
                .collect();
            std::fs::write(&p, kept).unwrap();
        }

        let reopened = ledger_with_root(dir.path());
        // history is short the newer partition…
        assert_eq!(reopened.object_history(&id).unwrap().len(), 1);

        // …until a rebuild re-derives the index from the partitions.
        reopened.rebuild_entity_index().unwrap();
        let hist: Vec<String> = reopened
            .object_history(&id)
            .unwrap()
            .into_iter()
            .map(|o| o.name)
            .collect();
        assert_eq!(hist, vec!["svc".to_string(), "svc_v2".to_string()]);
        assert_eq!(reopened.get_object(&id).unwrap().unwrap().name, "svc_v2");
    }

    /// Entity-index runs are merged once [`COMPACT_AT`] accumulate: many reopen-with-write cycles
    /// leave a bounded number of run files, and every entity still resolves.
    #[test]
    fn entity_index_runs_compact_on_open() {
        let dir = tempdir().unwrap();
        let mut ids = Vec::new();
        // COMPACT_AT + a few write sessions, each creating its own run file
        for i in 0..(COMPACT_AT + 3) {
            let ledger = ledger_with_root(dir.path());
            let obj = KirObject::new(format!("e{i}"), ObjectKind::Table);
            ids.push(obj.id);
            ledger.append_object(&obj).unwrap();
        }

        let run_count = std::fs::read_dir(dir.path().join("index"))
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("run-")
            })
            .count();
        assert!(
            run_count <= 5,
            "runs compacted, got {run_count} (COMPACT_AT = {COMPACT_AT})"
        );

        let reopened = ledger_with_root(dir.path());
        for id in &ids {
            assert!(reopened.get_object(id).unwrap().is_some());
        }
    }

    /// `PartitionDimension::SourceScope` routes by the source resolver's answer, independent of
    /// `ObjectKind`.
    #[test]
    fn source_scope_routes_by_resolver_not_kind() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let part_root = root.clone();
        let ledger = PartitionedLedger::new(
            root,
            PartitionDimension::SourceScope,
            TimeBucket::Monthly,
            move |key| part_root.join(&key.dimension_value).join(&key.time_bucket),
        )
        .unwrap()
        .with_source_resolver(|obj| {
            // pretend name prefix carries the source
            obj.name.split_once(':').map(|(src, _)| src.to_string())
        });

        // Same kind (Table), different sources → different partitions.
        ledger
            .append_object(&KirObject::new("sql:orders", ObjectKind::Table))
            .unwrap();
        ledger
            .append_object(&KirObject::new("pentaho:orders_stg", ObjectKind::Table))
            .unwrap();
        // Different kind, same source as the first → same partition.
        ledger
            .append_object(&KirObject::new(
                "sql:load_orders",
                ObjectKind::Custom("View".into()),
            ))
            .unwrap();

        let mut scopes: Vec<String> = ledger
            .catalog_partition_keys()
            .into_iter()
            .map(|k| k.dimension_value)
            .collect();
        scopes.sort();
        assert_eq!(scopes, vec!["pentaho".to_string(), "sql".to_string()]);
        assert_eq!(ledger.objects_in_kind("sql").unwrap().len(), 2);
        assert_eq!(ledger.objects_in_kind("pentaho").unwrap().len(), 1);
    }

    /// A source-based dimension with no resolver answer is a hard error, never a silent misroute.
    #[test]
    fn source_scope_without_a_resolved_source_errors() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let ledger = PartitionedLedger::new(
            root.clone(),
            PartitionDimension::SourceScope,
            TimeBucket::Monthly,
            move |key| root.join(&key.dimension_value),
        )
        .unwrap(); // no with_source_resolver → default returns None

        let err = ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap_err();
        assert!(matches!(err, PartitionError::UnresolvedSource { .. }));
    }

    /// `Composite` partitions by `source` + `kind` together.
    #[test]
    fn composite_partitions_by_source_and_kind() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let part_root = root.clone();
        let ledger = PartitionedLedger::new(
            root,
            PartitionDimension::Composite,
            TimeBucket::Monthly,
            move |key| part_root.join(key.dimension_value.replace('\u{1f}', "__")),
        )
        .unwrap()
        .with_source_resolver(|obj| obj.name.split_once(':').map(|(s, _)| s.to_string()));

        ledger
            .append_object(&KirObject::new("sql:orders", ObjectKind::Table))
            .unwrap();
        ledger
            .append_object(&KirObject::new(
                "sql:load",
                ObjectKind::Custom("View".into()),
            ))
            .unwrap();
        ledger
            .append_object(&KirObject::new("git:main.rs", ObjectKind::File))
            .unwrap();

        // sql+Table, sql+View, git+File → three distinct composite partitions.
        assert_eq!(ledger.catalog_partition_keys().len(), 3);
        assert_eq!(
            ledger
                .objects_in_kind(&format!("sql\u{1f}{}", ObjectKind::Table))
                .unwrap()
                .len(),
            1
        );
    }

    /// The routing/tiering config is frozen once partitions exist: reopening with a different
    /// dimension or time bucket is a `DimensionMismatch` error, not a silent re-route.
    #[test]
    fn reopening_with_a_changed_dimension_or_bucket_errors() {
        let dir = tempdir().unwrap();
        {
            let ledger = ledger_with_bucket(dir.path(), TimeBucket::Monthly);
            ledger
                .append_object(&KirObject::new("orders", ObjectKind::Table))
                .unwrap();
        }
        // same dimension + bucket → fine
        assert!(
            ledger_with_bucket(dir.path(), TimeBucket::Monthly)
                .get_object(&KirId::new())
                .is_ok()
        );

        // changed time bucket → error
        let root = dir.path().to_path_buf();
        let res = PartitionedLedger::new(
            root.clone(),
            PartitionDimension::EntityKind,
            TimeBucket::Daily,
            move |k| root.join(&k.dimension_value),
        );
        assert!(matches!(
            res,
            Err(PartitionError::DimensionMismatch {
                field: "time-bucket",
                ..
            })
        ));

        // changed dimension → error
        let root = dir.path().to_path_buf();
        let res = PartitionedLedger::new(
            root.clone(),
            PartitionDimension::SourceScope,
            TimeBucket::Monthly,
            move |k| root.join(&k.dimension_value),
        );
        assert!(matches!(
            res,
            Err(PartitionError::DimensionMismatch {
                field: "dimension",
                ..
            })
        ));
    }

    #[test]
    fn dimension_and_bucket_parse_config_strings() {
        assert_eq!(
            PartitionDimension::parse("entity-kind"),
            Some(PartitionDimension::EntityKind)
        );
        assert_eq!(
            PartitionDimension::parse("SOURCE_SCOPE"),
            Some(PartitionDimension::SourceScope)
        );
        assert_eq!(
            PartitionDimension::parse("composite").unwrap().as_str(),
            "composite"
        );
        assert_eq!(PartitionDimension::parse("nope"), None);
        assert_eq!(TimeBucket::Weekly.as_str(), "weekly");
    }

    /// RFC 0111 §3: `mark_cold_before` demotes aged partitions (evicting their handles), the tier
    /// survives a reopen, reads still return byte-identical data, and any read promotes a cold
    /// partition back to hot.
    #[test]
    fn aged_partitions_go_cold_evict_handles_and_rehydrate() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());

        let mut legacy = KirObject::new("legacy", ObjectKind::Table);
        legacy.created_at = "2026-01-15T00:00:00Z".parse().unwrap();
        ledger.append_object(&legacy).unwrap();
        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap(); // current month

        let old_key = ledger
            .catalog_partition_keys()
            .into_iter()
            .find(|k| k.time_bucket == "2026-01")
            .unwrap();
        assert_eq!(ledger.partition_tier(&old_key), Some(Tier::Hot));
        assert_eq!(ledger.partition_keys().len(), 2);

        // sweep: anything before 2026-06 goes cold
        let cutoff = "2026-06-01T00:00:00Z".parse().unwrap();
        assert_eq!(ledger.mark_cold_before(cutoff).unwrap(), 1);
        assert_eq!(ledger.mark_cold_before(cutoff).unwrap(), 0, "idempotent");
        assert_eq!(ledger.partition_tier(&old_key), Some(Tier::Cold));
        assert_eq!(ledger.cold_partition_keys(), vec![old_key.clone()]);
        assert_eq!(
            ledger.partition_keys().len(),
            1,
            "cold partition's open handle was evicted"
        );

        // read the cold partition → data intact, tier auto-promoted
        assert_eq!(
            ledger.get_object(&legacy.id).unwrap().unwrap().name,
            "legacy"
        );
        assert_eq!(ledger.partition_tier(&old_key), Some(Tier::Hot));

        // re-cold, drop, reopen — persisted tier survives
        ledger.mark_cold_before(cutoff).unwrap();
        assert!(dir.path().join("catalog.json").exists());
        drop(ledger);

        let reopened = ledger_with_root(dir.path());
        assert_eq!(reopened.partition_tier(&old_key), Some(Tier::Cold));
        // unscoped read is still complete and byte-identical across hot + cold
        let mut names: Vec<String> = reopened
            .objects_in_kind("Table")
            .unwrap()
            .into_iter()
            .map(|o| o.name)
            .collect();
        names.sort();
        assert_eq!(names, vec!["legacy".to_string(), "orders".to_string()]);
        // …and that read promoted the cold one
        assert_eq!(reopened.partition_tier(&old_key), Some(Tier::Hot));
    }

    /// RFC 0111 amendment §1: relationships route by `"rel:"+kind`, disjoint from object
    /// partitions; §2: `relationships_for` prunes to the endpoint's relationship partitions.
    #[test]
    fn relationships_route_by_kind_and_relationships_for_is_pruned() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());

        let a = KirObject::new("a", ObjectKind::Table);
        let b = KirObject::new("b", ObjectKind::Table);
        let c = KirObject::new("c", ObjectKind::File);
        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();
        ledger.append_object(&c).unwrap();

        let r_ab = rel(a.id, b.id, RelationshipKind::DependsOn);
        let r_bc = rel(b.id, c.id, RelationshipKind::Calls);
        ledger.append_relationship(&r_ab).unwrap();
        ledger.append_relationship(&r_bc).unwrap();

        // object + relationship partitions are disjoint; rel partitions are "rel:*"
        let rel_parts: Vec<String> = ledger
            .catalog_partition_keys()
            .into_iter()
            .map(|k| k.dimension_value)
            .filter(|d| d.starts_with("rel:"))
            .collect();
        let mut rel_parts = rel_parts;
        rel_parts.sort();
        assert_eq!(rel_parts, vec!["rel:Calls", "rel:DependsOn"]);
        assert!(ledger.objects_in_kind("rel:DependsOn").unwrap().is_empty());

        assert_eq!(ledger.relationship_count().unwrap(), 2);
        assert_eq!(
            ledger
                .get_relationship(&r_ab.id)
                .unwrap()
                .unwrap()
                .kind
                .to_string(),
            "DependsOn"
        );

        // relationships_for(b): both rels touch b → both, from exactly two partitions
        assert_eq!(ledger.relationships_for(&b.id).unwrap().len(), 2);
        // relationships_for(a): only r_ab → touches only the "rel:DependsOn" partition
        assert_eq!(ledger.relationships_for(&a.id).unwrap().len(), 1);
        assert_eq!(ledger.endpoint_sites(&a.id).unwrap().len(), 1);
        assert!(
            ledger.endpoint_sites(&a.id).unwrap().len() < ledger.catalog_partition_keys().len()
        );

        // history + reopen: the rel index persists → resolves with zero scans
        assert_eq!(ledger.relationship_history(&r_bc.id).unwrap().len(), 1);
        drop(ledger);

        let reopened = ledger_with_root(dir.path());
        assert!(reopened.partition_keys().is_empty());
        assert_eq!(reopened.relationships_for(&c.id).unwrap().len(), 1);
        assert_eq!(
            reopened.partition_keys().len(),
            1,
            "pruned to c's one rel partition, no scan"
        );
        assert_eq!(
            reopened.get_relationship(&r_ab.id).unwrap().unwrap().from,
            a.id
        );
    }

    /// `rebuild_entity_index` re-derives the relationship + endpoint index, not just objects.
    #[test]
    fn rebuild_also_repairs_the_relationship_index() {
        let dir = tempdir().unwrap();
        let x = KirId::new();
        let y = KirId::new();
        let r = rel(x, y, RelationshipKind::DependsOn);
        {
            let ledger = ledger_with_root(dir.path());
            ledger
                .append_object(&{
                    let mut o = KirObject::new("x", ObjectKind::Table);
                    o.id = x;
                    o
                })
                .unwrap();
            ledger.append_relationship(&r).unwrap();
        }
        // wipe the index dir entirely
        std::fs::remove_dir_all(dir.path().join("index")).unwrap();

        let reopened = ledger_with_root(dir.path());
        // index gone → relationships_for falls back to a scan, still correct
        assert_eq!(reopened.relationships_for(&x).unwrap().len(), 1);

        reopened.rebuild_entity_index().unwrap();
        // now served from the rebuilt index
        assert_eq!(reopened.get_relationship(&r.id).unwrap().unwrap().to, y);
        assert_eq!(reopened.relationships_for(&y).unwrap().len(), 1);
    }
}
