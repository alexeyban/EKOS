//! RFC 0111 Phase A: real, tested partitioning and fan-out for the whole knowledge model, keyed by
//! a configurable [`PartitionDimension`] + [`TimeBucket`], with a **persisted catalog and run-file
//! index** so partitions — and each id's partition set — survive process restarts.
//!
//! **Scope:** [`PartitionedLedger`] implements the full [`KnowledgeStore`] trait — objects,
//! relationships, events, evidence, point-in-time reads, full-text search, `diff`, `vacuum_into`,
//! counts — so it is a drop-in for [`FactLedger`]/`Ledger` (RFC 0111 amendment 2026-08-29). All
//! three `PartitionDimension`s route; `SourceScope`/`Composite` need a caller-supplied source
//! resolver ([`PartitionedLedger::with_source_resolver`]) because `KirObject` has no explicit
//! source field yet — a `None` under a source-based dimension is
//! [`PartitionError::UnresolvedSource`], never a silent misroute. `open_store` /
//! `open_store_read_only` (`crates/cli`) build this when `[storage.partition]` is enabled on a
//! fresh workspace; [`PartitionedLedger::read_only`] opens each partition via
//! [`FactLedger::open_read_only`].
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
//!   to an OS/power crash needs [`PartitionedLedger::rebuild_entity_index`] — it rebuilds
//!   obj/rel/endpoint from the partitions, but **not** evt/evid (`FactLedger` can't enumerate
//!   events/evidence); those recover only via the per-read self-healing scan.
//! - **`find_objects` skips cold partitions** (documented on the method) and merges per-partition
//!   BM25 — RFC §7's query-then-fetch approximation. **`diff`'s `added` entry-ids are
//!   per-partition-local** (concatenated, not globally unique); `touched`/`unchanged` merge
//!   cleanly. **Events all share one `"events"` partition per time bucket** (`EventKind` has no
//!   `Display`); fine — `KnowledgeStore` has no `events_for`/`all_events`.
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

use crate::{FactLedger, KnowledgeStore, LedgerDiff, LedgerError};

use chrono::{DateTime, Utc};
use ekos_kir::{KirEvent, KirEvidence, KirId, KirObject, KirRelationship};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

mod knowledge_store;
mod types;
pub use types::*;

/// Merge every index run into one once this many accumulate (`merge_runs`-style bound).
pub const COMPACT_AT: usize = 16;

/// The loaded in-memory index maps plus the on-disk run state:
/// `(maps, next unused run number, run files read)`.
type LoadedIndex = (IndexMaps, u32, Vec<PathBuf>);

/// Resolves a *new* partition's on-disk root from its key (container-config routing hook).
type RootResolver = Box<dyn Fn(&PartitionKey) -> PathBuf + Send + Sync>;
/// Resolves a `KirObject`'s originating source/connector for `SourceScope`/`Composite` routing.
type SourceResolver = Box<dyn Fn(&KirObject) -> Option<String> + Send + Sync>;

/// `id → partitions` cache entry. `complete` is true when the set is known to be the full
/// membership — either loaded from the persisted index on open, or filled in by a one-time catalog
/// scan (see the module doc's "Known limits").
#[derive(Default)]
struct Sites {
    partitions: BTreeSet<PartitionKey>,
    complete: bool,
}

/// What a run-file line's `id` refers to (RFC 0111 amendment 2026-08-29 §2/§3).
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
    /// event id → the `"events:"` partition it lives in
    Evt,
    /// evidence id → the `"evidence:"` partition it lives in
    Evid,
}

impl IndexKind {
    const ALL: [IndexKind; 5] = [
        IndexKind::Obj,
        IndexKind::Rel,
        IndexKind::Endpoint,
        IndexKind::Evt,
        IndexKind::Evid,
    ];
}

/// The `dimension_value` events and evidence route to (one partition per time bucket each).
const EVENTS_DV: &str = "events";
const EVIDENCE_DV: &str = "evidence";

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

/// An object partition holds `KirObject`s — anything that isn't a relationship, event, or evidence
/// partition.
fn is_object_partition(key: &PartitionKey) -> bool {
    !is_relationship_partition(key)
        && key.dimension_value != EVENTS_DV
        && key.dimension_value != EVIDENCE_DV
}

/// A filesystem-safe folder name for a partition (used by `vacuum_into`).
fn sanitize_key(key: &PartitionKey) -> String {
    format!("{}__{}", key.dimension_value, key.time_bucket).replace(['/', '\\', '\u{1f}', ':'], "_")
}

/// Copy the flat `index/` directory (only `run-*.jsonl` files) into `dst`.
fn copy_dir_shallow(src: &Path, dst: &Path) -> Result<(), PartitionError> {
    std::fs::create_dir_all(dst).map_err(|source| PartitionError::Catalog {
        path: dst.to_path_buf(),
        source,
    })?;
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                std::fs::copy(&p, dst.join(entry.file_name())).map_err(|source| {
                    PartitionError::Catalog {
                        path: p.clone(),
                        source,
                    }
                })?;
            }
        }
    }
    Ok(())
}
/// Wrap a `LedgerError` from a partition read where the exact key is not threaded through.
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

/// The `id → Sites` maps the run-file index resolves into, one per [`IndexKind`].
#[derive(Default)]
struct IndexMaps {
    obj: HashMap<KirId, Sites>,
    rel: HashMap<KirId, Sites>,
    endpoint: HashMap<KirId, Sites>,
    evt: HashMap<KirId, Sites>,
    evid: HashMap<KirId, Sites>,
}

impl IndexMaps {
    fn map(&self, k: IndexKind) -> &HashMap<KirId, Sites> {
        match k {
            IndexKind::Obj => &self.obj,
            IndexKind::Rel => &self.rel,
            IndexKind::Endpoint => &self.endpoint,
            IndexKind::Evt => &self.evt,
            IndexKind::Evid => &self.evid,
        }
    }
    fn map_mut(&mut self, k: IndexKind) -> &mut HashMap<KirId, Sites> {
        match k {
            IndexKind::Obj => &mut self.obj,
            IndexKind::Rel => &mut self.rel,
            IndexKind::Endpoint => &mut self.endpoint,
            IndexKind::Evt => &mut self.evt,
            IndexKind::Evid => &mut self.evid,
        }
    }
    fn is_empty(&self) -> bool {
        IndexKind::ALL.iter().all(|k| self.map(*k).is_empty())
    }
    fn mark_all_complete(&mut self) {
        for k in IndexKind::ALL {
            for s in self.map_mut(k).values_mut() {
                s.complete = true;
            }
        }
    }
}

/// Write the whole index to `run-<num>.jsonl` atomically (temp + rename), lines sorted by
/// `(kind, id, partition key)` — the AEVT-style ordering compaction/rebuild produce.
fn write_run(dir: &Path, num: u32, maps: &IndexMaps) -> Result<(), PartitionError> {
    let mut lines: Vec<(IndexKind, KirId, &PartitionKey)> = Vec::new();
    for k in IndexKind::ALL {
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
    /// event id → `"events"` partitions (`IndexKind::Evt`)
    evt_sites: Mutex<HashMap<KirId, Sites>>,
    /// evidence id → `"evidence"` partitions (`IndexKind::Evid`)
    evid_sites: Mutex<HashMap<KirId, Sites>>,
    index: Mutex<IndexWriter>,
    /// When set, each partition opens via [`FactLedger::open_read_only`] (RFC 0097) — never
    /// acquiring tantivy's exclusive writer lock, so a long-lived reader can't block a concurrent
    /// writer process. Writes on a read-only handle fail inside `FactLedger`.
    read_only: bool,
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
        if run_files.len() >= COMPACT_AT && !maps.is_empty() {
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
            evt_sites: Mutex::new(maps.evt),
            evid_sites: Mutex::new(maps.evid),
            index: Mutex::new(IndexWriter {
                next_run,
                current: None,
            }),
            read_only: false,
        })
    }

    /// Open for reads only — every partition opens via [`FactLedger::open_read_only`] (RFC 0097).
    /// Read paths never register new partitions, so nothing is written; a stray `append_*` on the
    /// returned handle fails inside `FactLedger`.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
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
                        maps.map_mut(l.k)
                            .entry(l.id)
                            .or_default()
                            .partitions
                            .insert(l.p);
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
        if keys.is_empty() || self.read_only {
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
        // A read-only handle never mutates the catalog (no registers, no tier promotion, no writes).
        let register = register && !self.read_only;
        if let Some(ledger) = self.open.lock().unwrap().get(key) {
            return Ok(Arc::clone(ledger));
        }
        let root = {
            let mut catalog = self.catalog.lock().unwrap();
            let mut changed = false;
            let root = match catalog.partitions.iter_mut().find(|e| &e.key == key) {
                Some(entry) => {
                    // RFC 0111 §3: any access to a cold partition promotes it back to hot.
                    if entry.tier == Tier::Cold && !self.read_only {
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
        let opened = if self.read_only {
            FactLedger::open_read_only(&root)
        } else {
            FactLedger::open(&root)
        };
        let ledger = Arc::new(opened.map_err(|source| PartitionError::Ledger {
            key: key.clone(),
            source,
        })?);
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
            IndexKind::Evt => &self.evt_sites,
            IndexKind::Evid => &self.evid_sites,
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

    // ── events & evidence (RFC 0111 amendment §3) ───────────────────────────

    fn events_key(&self, at: DateTime<Utc>) -> PartitionKey {
        PartitionKey {
            time_bucket: self.time_bucket.label(at),
            dimension_value: EVENTS_DV.to_string(),
        }
    }

    fn evidence_key(&self, at: DateTime<Utc>) -> PartitionKey {
        PartitionKey {
            time_bucket: self.time_bucket.label(at),
            dimension_value: EVIDENCE_DV.to_string(),
        }
    }

    /// Append an event to the `"events"` partition for its `occurred_at` bucket; index its id.
    pub fn append_event(&self, ev: &KirEvent) -> Result<(), PartitionError> {
        let key = self.events_key(ev.occurred_at);
        let ledger = self.partition(&key, true)?;
        ledger
            .append_event(ev)
            .map_err(|source| PartitionError::Ledger {
                key: key.clone(),
                source,
            })?;
        if insert_site(&self.evt_sites, ev.id, &key) {
            self.record(IndexKind::Evt, ev.id, std::slice::from_ref(&key))?;
        }
        Ok(())
    }

    /// Append evidence to the `"evidence"` partition for its `created_at` bucket; index its id.
    pub fn append_evidence(&self, ev: &KirEvidence) -> Result<(), PartitionError> {
        let key = self.evidence_key(ev.created_at);
        let ledger = self.partition(&key, true)?;
        ledger
            .append_evidence(ev)
            .map_err(|source| PartitionError::Ledger {
                key: key.clone(),
                source,
            })?;
        if insert_site(&self.evid_sites, ev.id, &key) {
            self.record(IndexKind::Evid, ev.id, std::slice::from_ref(&key))?;
        }
        Ok(())
    }

    fn evt_id_sites(&self, id: &KirId) -> Result<BTreeSet<PartitionKey>, PartitionError> {
        self.resolve_sites(
            IndexKind::Evt,
            id,
            || self.catalog_snapshot(Some(EVENTS_DV)),
            |ledger, id| Ok(ledger.get_event(id).map_err(ledger_err)?.is_some()),
        )
    }

    fn evid_id_sites(&self, id: &KirId) -> Result<BTreeSet<PartitionKey>, PartitionError> {
        self.resolve_sites(
            IndexKind::Evid,
            id,
            || self.catalog_snapshot(Some(EVIDENCE_DV)),
            |ledger, id| Ok(ledger.get_evidence(id).map_err(ledger_err)?.is_some()),
        )
    }

    pub fn get_event(&self, id: &KirId) -> Result<Option<KirEvent>, PartitionError> {
        let Some(newest) = self.evt_id_sites(id)?.into_iter().next_back() else {
            return Ok(None);
        };
        self.partition(&newest, false)?
            .get_event(id)
            .map_err(|source| PartitionError::Ledger {
                key: newest,
                source,
            })
    }

    pub fn get_evidence(&self, id: &KirId) -> Result<Option<KirEvidence>, PartitionError> {
        let Some(newest) = self.evid_id_sites(id)?.into_iter().next_back() else {
            return Ok(None);
        };
        self.partition(&newest, false)?
            .get_evidence(id)
            .map_err(|source| PartitionError::Ledger {
                key: newest,
                source,
            })
    }

    // ── point-in-time (RFC 0111 amendment §3) ───────────────────────────────

    /// The entity's state as the ledger knew it at `at`. Fan out to the entity's partitions
    /// newest→oldest; the first that has a version at or before `at` wins.
    pub fn object_at(
        &self,
        id: &KirId,
        at: DateTime<Utc>,
    ) -> Result<Option<KirObject>, PartitionError> {
        let keys: Vec<PartitionKey> = self.sites(id)?.into_iter().collect();
        for key in keys.into_iter().rev() {
            let ledger = self.partition(&key, false)?;
            if let Some(obj) =
                ledger
                    .object_at(id, at)
                    .map_err(|source| PartitionError::Ledger {
                        key: key.clone(),
                        source,
                    })?
            {
                return Ok(Some(obj));
            }
        }
        Ok(None)
    }

    /// Every object as it existed at or before `at`, one row per id.
    pub fn all_objects_at(&self, at: DateTime<Utc>) -> Result<Vec<KirObject>, PartitionError> {
        let mut rows = Vec::new();
        for (key, ledger) in self.catalog_snapshot_where(is_object_partition)? {
            let objs = ledger
                .all_objects_at(at)
                .map_err(|source| PartitionError::Ledger {
                    key: key.clone(),
                    source,
                })?;
            rows.push((key, objs));
        }
        Ok(Self::dedup_current(rows))
    }

    /// Relationships touching `id` as the ledger knew them at `at` — pruned to `id`'s relationship
    /// partitions.
    pub fn relationships_at(
        &self,
        id: &KirId,
        at: DateTime<Utc>,
    ) -> Result<Vec<KirRelationship>, PartitionError> {
        let mut out = Vec::new();
        for key in self.endpoint_sites(id)? {
            let ledger = self.partition(&key, false)?;
            out.extend(ledger.relationships_at(id, at).map_err(|source| {
                PartitionError::Ledger {
                    key: key.clone(),
                    source,
                }
            })?);
        }
        Ok(dedup_current_rels(out))
    }

    /// Every relationship as it existed at or before `at`.
    pub fn all_relationships_at(
        &self,
        at: DateTime<Utc>,
    ) -> Result<Vec<KirRelationship>, PartitionError> {
        let mut out = Vec::new();
        for (key, ledger) in self.catalog_snapshot_where(is_relationship_partition)? {
            out.extend(
                ledger
                    .all_relationships_at(at)
                    .map_err(|source| PartitionError::Ledger { key, source })?,
            );
        }
        Ok(dedup_current_rels(out))
    }

    // ── search, diff, vacuum, counts (RFC 0111 amendment §3) ────────────────

    /// Full-text object search fanned out across every **hot** object partition's tantivy index,
    /// results concatenated and deduplicated by id. Per-partition BM25 (RFC 0111 §7's
    /// query-then-fetch approximation); cold partitions are skipped — a query needing them must
    /// rehydrate first (touch them with another read).
    pub fn find_objects(&self, query: &str) -> Result<Vec<(KirId, String)>, PartitionError> {
        let hot: Vec<PartitionKey> = {
            let catalog = self.catalog.lock().unwrap();
            catalog
                .partitions
                .iter()
                .filter(|e| e.tier == Tier::Hot && is_object_partition(&e.key))
                .map(|e| e.key.clone())
                .collect()
        };
        let mut seen: std::collections::HashSet<KirId> = std::collections::HashSet::new();
        let mut out = Vec::new();
        for key in hot {
            let ledger = self.partition(&key, false)?;
            for (id, name) in
                ledger
                    .find_objects(query)
                    .map_err(|source| PartitionError::Ledger {
                        key: key.clone(),
                        source,
                    })?
            {
                if seen.insert(id) {
                    out.push((id, name));
                }
            }
        }
        Ok(out)
    }

    /// Total ledger entries across every partition.
    pub fn entry_count(&self) -> Result<usize, PartitionError> {
        let mut total = 0;
        for (key, ledger) in self.catalog_snapshot_where(|_| true)? {
            total += ledger
                .entry_count()
                .map_err(|source| PartitionError::Ledger { key, source })?;
        }
        Ok(total)
    }

    /// Merge per-partition [`LedgerDiff`]s over `(from, to]`. `added` entry-ids are per-partition
    /// local (concatenated); `touched` (logical ids) and `unchanged` merge cleanly.
    pub fn diff(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<LedgerDiff, PartitionError> {
        let mut merged = LedgerDiff {
            added: Vec::new(),
            touched: Vec::new(),
            unchanged: 0,
        };
        let mut touched: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (key, ledger) in self.catalog_snapshot_where(|_| true)? {
            let d = ledger
                .diff(from, to)
                .map_err(|source| PartitionError::Ledger { key, source })?;
            merged.added.extend(d.added);
            touched.extend(d.touched);
            merged.unchanged += d.unchanged;
        }
        merged.touched = touched.into_iter().collect();
        Ok(merged)
    }

    /// Write a self-contained branch copy of the whole partitioned ledger to `dest` — a fresh
    /// `catalog.json` (roots rewritten to `dest`), the `index/`, and every partition's `FactLedger`
    /// under `dest/p/<key>/`.
    pub fn vacuum_into(&self, dest: &Path) -> Result<(), PartitionError> {
        std::fs::create_dir_all(dest.join("p")).map_err(|source| PartitionError::Catalog {
            path: dest.to_path_buf(),
            source,
        })?;
        copy_dir_shallow(&self.index_dir, &dest.join("index"))?;

        let mut new_catalog = PartitionCatalog {
            dimension: Some(self.dimension.as_str().to_string()),
            time_bucket: Some(self.time_bucket.as_str().to_string()),
            partitions: Vec::new(),
        };
        for (key, ledger) in self.catalog_snapshot_where(|_| true)? {
            let sub = dest.join("p").join(sanitize_key(&key));
            ledger
                .vacuum_into(&sub)
                .map_err(|source| PartitionError::Ledger {
                    key: key.clone(),
                    source,
                })?;
            let tier = self.partition_tier(&key).unwrap_or(Tier::Hot);
            new_catalog.partitions.push(PartitionEntry {
                key,
                root: sub,
                tier,
            });
        }
        new_catalog.partitions.sort();
        let json = serde_json::to_vec_pretty(&new_catalog).map_err(|source| {
            PartitionError::CatalogParse {
                path: dest.to_path_buf(),
                source,
            }
        })?;
        std::fs::write(dest.join("catalog.json"), json).map_err(|source| {
            PartitionError::Catalog {
                path: dest.join("catalog.json"),
                source,
            }
        })?;
        Ok(())
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

    /// The on-disk root of a catalogued partition, or `None` if the key isn't in the catalog.
    /// Used to enumerate `(partition, location)` pairs when registering an existing Local-mode
    /// workspace's partitions with a Distributed-mode coordinator (RFC 0113 B4).
    pub fn partition_root(&self, key: &PartitionKey) -> Option<PathBuf> {
        self.catalog
            .lock()
            .unwrap()
            .partitions
            .iter()
            .find(|e| &e.key == key)
            .map(|e| e.root.clone())
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

/// `PartitionedLedger` is a drop-in [`KnowledgeStore`] (RFC 0111 amendment §4). `PartitionError`
/// maps to `LedgerError` via `From` (a wrapped `Ledger` error is unwrapped; anything else becomes
/// `Corrupt`).
#[cfg(test)]
mod tests;
