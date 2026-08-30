//! RFC 0016 Phases 4–6 — the fact engine behind the `Ledger` API.
//!
//! [`FactLedger`] exposes the same public surface as the SQLite [`Ledger`]
//! (`append_object` … `find_objects`, diff/merge/branching) implemented over
//! the fact model (Phase 1), segment batches (Phase 2), index runs
//! (Phase 3), tantivy search (Phase 5), and mmap'd sealed reads (Phase 6).
//!
//! Read architecture (Phase 6): committed history lives in the segments;
//! entries up to the `indexes/last_tx` marker are served from the on-disk
//! EAVT/AEVT/AVET runs, and the (bounded) remainder — everything since the
//! last seal — lives in an in-memory **memtable**. A seal flushes the
//! memtable into new runs and advances the marker, so `open` replays only
//! the post-seal tail instead of the whole ledger: frame *headers* are
//! walked for the time→tx map, but batch bodies before the marker are never
//! decompressed.
//!
//! Entity typing (object / relationship / evidence / event) derives from
//! payload shape (`from`+`to`, `fragment`, `subject`) — deterministic, and
//! exactly the information the SQLite `entry_type` column carried. Time
//! travel maps wall time to the greatest `tx` at or before it (RFC 0016
//! §2); `tx` is the ordering authority, so same-microsecond appends can
//! never produce an ambiguous history.

use chrono::{DateTime, Utc};
use ekos_kir::{KirEvent, KirEvidence, KirId, KirObject, KirRelationship};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ekos_segment_backend::SegmentBackend;
use uuid::Uuid;

use crate::fact::{AttrId, Fact, FactOp, FactValue, TxId, decompose, diff, reconstruct};
use crate::index::{FactIndexes, IndexEntry, ScanPrefix, entries_from_batches};
use crate::search::SearchIndex;
use crate::segment::{SEGMENT_SEAL_BYTES, SegmentError, SegmentStore};
use crate::{
    LedgerDiff, LedgerEntryId, LedgerError, MergeConflict, MergeReport, content_signature,
};

impl From<SegmentError> for LedgerError {
    fn from(e: SegmentError) -> Self {
        match e {
            SegmentError::Io(io) => LedgerError::Io(io),
            SegmentError::Json(j) => LedgerError::Json(j),
            SegmentError::Corrupt(msg) => LedgerError::Corrupt(msg),
            SegmentError::Backend(b) => LedgerError::Corrupt(b.to_string()),
        }
    }
}

/// What a payload's shape says the entity is — the fact engine's equivalent
/// of the SQLite `entry_type` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntityKind {
    Object,
    Relationship,
    Evidence,
    Event,
}

fn kind_of_payload(payload: &serde_json::Value) -> EntityKind {
    let has = |k: &str| payload.get(k).is_some();
    if has("from") && has("to") {
        EntityKind::Relationship
    } else if has("fragment") {
        EntityKind::Evidence
    } else if has("subject") {
        EntityKind::Event
    } else {
        EntityKind::Object
    }
}

/// Compact runs once more than this many accumulate per sort order.
const MERGE_RUNS_AT: usize = 8;

/// Write a new checkpoint (RFC 0106 Phase 3) once an entity has accumulated this many real
/// versions since its last one — small enough that a long-lived entity's fold cost stays bounded,
/// large enough that checkpoint writes (each paying one real full-history fold) stay rare.
const CHECKPOINT_INTERVAL: usize = 20;

/// Where checkpoints are persisted — a plain JSON-Lines file, deliberately *not* named or shaped
/// like the unrelated, already-existing `.ekos/snapshots/*.json.zst` build-index mechanism (RFC
/// 0080's own investigation flagged the naming collision risk explicitly). No segment-grade
/// fsync/atomic-rename durability: correctness never depends on this file (see
/// [`load_checkpoints`]'s own doc comment), so a lost or torn write only costs a slower fold on
/// the next read, never a wrong one.
fn checkpoints_path(root: &Path) -> PathBuf {
    root.join("checkpoints.jsonl")
}

/// One persisted checkpoint line — `(entity, tx, facts)`.
#[derive(Serialize, Deserialize)]
struct CheckpointRecord {
    entity: Uuid,
    tx: TxId,
    facts: Vec<Fact>,
}

/// Load every checkpoint ever written for this ledger, grouped by entity and sorted by `tx`.
/// Missing file → empty map (every pre-RFC-0106 workspace, and the common case of an entity that
/// has never crossed [`CHECKPOINT_INTERVAL`]) — not an error, the same "derived, optional,
/// additive" contract this whole feature is built on. A line that fails to parse (most plausibly
/// a torn trailing line from a write interrupted by a crash) is silently skipped, not treated as
/// corruption — checkpoints are never a source of truth, only a shortcut, so a bad one just means
/// one fewer shortcut is available, not a wrong answer.
fn load_checkpoints(root: &Path) -> HashMap<Uuid, BTreeMap<TxId, Vec<Fact>>> {
    let mut out: HashMap<Uuid, BTreeMap<TxId, Vec<Fact>>> = HashMap::new();
    let Ok(content) = std::fs::read_to_string(checkpoints_path(root)) else {
        return out;
    };
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<CheckpointRecord>(line) {
            out.entry(record.entity)
                .or_default()
                .insert(record.tx, record.facts);
        }
    }
    out
}

/// Append one checkpoint record. Best-effort: an I/O error here is real but never a correctness
/// problem (see [`load_checkpoints`]'s own doc comment) — propagated to the caller anyway rather
/// than silently swallowed, since a real, unexpected I/O failure is worth surfacing even though
/// this feature could technically continue without it.
fn append_checkpoint(
    root: &Path,
    entity: Uuid,
    tx: TxId,
    facts: &[Fact],
) -> Result<(), LedgerError> {
    let record = CheckpointRecord {
        entity,
        tx,
        facts: facts.to_vec(),
    };
    let mut line = serde_json::to_string(&record)?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(checkpoints_path(root))
        .map_err(LedgerError::Io)?;
    use std::io::Write;
    file.write_all(line.as_bytes()).map_err(LedgerError::Io)?;
    Ok(())
}

struct Inner {
    store: SegmentStore,
    /// On-disk index runs covering all batches with `tx ≤ runs_marker`.
    runs: FactIndexes,
    runs_marker: Option<TxId>,
    /// Entries past the marker (everything since the last seal) — bounded by
    /// the seal threshold.
    memtable: Vec<IndexEntry>,
    /// (tx, wall_time_us) per committed batch, tx-ordered — the time→tx map,
    /// rebuilt from frame headers only.
    batch_times: Vec<(TxId, i64)>,
    /// Current content signatures, filled lazily (idempotence checks).
    sig_cache: HashMap<Uuid, String>,
    /// Tantivy object search (Phase 5): buffered upserts, lazy group commit.
    search: SearchIndex,
    /// RFC 0097: `true` when opened via [`FactLedger::open_read_only`] —
    /// checked once, up front, in `append_inner` so every write path
    /// (`append_object`/`append_evidence`/`append_relationship`/
    /// `append_event`) fails loudly with `LedgerError::ReadOnly` rather than
    /// reaching `SearchIndex`'s already-silent no-op writer guard.
    read_only: bool,
    /// RFC 0106 Phase 3: per-entity periodic full-state checkpoints, tx-sorted — a pure
    /// acceleration structure for [`Inner::state_at`], never consulted for correctness (see
    /// [`checkpoints_path`]'s own doc comment). Loaded once at open.
    checkpoints: HashMap<Uuid, BTreeMap<TxId, Vec<Fact>>>,
    /// The `FactLedger`'s own root — where [`checkpoints_path`] writes new checkpoints.
    checkpoints_root: PathBuf,
}

/// A real, designed cross-process write lock (RFC 0104 Phase 1) — a dedicated `write.lock` file
/// acquired via `fs4`'s `flock`(2)-backed exclusive lock (the same mechanism tantivy's own
/// `IndexWriter` lock already uses internally, promoted here to a direct, first-class dependency),
/// held for the whole writable [`FactLedger`] handle's lifetime and released automatically by the
/// OS when the returned `File` drops — including on a crash, so there is no separate cleanup step.
///
/// Before this, single-writer exclusion was only an *incidental* side effect of
/// [`SearchIndex`]'s own tantivy `IndexWriter` lock (still acquired too — a redundant second
/// safety net now, not the sole mechanism, and not removed here). That incidental lock is deep
/// inside `SearchIndex::open`, several steps into a writable open; a second writable process would
/// only discover the conflict there, as a tantivy-internal `LockBusy` error. This lock is acquired
/// first, before `SegmentStore`/`SearchIndex` are touched at all, so the failure is immediate and
/// named at the ledger's own level ([`LedgerError::Locked`]).
fn acquire_write_lock(root: &Path) -> Result<std::fs::File, LedgerError> {
    use fs4::FileExt;
    let path = root.join("write.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(LedgerError::Io)?;
    file.try_lock_exclusive().map_err(|_| {
        LedgerError::Locked(format!(
            "another writable process already holds the ledger's write lock at {} — only one \
             writable ekos process (build/recover/resolve/compile/commit, or `ekos mcp serve` \
             without --read-only) may run against this workspace at a time",
            path.display()
        ))
    })?;
    Ok(file)
}

/// The fact-segment ledger — RFC 0016's replacement for the SQLite backend,
/// behind the same API shape (`&self` methods, like `Ledger`).
pub struct FactLedger {
    inner: Mutex<Inner>,
    root: PathBuf,
    /// `Some` for a writable handle (RFC 0104 Phase 1) — held only for its `Drop` side effect,
    /// never read. `None` for a read-only handle, which must never hold this (see
    /// [`Self::open_read_only`]'s own doc comment for why).
    _write_lock: Option<std::fs::File>,
}

impl FactLedger {
    /// Open (or create) a fact ledger rooted at `root` (a directory).
    pub fn open(root: &Path) -> Result<Self, LedgerError> {
        Self::open_with_seal_threshold(root, SEGMENT_SEAL_BYTES)
    }

    /// `open`, but sealed segments are published to / fetched from `backend` (RFC 0113 B1/B4) —
    /// e.g. an `ObjectStoreBackend` so a partition's bulk (its 8 MB sealed segments) lives in
    /// object storage. `root` still holds the small local working state: the active/unsealed
    /// segment, `HEAD`, `manifest.json`, `dict.bin`, and the `search/` index.
    pub fn open_with_backend(
        root: &Path,
        backend: Arc<dyn SegmentBackend>,
    ) -> Result<Self, LedgerError> {
        Self::open_writable(root, SEGMENT_SEAL_BYTES, Some(backend))
    }

    /// [`Self::open_with_backend`] with a custom seal threshold (tests exercise the seal → publish
    /// path without writing 8 MB).
    pub fn open_with_backend_and_seal_threshold(
        root: &Path,
        backend: Arc<dyn SegmentBackend>,
        seal_bytes: u64,
    ) -> Result<Self, LedgerError> {
        Self::open_writable(root, seal_bytes, Some(backend))
    }

    /// `open` with a custom segment seal threshold (tests exercise the
    /// seal → run-flush path without writing megabytes).
    pub fn open_with_seal_threshold(root: &Path, seal_bytes: u64) -> Result<Self, LedgerError> {
        Self::open_writable(root, seal_bytes, None)
    }

    fn open_writable(
        root: &Path,
        seal_bytes: u64,
        backend: Option<Arc<dyn SegmentBackend>>,
    ) -> Result<Self, LedgerError> {
        std::fs::create_dir_all(root).map_err(LedgerError::Io)?;
        let write_lock = acquire_write_lock(root)?;
        let store = match backend {
            Some(b) => SegmentStore::open_with_backend(root, b, seal_bytes)?,
            None => SegmentStore::open_with_seal_threshold(root, seal_bytes)?,
        };
        let (mut runs, runs_clean) = FactIndexes::open(root.join("indexes"))?;
        let mut runs_marker = std::fs::read_to_string(root.join("indexes/last_tx"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(TxId);
        if !runs_clean {
            // Self-heal: some runs were unreadable (format upgrade). Runs are
            // derived — drop them all and rebuild via the memtable path.
            for order in crate::index::SortOrder::ALL {
                let _ = runs.merge_runs(order); // no-op ≤1 run; normalizes state
            }
            let _ = std::fs::remove_dir_all(root.join("indexes"));
            let (fresh, _) = FactIndexes::open(root.join("indexes"))?;
            runs = fresh;
            runs_marker = None;
        }
        let (search, search_marker) = SearchIndex::open(&root.join("search"))?;

        let mut inner = Inner {
            batch_times: store.batch_headers()?,
            memtable: entries_from_batches(&store.batches_after(runs_marker)?),
            store,
            runs,
            runs_marker,
            sig_cache: HashMap::new(),
            search,
            read_only: false,
            checkpoints: load_checkpoints(root),
            checkpoints_root: root.to_path_buf(),
        };

        // Catch the search index up: entities committed past its marker get
        // their current state re-indexed (bounded decode, usually ≈ memtable).
        let stale: HashSet<Uuid> = inner
            .store
            .batches_after(search_marker)?
            .iter()
            .flat_map(|b| b.ops.iter().map(|(_, f)| f.entity))
            .collect();
        for id in stale {
            if let Some(payload) = inner.reconstruct_at(id, TxId(u64::MAX))?
                && kind_of_payload(&payload) == EntityKind::Object
            {
                inner.index_object(id, &payload);
            }
        }
        let last_tx = inner.batch_times.last().map(|(t, _)| *t);
        inner.search.commit(last_tx)?;

        Ok(Self {
            inner: Mutex::new(inner),
            root: root.to_path_buf(),
            _write_lock: Some(write_lock),
        })
    }

    /// Open an **existing** fact ledger at `root` for reads only (RFC 0097).
    ///
    /// Unlike [`Self::open`]/[`Self::open_with_seal_threshold`], this never acquires the real,
    /// designed cross-process write lock ([`acquire_write_lock`], RFC 0104 Phase 1) or tantivy's
    /// exclusive `IndexWriter` lock ([`SearchIndex::open_read_only`]), and never re-indexes/commits
    /// the search index on open — all of that is write-path work a concurrent real writer (a
    /// separate `ekos build`/`commit` process) must stay free to do at any time, including while
    /// this handle stays open. Meant for a long-lived caller (e.g. `ekos mcp serve`) that wants to
    /// reuse one open handle across many reads without blocking a concurrent writer the way holding
    /// a normal, writable handle open indefinitely would.
    ///
    /// **Known limitations, not bugs, both spec'd precisely rather than assumed (RFC 0104 Phase
    /// 1)**:
    /// - Because the search-index catchup step is skipped, `find_objects` (bm25 search)
    ///   specifically may not reflect objects committed by a separate writer *after* this
    ///   read-only handle's `search/` index was last written by some write-capable process.
    /// - **More broadly, every read on *any* handle — writable or read-only — reflects the
    ///   ledger's on-disk state as of this handle's own `open()` call (plus whatever this same
    ///   handle has itself appended since), not automatically refreshed by a separate process's
    ///   concurrent writes.** `Inner`'s `memtable`/`SegmentStore::head.committed_len` are loaded
    ///   once at open and advanced only by this handle's own writes — nothing re-reads the on-disk
    ///   head segment on each query. This is a real, material difference from SQLite's WAL mode,
    ///   where a *new read transaction* on an already-open connection sees the latest committed
    ///   state without reopening the connection object — no such automatic refresh exists here. A
    ///   caller needing fresh cross-process visibility must re-open the handle.
    ///
    /// Fails with [`LedgerError::NotFound`] if `root` doesn't exist yet — a
    /// read-only open never creates a fresh store the way a writable one
    /// does for a genuinely new workspace. Every write method
    /// (`append_object`, …) fails with [`LedgerError::ReadOnly`] on the
    /// result.
    pub fn open_read_only(root: &Path) -> Result<Self, LedgerError> {
        Self::open_ro(root, None)
    }

    /// [`Self::open_read_only`] with sealed segments served from `backend` (RFC 0113 B4) — the read
    /// path a query worker uses for a partition whose bulk lives in object storage.
    pub fn open_read_only_with_backend(
        root: &Path,
        backend: Arc<dyn SegmentBackend>,
    ) -> Result<Self, LedgerError> {
        Self::open_ro(root, Some(backend))
    }

    fn open_ro(root: &Path, backend: Option<Arc<dyn SegmentBackend>>) -> Result<Self, LedgerError> {
        if !root.exists() {
            return Err(LedgerError::NotFound(root.display().to_string()));
        }
        let store = match backend {
            Some(b) => SegmentStore::open_with_backend(root, b, SEGMENT_SEAL_BYTES)?,
            None => SegmentStore::open_with_seal_threshold(root, SEGMENT_SEAL_BYTES)?,
        };
        let (runs, runs_clean) = FactIndexes::open(root.join("indexes"))?;
        if !runs_clean {
            return Err(LedgerError::Corrupt(
                "index runs need rebuilding, which a read-only open cannot do — open writable \
                 (e.g. `ekos build`) once to self-heal, then reopen read-only"
                    .to_string(),
            ));
        }
        let runs_marker = std::fs::read_to_string(root.join("indexes/last_tx"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(TxId);
        let (search, _search_marker) = SearchIndex::open_read_only(&root.join("search"))?;

        let inner = Inner {
            batch_times: store.batch_headers()?,
            memtable: entries_from_batches(&store.batches_after(runs_marker)?),
            store,
            runs,
            runs_marker,
            sig_cache: HashMap::new(),
            search,
            read_only: true,
            checkpoints: load_checkpoints(root),
            checkpoints_root: root.to_path_buf(),
        };

        Ok(Self {
            inner: Mutex::new(inner),
            root: root.to_path_buf(),
            _write_lock: None,
        })
    }

    // ── Append methods (same semantics as the SQLite backend) ─────────────

    /// Write a KirObject. Idempotent by content signature; returns `true`
    /// when a new version was recorded.
    pub fn append_object(&self, obj: &KirObject) -> Result<bool, LedgerError> {
        self.append_payload(obj.id.0, serde_json::to_value(obj)?)
    }

    /// Write a KirEvidence. Idempotent.
    pub fn append_evidence(&self, ev: &KirEvidence) -> Result<(), LedgerError> {
        self.append_payload(ev.id.0, serde_json::to_value(ev)?)?;
        Ok(())
    }

    /// Write a KirEvent. Immutable log entry (RFC 0029: the first real write
    /// path for events — `EntityKind::Event`/`kind_of_payload`'s `"subject"`
    /// dispatch already existed, only this wrapper was missing).
    pub fn append_event(&self, ev: &KirEvent) -> Result<(), LedgerError> {
        self.append_payload(ev.id.0, serde_json::to_value(ev)?)?;
        Ok(())
    }

    /// Write a KirRelationship. Returns `true` when a new version was recorded.
    pub fn append_relationship(&self, rel: &KirRelationship) -> Result<bool, LedgerError> {
        self.append_payload(rel.id.0, serde_json::to_value(rel)?)
    }

    fn append_payload(
        &self,
        entity: Uuid,
        payload: serde_json::Value,
    ) -> Result<bool, LedgerError> {
        self.append_inner(entity, payload, None)
    }

    /// Migration entry point: append one historical version with its
    /// **original** commit timestamp (RFC 0016 §8).
    pub(crate) fn append_version(
        &self,
        entity: Uuid,
        payload: serde_json::Value,
        wall_us: i64,
    ) -> Result<bool, LedgerError> {
        self.append_inner(entity, payload, Some(wall_us))
    }

    /// Current content signature of an entity (migration verification).
    pub(crate) fn current_signature(&self, entity: Uuid) -> Result<Option<String>, LedgerError> {
        self.inner.lock().unwrap().current_sig(entity)
    }

    /// Install the segment-body compression dictionary (RFC 0016 §7);
    /// migration calls this on the empty store before the first append.
    pub(crate) fn set_segment_dictionary(&self, bytes: Vec<u8>) -> Result<(), LedgerError> {
        self.inner
            .lock()
            .unwrap()
            .store
            .set_dictionary(bytes)
            .map_err(Into::into)
    }

    /// Seal the active segment and flush indexes — called at the end of a
    /// migration so the new store opens fast.
    pub(crate) fn seal_and_flush(&self) -> Result<(), LedgerError> {
        let mut inner = self.inner.lock().unwrap();
        inner.store.seal_active()?;
        if let Some((tx, _)) = inner.batch_times.last().copied() {
            inner.flush_memtable(tx)?;
            inner.search.commit(Some(tx))?;
        }
        Ok(())
    }

    fn append_inner(
        &self,
        entity: Uuid,
        payload: serde_json::Value,
        wall_override_us: Option<i64>,
    ) -> Result<bool, LedgerError> {
        let mut inner = self.inner.lock().unwrap();
        if inner.read_only {
            return Err(LedgerError::ReadOnly(self.root.display().to_string()));
        }
        let sig = content_signature(&payload);
        if inner.current_sig(entity)?.as_ref() == Some(&sig) {
            return Ok(false); // logically identical — no new version
        }

        let attrs_before = inner.store.manifest.attributes.len();
        let new_facts = decompose(entity, &payload, &mut inner.store.manifest.attributes)
            .map_err(|e| LedgerError::Corrupt(e.to_string()))?;
        // New attribute paths must be durable before any fact referencing
        // them — the registry lives in the manifest.
        if inner.store.manifest.attributes.len() > attrs_before {
            inner.store.persist_manifest()?;
        }
        let old_facts = inner.state_at(entity, None)?;
        let ops = diff(&old_facts, &new_facts);

        let wall = wall_override_us.unwrap_or_else(|| Utc::now().timestamp_micros());
        let (tx, sealed) = inner.store.append_with_seal(ops.clone(), wall)?;
        inner.batch_times.push((tx, wall));
        for (op, fact) in &ops {
            inner.memtable.push(IndexEntry::from_fact(fact, tx, *op));
        }
        inner.sig_cache.insert(entity, sig);
        if kind_of_payload(&payload) == EntityKind::Object {
            inner.index_object(entity, &payload);
        }
        if sealed {
            inner.flush_memtable(tx)?;
        }
        inner.maybe_checkpoint(entity, tx)?;
        Ok(true)
    }

    // ── Reads — current state ─────────────────────────────────────────────

    /// Retrieve the current state of a KirObject by id.
    pub fn get_object(&self, id: &KirId) -> Result<Option<KirObject>, LedgerError> {
        self.typed_current(id.0, EntityKind::Object)
    }

    /// Retrieve a KirEvidence by id.
    pub fn get_evidence(&self, id: &KirId) -> Result<Option<KirEvidence>, LedgerError> {
        self.typed_current(id.0, EntityKind::Evidence)
    }

    /// Retrieve a KirEvent by id.
    pub fn get_event(&self, id: &KirId) -> Result<Option<KirEvent>, LedgerError> {
        self.typed_current(id.0, EntityKind::Event)
    }

    /// Retrieve a KirRelationship by id.
    pub fn get_relationship(&self, id: &KirId) -> Result<Option<KirRelationship>, LedgerError> {
        self.typed_current(id.0, EntityKind::Relationship)
    }

    fn typed_current<T: serde::de::DeserializeOwned>(
        &self,
        id: Uuid,
        kind: EntityKind,
    ) -> Result<Option<T>, LedgerError> {
        let inner = self.inner.lock().unwrap();
        match inner.reconstruct_at(id, TxId(u64::MAX))? {
            Some(payload) if kind_of_payload(&payload) == kind => {
                Ok(Some(serde_json::from_value(payload)?))
            }
            _ => Ok(None),
        }
    }

    /// All objects currently tracked — one sequential EAVT pass, not a
    /// point scan per entity.
    pub fn all_objects(&self) -> Result<Vec<KirObject>, LedgerError> {
        self.all_of_kind(EntityKind::Object)
    }

    /// All relationships currently tracked.
    pub fn all_relationships(&self) -> Result<Vec<KirRelationship>, LedgerError> {
        self.all_of_kind(EntityKind::Relationship)
    }

    fn all_of_kind<T: serde::de::DeserializeOwned>(
        &self,
        kind: EntityKind,
    ) -> Result<Vec<T>, LedgerError> {
        let inner = self.inner.lock().unwrap();
        let mut out = Vec::new();
        for (_, payload) in inner.all_current_payloads()? {
            if kind_of_payload(&payload) == kind {
                out.push(serde_json::from_value(payload)?);
            }
        }
        Ok(out)
    }

    /// All relationships where `from` or `to` equals `id` — AVET ranged
    /// scans instead of a reverse-index table (RFC 0016 §4).
    pub fn relationships_for(&self, id: &KirId) -> Result<Vec<KirRelationship>, LedgerError> {
        let inner = self.inner.lock().unwrap();
        let mut out = Vec::new();
        for rel in inner.relationship_candidates(id.0)? {
            if let Some(payload) = inner.reconstruct_at(rel, TxId(u64::MAX))? {
                let touches = ["from", "to"].iter().any(|side| {
                    payload.get(side).and_then(|v| v.as_str()) == Some(id.to_string()).as_deref()
                });
                if touches && kind_of_payload(&payload) == EntityKind::Relationship {
                    out.push(serde_json::from_value(payload)?);
                }
            }
        }
        Ok(out)
    }

    // ── Reads — historical state ──────────────────────────────────────────

    /// The object as it was at or before `at` (true multi-version history).
    pub fn object_at(
        &self,
        id: &KirId,
        at: DateTime<Utc>,
    ) -> Result<Option<KirObject>, LedgerError> {
        let inner = self.inner.lock().unwrap();
        let Some(cut) = inner.tx_at(at) else {
            return Ok(None);
        };
        match inner.reconstruct_at(id.0, cut)? {
            Some(payload) if kind_of_payload(&payload) == EntityKind::Object => {
                Ok(Some(serde_json::from_value(payload)?))
            }
            _ => Ok(None),
        }
    }

    /// Relationships involving `id`, each reconstructed as it actually was
    /// *at* `at` — true multi-version history, the same guarantee
    /// `object_at` already gives (RFC 0054: found necessary once `Replay`
    /// needed correct historical reconstruction across relationships
    /// updated more than once; the SQLite backend's own `relationships_at`
    /// had the identical bug, fixed the same way, in the same RFC). This
    /// used to filter on whether a relationship's *current* version's tx was
    /// at-or-before the cut, then reconstruct its *latest* state regardless
    /// (`RFC 0011`, "kept for parity" with a since-fixed SQLite limitation)
    /// — silently excluding, or showing the wrong version of, any
    /// relationship updated after `at`. `reconstruct_at(rel, cut)` mirrors
    /// `object_at`'s own call exactly: `None` when the entity had no facts
    /// yet by `cut`, the historically-correct snapshot otherwise.
    pub fn relationships_at(
        &self,
        id: &KirId,
        at: DateTime<Utc>,
    ) -> Result<Vec<KirRelationship>, LedgerError> {
        let inner = self.inner.lock().unwrap();
        let Some(cut) = inner.tx_at(at) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for rel in inner.relationship_candidates(id.0)? {
            if let Some(payload) = inner.reconstruct_at(rel, cut)?
                && kind_of_payload(&payload) == EntityKind::Relationship
            {
                out.push(serde_json::from_value(payload)?);
            }
        }
        Ok(out)
    }

    /// Bulk counterpart to `object_at` (RFC 0096) — every object as it
    /// existed at or before `at`, via one `all_payloads_at` pass instead of
    /// a per-id `reconstruct_at` scan.
    pub fn all_objects_at(&self, at: DateTime<Utc>) -> Result<Vec<KirObject>, LedgerError> {
        let inner = self.inner.lock().unwrap();
        let Some(cut) = inner.tx_at(at) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for (_, payload) in inner.all_payloads_at(Some(cut))? {
            if kind_of_payload(&payload) == EntityKind::Object {
                out.push(serde_json::from_value(payload)?);
            }
        }
        Ok(out)
    }

    /// Bulk counterpart to `relationships_at` (RFC 0096) — every
    /// relationship as it existed at or before `at`.
    pub fn all_relationships_at(
        &self,
        at: DateTime<Utc>,
    ) -> Result<Vec<KirRelationship>, LedgerError> {
        let inner = self.inner.lock().unwrap();
        let Some(cut) = inner.tx_at(at) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for (_, payload) in inner.all_payloads_at(Some(cut))? {
            if kind_of_payload(&payload) == EntityKind::Relationship {
                out.push(serde_json::from_value(payload)?);
            }
        }
        Ok(out)
    }

    /// Every historical version of the object at `id`, oldest to newest
    /// (RFC 0047).
    pub fn object_history(&self, id: &KirId) -> Result<Vec<KirObject>, LedgerError> {
        let inner = self.inner.lock().unwrap();
        inner
            .entity_history(id.0)?
            .into_iter()
            .filter(|p| kind_of_payload(p) == EntityKind::Object)
            .map(|p| serde_json::from_value(p).map_err(Into::into))
            .collect()
    }

    /// Every historical version of the relationship at `id`, oldest to
    /// newest (RFC 0047) — scoped to the relationship's own id, not every
    /// relationship touching it as an endpoint (that's
    /// `relationship_candidates`/`relationships_at`'s job).
    pub fn relationship_history(&self, id: &KirId) -> Result<Vec<KirRelationship>, LedgerError> {
        let inner = self.inner.lock().unwrap();
        inner
            .entity_history(id.0)?
            .into_iter()
            .filter(|p| kind_of_payload(p) == EntityKind::Relationship)
            .map(|p| serde_json::from_value(p).map_err(Into::into))
            .collect()
    }

    // ── Search (tantivy, RFC 0016 Phase 5) ────────────────────────────────

    /// Ranked BM25 search over object names, kinds, and content excerpts.
    /// Terms are ANDed; a trailing `*` prefix-matches a token; name hits
    /// outrank kind hits outrank content hits (10/4/1 boosts, as RFC 0014
    /// tuned). Buffered upserts group-commit here — read-your-writes without
    /// per-append commit cost.
    pub fn find_objects(&self, query: &str) -> Result<Vec<(KirId, String)>, LedgerError> {
        Ok(self
            .find_objects_scored(query, 50)?
            .into_iter()
            .map(|(id, name, _)| (id, name))
            .collect())
    }

    /// Like [`FactLedger::find_objects`], but bounded to `limit` hits and each carries its raw
    /// BM25 score. The Distributed-mode gateway (RFC 0113 B5) fans this to every shard and
    /// merge-sorts the per-shard top-K lists; the scores are shard-local, the accepted
    /// query-then-fetch approximation.
    pub fn find_objects_scored(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(KirId, String, f32)>, LedgerError> {
        let mut inner = self.inner.lock().unwrap();
        let last_tx = inner.batch_times.last().map(|(t, _)| *t);
        inner.search.commit(last_tx)?;
        let hits = inner.search.query_scored(query, limit)?;
        Ok(hits
            .into_iter()
            .map(|(id, name, score)| (KirId(id), name, score))
            .collect())
    }

    // ── Counters ──────────────────────────────────────────────────────────

    /// Total version count (committed batches) — mirrors the SQLite
    /// backend's `entries` row count.
    pub fn entry_count(&self) -> Result<usize, LedgerError> {
        Ok(self.inner.lock().unwrap().batch_times.len())
    }

    /// Number of distinct objects currently tracked — one AEVT scan over
    /// the `name` attribute (objects are the only entities carrying it).
    /// Like the SQLite backend's pointer tables, the count never shrinks on
    /// retraction; no reconstruction happens here.
    pub fn object_count(&self) -> Result<usize, LedgerError> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.entities_with_attr("name")?.len())
    }

    /// Number of distinct relationships currently tracked (AEVT over `from`).
    pub fn relationship_count(&self) -> Result<usize, LedgerError> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.entities_with_attr("from")?.len())
    }

    /// Real repair-tool report (RFC 0105 Phase 2): one row per sealed segment, checked
    /// unconditionally against its manifest hash — see
    /// [`crate::segment::SegmentStore::verify_sealed_report`] for what `ok: false` does and
    /// doesn't mean.
    pub fn verify_sealed_report(&self) -> Vec<crate::segment::SealedSegmentCheck> {
        self.inner.lock().unwrap().store.verify_sealed_report()
    }

    // ── Branching / diff / merge ──────────────────────────────────────────

    /// Write a complete copy of this ledger to `dest` (a directory) — the
    /// branch operation. O(1) manifest sharing arrives with the backend
    /// swap; for parity this is a verified file copy of sealed state.
    pub fn vacuum_into(&self, dest: &Path) -> Result<(), LedgerError> {
        let mut inner = self.inner.lock().unwrap();
        // Flush buffered search upserts so the copy is self-consistent.
        let last_tx = inner.batch_times.last().map(|(t, _)| *t);
        inner.search.commit(last_tx)?;
        copy_dir(&self.root, dest)?;
        drop(inner);
        FactLedger::open(dest).map(|_| ())
    }

    /// Object/relationship versions committed in `(from, to]` — the fact
    /// engine's `diff_ledger`.
    pub fn diff(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<LedgerDiff, LedgerError> {
        let inner = self.inner.lock().unwrap();
        let from_us = from.timestamp_micros();
        let to_us = to.timestamp_micros();

        let window_start = inner
            .batch_times
            .iter()
            .rev()
            .find(|(_, w)| *w <= from_us)
            .map(|(t, _)| *t);
        let in_window: HashSet<TxId> = inner
            .batch_times
            .iter()
            .filter(|(_, w)| *w > from_us && *w <= to_us)
            .map(|(t, _)| *t)
            .collect();

        let mut added = Vec::new();
        let mut touched_ids = HashSet::new();
        for batch in inner.store.batches_after(window_start)? {
            if !in_window.contains(&batch.tx) {
                continue;
            }
            let Some(entity) = batch.ops.first().map(|(_, f)| f.entity) else {
                continue;
            };
            let Some(payload) = inner.reconstruct_at(entity, TxId(u64::MAX))? else {
                continue;
            };
            if matches!(
                kind_of_payload(&payload),
                EntityKind::Object | EntityKind::Relationship
            ) {
                added.push(LedgerEntryId(batch.tx.0 as i64));
                touched_ids.insert(entity.to_string());
            }
        }

        let total = self_counts(&inner)?;
        let unchanged = total.saturating_sub(touched_ids.len());
        let mut touched: Vec<String> = touched_ids.into_iter().collect();
        touched.sort();
        Ok(LedgerDiff {
            added,
            touched,
            unchanged,
        })
    }

    /// Merge every object/relationship from `branch` — same last-write
    /// divergence semantics as the SQLite `merge_branch` (RFC 0011).
    pub fn merge_from(&self, branch: &FactLedger) -> Result<MergeReport, LedgerError> {
        let mut report = MergeReport::default();
        for obj in branch.all_objects()? {
            match self.get_object(&obj.id)? {
                None => {
                    self.append_object(&obj)?;
                    report.objects_merged += 1;
                }
                Some(existing) => {
                    let a = content_signature(&serde_json::to_value(&existing)?);
                    let b = content_signature(&serde_json::to_value(&obj)?);
                    if a != b {
                        report.conflicts.push(MergeConflict {
                            object_id: obj.id.to_string(),
                            reason: "object diverged between branches".to_string(),
                        });
                    }
                }
            }
        }
        for rel in branch.all_relationships()? {
            match self.get_relationship(&rel.id)? {
                None => {
                    self.append_relationship(&rel)?;
                    report.relationships_merged += 1;
                }
                Some(existing) => {
                    let a = content_signature(&serde_json::to_value(&existing)?);
                    let b = content_signature(&serde_json::to_value(&rel)?);
                    if a != b {
                        report.conflicts.push(MergeConflict {
                            object_id: rel.id.to_string(),
                            reason: "relationship diverged between branches".to_string(),
                        });
                    }
                }
            }
        }
        Ok(report)
    }

    /// Runs currently open per sort order (test/diagnostic hook).
    pub fn run_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap()
            .runs
            .run_count(crate::index::SortOrder::Eavt)
    }
}

/// Objects + relationships currently tracked (diff's `unchanged` base).
fn self_counts(inner: &Inner) -> Result<usize, LedgerError> {
    let mut total = 0usize;
    for attr in ["name", "from"] {
        for id in inner.entities_with_attr(attr)? {
            if let Some(payload) = inner.reconstruct_at(id, TxId(u64::MAX))?
                && matches!(
                    kind_of_payload(&payload),
                    EntityKind::Object | EntityKind::Relationship
                )
            {
                total += 1;
            }
        }
    }
    Ok(total)
}

impl Inner {
    /// Every entity's current payload, from ONE sequential pass over the
    /// EAVT runs plus the memtable — the bulk-read path for listings.
    fn all_current_payloads(&self) -> Result<Vec<(Uuid, serde_json::Value)>, LedgerError> {
        self.all_payloads_at(None)
    }

    /// Every entity's payload as of `cut` (or current, when `None`) — one
    /// sequential pass over the EAVT runs plus the memtable, folding each
    /// entity's history up to `cut` instead of taking its latest state
    /// (RFC 0096: generalizes `all_current_payloads` to a bulk historical
    /// read, the primitive EKL's `AS OF` clause needs — only single-entity
    /// `reconstruct_at` existed before this RFC).
    fn all_payloads_at(
        &self,
        cut: Option<TxId>,
    ) -> Result<Vec<(Uuid, serde_json::Value)>, LedgerError> {
        let mut by_entity: HashMap<Uuid, Vec<IndexEntry>> = HashMap::new();
        for run in self.runs.runs_of(crate::index::SortOrder::Eavt) {
            for entry in run.all()? {
                by_entity.entry(entry.entity).or_default().push(entry);
            }
        }
        for entry in &self.memtable {
            by_entity
                .entry(entry.entity)
                .or_default()
                .push(entry.clone());
        }

        let mut out = Vec::with_capacity(by_entity.len());
        for (entity, entries) in by_entity {
            let facts = fold_state(entity, &entries, cut);
            if facts.is_empty() {
                continue;
            }
            let payload = reconstruct(&facts, &self.store.manifest.attributes)
                .map_err(|e| LedgerError::Corrupt(e.to_string()))?;
            out.push((entity, payload));
        }
        Ok(out)
    }

    /// All history entries of one entity: run scans + memtable tail.
    fn entity_entries(&self, entity: Uuid) -> Result<Vec<IndexEntry>, LedgerError> {
        let mut entries = self.runs.scan(&ScanPrefix::Entity { entity, attr: None })?;
        entries.extend(self.memtable.iter().filter(|e| e.entity == entity).cloned());
        Ok(entries)
    }

    /// Every distinct whole-entity snapshot across this entity's history,
    /// oldest to newest (RFC 0047) — one entry per `tx` at which *any* fact
    /// about it changed, each reconstructed by folding history up to that
    /// tx. Facts are per-attribute here (unlike the SQLite backend's
    /// whole-object rows), so a "version" is defined as the state
    /// immediately after each point where something changed, not a stored
    /// snapshot — the same notion `object_at`/`reconstruct_at` already use
    /// for a single point-in-time cut, just walked across every cut instead
    /// of one. O(versions × entries) — fine for this RFC's scope (a small
    /// fixture), not optimized for entities with very long histories.
    fn entity_history(&self, entity: Uuid) -> Result<Vec<serde_json::Value>, LedgerError> {
        let entries = self.entity_entries(entity)?;
        let mut txs: Vec<TxId> = entries.iter().map(|e| e.tx).collect();
        txs.sort();
        txs.dedup();

        let mut out = Vec::new();
        for cut in txs {
            let facts = fold_state(entity, &entries, Some(cut));
            if facts.is_empty() {
                continue;
            }
            let payload = reconstruct(&facts, &self.store.manifest.attributes)
                .map_err(|e| LedgerError::Corrupt(e.to_string()))?;
            out.push(payload);
        }
        Ok(out)
    }

    /// Latest checkpoint for `entity` at or before `cut` (`None` cut means "current state" —
    /// the latest checkpoint overall). `None` when no checkpoint exists yet, or none is old
    /// enough to apply — the correct, honest fallback to a full genesis fold (RFC 0106 Phase 3).
    fn checkpoint_at(&self, entity: Uuid, cut: Option<TxId>) -> Option<(TxId, &[Fact])> {
        let by_tx = self.checkpoints.get(&entity)?;
        let (tx, facts) = match cut {
            Some(cut) => by_tx.range(..=cut).next_back()?,
            None => by_tx.iter().next_back()?,
        };
        Some((*tx, facts.as_slice()))
    }

    /// Fold an entity's history (up to `cut`, if given) into its live fact set — see
    /// [`fold_state`]. RFC 0106 Phase 3: seeds the fold from the nearest applicable checkpoint
    /// instead of always from genesis, when one exists — see [`Self::checkpoint_at`]'s own doc
    /// comment for the exact equivalence this relies on. Never a source of correctness: a
    /// missing, stale, or absent checkpoint just means this falls back to exactly today's
    /// behavior (fold every real entry, unseeded).
    fn state_at(&self, entity: Uuid, cut: Option<TxId>) -> Result<Vec<Fact>, LedgerError> {
        let entries = self.entity_entries(entity)?;
        match self.checkpoint_at(entity, cut) {
            Some((checkpoint_tx, facts)) => {
                let mut seeded: Vec<IndexEntry> = facts
                    .iter()
                    .map(|f| IndexEntry::from_fact(f, checkpoint_tx, FactOp::Assert))
                    .collect();
                seeded.extend(entries.into_iter().filter(|e| e.tx > checkpoint_tx));
                Ok(fold_state(entity, &seeded, cut))
            }
            None => Ok(fold_state(entity, &entries, cut)),
        }
    }

    /// Real entries for `entity` committed strictly after its latest checkpoint (or all of them,
    /// if none exists yet) — the cheap, checkpoint-bounded count [`Self::maybe_checkpoint`] uses
    /// to decide whether enough new history has accumulated to justify writing another one.
    fn entries_since_checkpoint(&self, entity: Uuid) -> Result<Vec<IndexEntry>, LedgerError> {
        let entries = self.entity_entries(entity)?;
        Ok(match self.checkpoint_at(entity, None) {
            Some((checkpoint_tx, _)) => entries
                .into_iter()
                .filter(|e| e.tx > checkpoint_tx)
                .collect(),
            None => entries,
        })
    }

    /// Write a new checkpoint for `entity` at `tx` once [`CHECKPOINT_INTERVAL`] real versions
    /// have accumulated since its last one (RFC 0106 Phase 3) — called from the write path
    /// (`append_inner`), after the write that might cross the threshold has already committed.
    /// Pays the one real O(full history) fold this design needs, but only once per interval, not
    /// on every read. Never removes or supersedes an earlier checkpoint for the same entity —
    /// every checkpoint ever written stays valid and queryable, so a point-in-time read for a
    /// `cut` older than the newest checkpoint still finds the right, earlier one via
    /// [`Self::checkpoint_at`].
    fn maybe_checkpoint(&mut self, entity: Uuid, tx: TxId) -> Result<(), LedgerError> {
        let since = self.entries_since_checkpoint(entity)?;
        let mut versions: Vec<TxId> = since.iter().map(|e| e.tx).collect();
        versions.sort_unstable();
        versions.dedup();
        if versions.len() < CHECKPOINT_INTERVAL {
            return Ok(());
        }
        let facts = self.state_at(entity, Some(tx))?;
        append_checkpoint(&self.checkpoints_root, entity, tx, &facts)?;
        self.checkpoints
            .entry(entity)
            .or_default()
            .insert(tx, facts);
        Ok(())
    }

    fn reconstruct_at(
        &self,
        entity: Uuid,
        cut: TxId,
    ) -> Result<Option<serde_json::Value>, LedgerError> {
        let facts = self.state_at(entity, Some(cut))?;
        if facts.is_empty() {
            return Ok(None);
        }
        let payload = reconstruct(&facts, &self.store.manifest.attributes)
            .map_err(|e| LedgerError::Corrupt(e.to_string()))?;
        Ok(Some(payload))
    }

    /// Current signature, computed lazily through reconstruction.
    fn current_sig(&mut self, entity: Uuid) -> Result<Option<String>, LedgerError> {
        if let Some(sig) = self.sig_cache.get(&entity) {
            return Ok(Some(sig.clone()));
        }
        match self.reconstruct_at(entity, TxId(u64::MAX))? {
            Some(payload) => {
                let sig = content_signature(&payload);
                self.sig_cache.insert(entity, sig.clone());
                Ok(Some(sig))
            }
            None => Ok(None),
        }
    }

    /// Distinct entities carrying `attr_path` anywhere in history — AEVT
    /// runs plus the memtable (current-state filtering is the caller's job).
    fn entities_with_attr(&self, attr_path: &str) -> Result<Vec<Uuid>, LedgerError> {
        let Some(attr) = self.store.manifest.attributes.get(attr_path) else {
            return Ok(Vec::new());
        };
        let mut ids: HashSet<Uuid> = self
            .runs
            .scan(&ScanPrefix::Attr { attr })?
            .into_iter()
            .map(|e| e.entity)
            .collect();
        ids.extend(
            self.memtable
                .iter()
                .filter(|e| e.attr == attr)
                .map(|e| e.entity),
        );
        let mut out: Vec<Uuid> = ids.into_iter().collect();
        out.sort();
        Ok(out)
    }

    /// Relationship entities that ever referenced `node` in `from`/`to` —
    /// AVET ranged scans + memtable (the RFC's graph-hop read path).
    fn relationship_candidates(&self, node: Uuid) -> Result<Vec<Uuid>, LedgerError> {
        let mut ids: HashSet<Uuid> = HashSet::new();
        for side in ["from", "to"] {
            let Some(attr) = self.store.manifest.attributes.get(side) else {
                continue;
            };
            ids.extend(
                self.runs
                    .scan(&ScanPrefix::AttrValue {
                        attr,
                        value: FactValue::Ref(node),
                    })?
                    .into_iter()
                    .map(|e| e.entity),
            );
            ids.extend(
                self.memtable
                    .iter()
                    .filter(|e| e.attr == attr && e.value == FactValue::Ref(node))
                    .map(|e| e.entity),
            );
        }
        let mut out: Vec<Uuid> = ids.into_iter().collect();
        out.sort();
        Ok(out)
    }

    /// Seal hook: flush the memtable into new runs, advance the marker,
    /// and compact when runs accumulate.
    fn flush_memtable(&mut self, up_to: TxId) -> Result<(), LedgerError> {
        if self.memtable.is_empty() {
            return Ok(());
        }
        self.runs
            .add_runs(&format!("{:012}", up_to.0), &self.memtable)?;
        std::fs::write(self.runs_dir().join("last_tx"), up_to.0.to_string())
            .map_err(LedgerError::Io)?;
        self.runs_marker = Some(up_to);
        self.memtable.clear();
        for order in crate::index::SortOrder::ALL {
            if self.runs.run_count(order) > MERGE_RUNS_AT {
                self.runs.merge_runs(order)?;
            }
        }
        Ok(())
    }

    fn runs_dir(&self) -> PathBuf {
        self.store.root().join("indexes")
    }

    /// Buffer this object's current state into the tantivy index.
    /// RFC 0100: deserializes into the real typed `KirObject` and reuses
    /// its own `indexed_content()` rather than re-deriving the same field
    /// list inline from raw JSON a second time. That inline copy (excerpt +
    /// symbols only) had silently drifted from `indexed_content()`'s real
    /// field list — found live: it never included `ocr_text` at all, so
    /// OCR'd scanned-document text was unsearchable on this backend (the
    /// RFC 0016 default for every new workspace) despite RFC 0024 adding it
    /// to `indexed_content()` specifically to make it searchable. The
    /// SQLite backend (`index_object_fts_v1`/`v2`) never had this bug — it
    /// always called `obj.indexed_content()` directly; only this backend's
    /// independent reimplementation had drifted. One shared field list
    /// instead of two independently-maintained copies closes the whole
    /// class of "fixed on one backend, not the other" bug, not just this
    /// one instance of it.
    fn index_object(&mut self, entity: Uuid, payload: &serde_json::Value) {
        let name = payload
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let kind = payload
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        // RFC 0101: is_under_memory_path shares this one deserialization
        // with indexed_content() rather than parsing the payload twice.
        let (content, is_memory_path) = match serde_json::from_value::<KirObject>(payload.clone()) {
            Ok(obj) => (obj.indexed_content(), obj.is_under_memory_path()),
            Err(_) => (String::new(), false),
        };
        self.search
            .upsert(entity, name, kind, &content, is_memory_path);
    }

    /// The greatest tx whose batch wall time is ≤ `at` (RFC 0016 §2).
    fn tx_at(&self, at: DateTime<Utc>) -> Option<TxId> {
        let at_us = at.timestamp_micros();
        self.batch_times
            .iter()
            .rev()
            .find(|(_, w)| *w <= at_us)
            .map(|(t, _)| *t)
    }
}

/// Fold history entries into the live fact set at `cut` (or now): the
/// latest op per (attr, pos) wins; a retract removes the slot.
fn fold_state(entity: Uuid, entries: &[IndexEntry], cut: Option<TxId>) -> Vec<Fact> {
    let mut live: HashMap<(AttrId, Option<u32>), (TxId, FactOp, &FactValue)> = HashMap::new();
    for e in entries {
        if let Some(cut) = cut
            && e.tx > cut
        {
            continue;
        }
        let slot = live
            .entry((e.attr, e.pos))
            .or_insert((e.tx, e.op, &e.value));
        if e.tx >= slot.0 {
            *slot = (e.tx, e.op, &e.value);
        }
    }
    let mut facts: Vec<Fact> = live
        .into_iter()
        .filter(|(_, (_, op, _))| matches!(op, FactOp::Assert))
        .map(|((attr, pos), (_, _, value))| Fact {
            entity,
            attr,
            pos,
            value: value.clone(),
        })
        .collect();
    facts.sort_by_key(|f| (f.attr, f.pos));
    facts
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

// The parity suite lives in `tests/` style within the crate: every test
// mirrors a case from the SQLite backend's suite (same names where the
// semantics are identical), plus cross-backend parity checks.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Ledger;
    use chrono::Duration;
    use ekos_kir::{EventKind, ObjectKind, RelationshipKind, SourceLocation};
    use std::time::Duration as StdDuration;
    use tempfile::tempdir;

    fn temp_ledger() -> (FactLedger, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("factledger");
        (FactLedger::open(&path).unwrap(), dir)
    }

    /// RFC 0113 B4 — with a `SegmentBackend`, the sealed segments (a partition's bulk) live on the
    /// backend, not local disk: wiping the local `segments/` dir loses nothing.
    #[test]
    fn sealed_segments_are_served_from_the_backend_not_local_disk() {
        let root_dir = tempdir().unwrap();
        let backend_cache = tempdir().unwrap();
        let root = root_dir.path().join("part");
        let backend = std::sync::Arc::new(crate::MemBackend::new(backend_cache.path()));

        let a = KirObject::new("orders", ObjectKind::Table);
        let b = KirObject::new("customers", ObjectKind::Table);
        {
            let l = FactLedger::open_with_backend_and_seal_threshold(&root, backend.clone(), 1)
                .unwrap();
            l.append_object(&a).unwrap();
            l.append_object(&b).unwrap();
        }

        // Sealed segments really were published to the backend...
        assert!(
            !backend.list("segments/").unwrap().is_empty(),
            "seals must publish to the backend"
        );
        // ...and the local sealed-segment files are now expendable.
        std::fs::remove_dir_all(root.join("segments")).unwrap();

        let l = FactLedger::open_read_only_with_backend(&root, backend.clone()).unwrap();
        assert!(l.get_object(&a.id).unwrap().is_some());
        assert!(l.get_object(&b.id).unwrap().is_some());
        assert_eq!(l.object_count().unwrap(), 2);
    }

    #[test]
    fn append_and_retrieve_object() {
        let (ledger, _dir) = temp_ledger();
        let obj = KirObject::new("orders", ObjectKind::Table);
        let id = obj.id;
        ledger.append_object(&obj).unwrap();
        let found = ledger.get_object(&id).unwrap().unwrap();
        assert_eq!(found.name, "orders");
    }

    // ── RFC 0100: index_object reuses indexed_content(), fixing a real ──────
    // ── ocr_text gap and adding ai_overview/ai_usage search coverage ────────

    #[test]
    fn find_objects_matches_ocr_text_a_real_regression_this_backend_had() {
        // Before RFC 0100, this backend's own index_object re-derived the
        // indexed field list inline and never included ocr_text at all —
        // the SQLite backend's equivalent path always had it (RFC 0024).
        let (ledger, _dir) = temp_ledger();
        let obj = KirObject::new("scan.pdf", ObjectKind::Custom("Document".into())).with_property(
            "ocr_text",
            serde_json::json!("a distinctive scanned phrase"),
        );
        ledger.append_object(&obj).unwrap();

        let hits = ledger.find_objects("distinctive").unwrap();
        assert!(
            hits.iter().any(|(id, _)| *id == obj.id),
            "OCR'd text must be searchable on this backend, same as SQLite"
        );
    }

    #[test]
    fn find_objects_matches_ai_overview_text() {
        let (ledger, _dir) = temp_ledger();
        let obj = KirObject::new("orders", ObjectKind::Table).with_property(
            "ai_overview",
            serde_json::json!("Tracks customer purchases and sales transactions."),
        );
        ledger.append_object(&obj).unwrap();

        let hits = ledger.find_objects("purchases").unwrap();
        assert!(
            hits.iter().any(|(id, _)| *id == obj.id),
            "a real LLM-generated overview term must be searchable even though \
             the word 'purchases' never appears in the object's own name"
        );
    }

    // ── RFC 0101: memory/-path structural search boost ──────────────────

    #[test]
    fn find_objects_ranks_a_memory_path_content_match_above_an_equal_ordinary_match() {
        let (ledger, _dir) = temp_ledger();
        // Same content term, same field, no other ranking signal that
        // would otherwise separate them — only the memory/ path should
        // move the memory note ahead of the ordinary project file.
        let memory_note = KirObject::new(
            "global--lesson--x.md",
            ObjectKind::Custom("Document".into()),
        )
        .with_property("project", serde_json::json!("memory"))
        .with_property("path", serde_json::json!("global--lesson--x.md"))
        .with_property(
            "excerpt",
            serde_json::json!("a distinctive keyword appears here"),
        );
        let ordinary_file = KirObject::new("notes.md", ObjectKind::Custom("Document".into()))
            .with_property("project", serde_json::json!("some-project"))
            .with_property("path", serde_json::json!("docs/notes.md"))
            .with_property(
                "excerpt",
                serde_json::json!("a distinctive keyword appears here"),
            );
        ledger.append_object(&memory_note).unwrap();
        ledger.append_object(&ordinary_file).unwrap();

        let hits = ledger.find_objects("distinctive").unwrap();
        let memory_rank = hits.iter().position(|(id, _)| *id == memory_note.id);
        let ordinary_rank = hits.iter().position(|(id, _)| *id == ordinary_file.id);
        assert!(
            memory_rank.is_some() && ordinary_rank.is_some(),
            "both objects must match: {hits:?}"
        );
        assert!(
            memory_rank < ordinary_rank,
            "the memory-path object must rank above the otherwise-identical \
             ordinary file, got hits: {hits:?}"
        );
    }

    #[test]
    fn find_objects_boost_never_makes_a_non_matching_memory_object_appear() {
        // The Should clause must never widen the result set — a memory-path
        // object that doesn't match the query terms at all must still be
        // absent, same as any other non-matching document.
        let (ledger, _dir) = temp_ledger();
        let memory_note = KirObject::new(
            "global--lesson--x.md",
            ObjectKind::Custom("Document".into()),
        )
        .with_property("project", serde_json::json!("memory"))
        .with_property("path", serde_json::json!("global--lesson--x.md"))
        .with_property("excerpt", serde_json::json!("completely unrelated content"));
        ledger.append_object(&memory_note).unwrap();

        let hits = ledger.find_objects("nonexistentterm").unwrap();
        assert!(hits.is_empty(), "got: {hits:?}");
    }

    // ── RFC 0097: open_read_only ────────────────────────────────────────────

    #[test]
    fn open_read_only_fails_cleanly_on_a_never_built_workspace() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("factledger");
        assert!(matches!(
            FactLedger::open_read_only(&path),
            Err(LedgerError::NotFound(_))
        ));
    }

    #[test]
    fn open_read_only_reads_data_a_writable_open_committed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("factledger");
        {
            let writable = FactLedger::open(&path).unwrap();
            writable
                .append_object(&KirObject::new("orders", ObjectKind::Table))
                .unwrap();
            // Force the search index's lazy group-commit (module doc: "commit
            // lazily on the first query after a write") — an uncommitted
            // tantivy writer discards its buffer on drop, so without this the
            // read-only reopen below would legitimately find nothing via
            // `find_objects`, a fact about the lazy-commit design, not this
            // RFC's read-only path.
            writable.find_objects("orders").unwrap();
        } // dropped — releases the tantivy writer lock

        let reader = FactLedger::open_read_only(&path).unwrap();
        assert_eq!(reader.object_count().unwrap(), 1);
        let (id, _) = reader
            .find_objects("orders")
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(reader.get_object(&id).unwrap().unwrap().name, "orders");
    }

    #[test]
    fn open_read_only_rejects_every_write_method() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("factledger");
        {
            let writable = FactLedger::open(&path).unwrap();
            writable
                .append_object(&KirObject::new("orders", ObjectKind::Table))
                .unwrap();
        }

        let reader = FactLedger::open_read_only(&path).unwrap();
        assert!(matches!(
            reader.append_object(&KirObject::new("customers", ObjectKind::Table)),
            Err(LedgerError::ReadOnly(_))
        ));
        assert!(matches!(
            reader.append_relationship(&KirRelationship::new(
                RelationshipKind::ForeignKey,
                KirId::new(),
                KirId::new(),
            )),
            Err(LedgerError::ReadOnly(_))
        ));
        assert_eq!(
            reader.object_count().unwrap(),
            1,
            "the rejected write must not have landed"
        );
    }

    #[test]
    fn open_read_only_never_blocks_a_concurrent_writable_open() {
        // The exact regression this RFC exists to prevent: a long-lived
        // read-only handle must never hold tantivy's exclusive IndexWriter
        // lock, or a real concurrent `ekos build`/`commit` (a fresh writable
        // open) could never acquire it for as long as the reader stays open.
        let dir = tempdir().unwrap();
        let path = dir.path().join("factledger");
        {
            let writable = FactLedger::open(&path).unwrap();
            writable
                .append_object(&KirObject::new("orders", ObjectKind::Table))
                .unwrap();
        }

        let reader = FactLedger::open_read_only(&path).unwrap();
        // `reader` stays alive (not dropped) across this second writable
        // open — the pre-fix design would fail here with a real
        // tantivy `LockBusy` error.
        let writable_again = FactLedger::open(&path).unwrap();
        writable_again
            .append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();
        assert_eq!(writable_again.object_count().unwrap(), 2);
        drop(reader);
    }

    // ── RFC 0104 Phase 1: real cross-process write lock ─────────────────────

    #[test]
    fn a_second_writable_open_fails_fast_with_a_clear_locked_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("factledger");
        let first = FactLedger::open(&path).unwrap();

        let result = FactLedger::open(&path);
        match result {
            Err(LedgerError::Locked(msg)) => {
                assert!(
                    msg.contains("write.lock"),
                    "error should name the real lock file, got: {msg}"
                );
            }
            other => panic!(
                "expected a clear LedgerError::Locked, got: {}",
                match other {
                    Ok(_) => "Ok(_)".to_string(),
                    Err(e) => format!("{e:?}"),
                }
            ),
        }
        drop(first);
    }

    #[test]
    fn dropping_a_writable_handle_releases_the_write_lock_for_the_next_one() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("factledger");
        let first = FactLedger::open(&path).unwrap();
        drop(first);

        // Must succeed now that the first handle (and its lock file) is gone.
        let second = FactLedger::open(&path).unwrap();
        second
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        assert_eq!(second.object_count().unwrap(), 1);
    }

    /// The concurrent-read-visibility spec (RFC 0104 Phase 1), made concrete: a `FactLedger`
    /// handle's view of the ledger is frozen as of its own `open()` call, not automatically
    /// refreshed by a separate process's (here: a separate handle's) concurrent writes. This is a
    /// real, documented limitation (see [`FactLedger::open_read_only`]'s own doc comment) — this
    /// test proves it's actually true of the implementation, not just plausible from reading the
    /// code.
    #[test]
    fn a_long_lived_handle_does_not_see_a_separate_handles_writes_until_reopened() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("factledger");
        let writer = FactLedger::open(&path).unwrap();
        writer
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        drop(writer);

        let long_lived = FactLedger::open_read_only(&path).unwrap();
        assert_eq!(long_lived.object_count().unwrap(), 1);

        // A separate handle appends a second object — simulating a second process's
        // `ekos build`/`commit` running while `long_lived` stays open.
        let second_writer = FactLedger::open(&path).unwrap();
        second_writer
            .append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();
        drop(second_writer);

        assert_eq!(
            long_lived.object_count().unwrap(),
            1,
            "a long-lived handle's view must stay frozen as of its own open() call — it must \
             not see a separate handle's write without being reopened"
        );

        // Re-opening does pick up the write — proves this is a real, spec'd staleness window,
        // not a permanently broken read path.
        let reopened = FactLedger::open_read_only(&path).unwrap();
        assert_eq!(reopened.object_count().unwrap(), 2);
    }

    #[test]
    fn all_objects_and_relationships_are_listed() {
        let (ledger, _dir) = temp_ledger();
        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        ledger
            .append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();
        ledger
            .append_relationship(&KirRelationship::new(
                RelationshipKind::ForeignKey,
                KirId::new(),
                KirId::new(),
            ))
            .unwrap();
        assert_eq!(ledger.all_objects().unwrap().len(), 2);
        assert_eq!(ledger.all_relationships().unwrap().len(), 1);
        assert_eq!(ledger.object_count().unwrap(), 2);
        assert_eq!(ledger.relationship_count().unwrap(), 1);
    }

    #[test]
    fn append_is_idempotent() {
        let (ledger, _dir) = temp_ledger();
        let obj = KirObject::new("customers", ObjectKind::Table);
        assert!(ledger.append_object(&obj).unwrap());
        assert!(!ledger.append_object(&obj).unwrap());
        assert_eq!(ledger.entry_count().unwrap(), 1);
    }

    #[test]
    fn get_unknown_object_returns_none() {
        let (ledger, _dir) = temp_ledger();
        assert!(ledger.get_object(&KirId::new()).unwrap().is_none());
    }

    #[test]
    fn evidence_round_trips_and_is_not_an_object() {
        let (ledger, _dir) = temp_ledger();
        let ev = KirEvidence::new(SourceLocation::at("schema.sql", 10), "CREATE TABLE orders")
            .with_confidence(0.5);
        let id = ev.id;
        ledger.append_evidence(&ev).unwrap();
        let found = ledger.get_evidence(&id).unwrap().unwrap();
        assert_eq!(found.fragment, "CREATE TABLE orders");
        assert_eq!(found.confidence, 0.5);
        assert!(
            ledger.get_object(&id).unwrap().is_none(),
            "typed reads respect entity kind"
        );
        assert_eq!(ledger.object_count().unwrap(), 0);
    }

    /// RFC 0029: first real write path for events on the fact-engine
    /// backend — `kind_of_payload`'s `"subject"` dispatch already existed,
    /// only the public `append_event`/`get_event` wrappers were missing.
    #[test]
    fn event_round_trips_and_is_not_an_object() {
        let (ledger, _dir) = temp_ledger();
        let subject = KirId::new();
        let ev = KirEvent {
            id: KirId::new(),
            kind: EventKind::Merged,
            subject,
            payload: serde_json::json!({"decision": "confirmed"}),
            evidence: vec![],
            occurred_at: Utc::now(),
        };
        let id = ev.id;
        ledger.append_event(&ev).unwrap();
        let found = ledger.get_event(&id).unwrap().unwrap();
        assert_eq!(found.subject, subject);
        assert_eq!(found.kind, EventKind::Merged);
        assert!(
            ledger.get_object(&id).unwrap().is_none(),
            "typed reads respect entity kind"
        );
    }

    #[test]
    fn updating_creates_new_version_and_keeps_latest_current() {
        let (ledger, _dir) = temp_ledger();
        let mut obj = KirObject::new("orders", ObjectKind::Table);
        let id = obj.id;
        ledger.append_object(&obj).unwrap();

        obj.properties
            .insert("row_count".into(), serde_json::json!(42));
        assert!(ledger.append_object(&obj).unwrap());
        assert_eq!(ledger.entry_count().unwrap(), 2);
        assert_eq!(ledger.object_count().unwrap(), 1);
        let current = ledger.get_object(&id).unwrap().unwrap();
        assert_eq!(
            current.properties.get("row_count"),
            Some(&serde_json::json!(42))
        );
    }

    #[test]
    fn object_at_returns_true_historical_version_after_update() {
        let (ledger, _dir) = temp_ledger();
        let mut obj = KirObject::new("orders", ObjectKind::Table);
        let id = obj.id;
        ledger.append_object(&obj).unwrap();
        std::thread::sleep(StdDuration::from_millis(2));
        let mid = Utc::now();
        std::thread::sleep(StdDuration::from_millis(2));

        obj.properties
            .insert("row_count".into(), serde_json::json!(99));
        ledger.append_object(&obj).unwrap();

        let historical = ledger.object_at(&id, mid).unwrap().unwrap();
        assert!(!historical.properties.contains_key("row_count"));
        let current = ledger.get_object(&id).unwrap().unwrap();
        assert_eq!(
            current.properties.get("row_count"),
            Some(&serde_json::json!(99))
        );
        // Before anything was written: none.
        assert!(
            ledger
                .object_at(&id, mid - Duration::seconds(60))
                .unwrap()
                .is_none()
        );
    }

    // ── RFC 0106 Phase 3: version-chain checkpoints ─────────────────────────

    #[test]
    fn a_checkpoint_is_written_after_crossing_the_interval_and_reads_stay_correct() {
        let (ledger, _dir) = temp_ledger();
        let mut obj = KirObject::new("orders", ObjectKind::Table);
        let id = obj.id;

        let mut timestamps = Vec::new();
        for i in 0..(CHECKPOINT_INTERVAL + 5) {
            obj.properties
                .insert("row_count".into(), serde_json::json!(i));
            ledger.append_object(&obj).unwrap();
            std::thread::sleep(StdDuration::from_millis(2));
            timestamps.push((Utc::now(), i));
        }

        // A checkpoint must have been written for this entity once it crossed the interval.
        let checkpoints = ledger.inner.lock().unwrap().checkpoints.clone();
        assert!(
            checkpoints.contains_key(&id.0),
            "an entity crossing CHECKPOINT_INTERVAL versions must have a checkpoint written"
        );

        // Every historical read must still be exactly correct — before, at, and after the
        // checkpoint boundary — proving checkpoint-seeded folding is equivalent to full replay.
        for (ts, expected) in &timestamps {
            let historical = ledger.object_at(&id, *ts).unwrap().unwrap();
            assert_eq!(
                historical.properties.get("row_count"),
                Some(&serde_json::json!(expected)),
                "wrong historical value at row_count={expected}"
            );
        }
    }

    #[test]
    fn object_at_between_two_checkpoints_picks_the_earlier_one_not_the_latest() {
        let (ledger, _dir) = temp_ledger();
        let mut obj = KirObject::new("orders", ObjectKind::Table);
        let id = obj.id;

        // Cross the interval twice, so two checkpoints exist.
        let mut mid_ts = None;
        for i in 0..(CHECKPOINT_INTERVAL * 2 + 3) {
            obj.properties
                .insert("row_count".into(), serde_json::json!(i));
            ledger.append_object(&obj).unwrap();
            std::thread::sleep(StdDuration::from_millis(2));
            if i == CHECKPOINT_INTERVAL {
                // Just after the first checkpoint, well before the second.
                mid_ts = Some(Utc::now());
            }
        }
        let checkpoints = ledger.inner.lock().unwrap().checkpoints.clone();
        let by_tx = checkpoints
            .get(&id.0)
            .expect("two checkpoints should exist");
        assert!(
            by_tx.len() >= 2,
            "expected at least two checkpoints, got {}",
            by_tx.len()
        );

        let historical = ledger.object_at(&id, mid_ts.unwrap()).unwrap().unwrap();
        assert_eq!(
            historical.properties.get("row_count"),
            Some(&serde_json::json!(CHECKPOINT_INTERVAL)),
            "must reflect state as of the requested cut, not silently jump ahead to a later \
             checkpoint"
        );
    }

    #[test]
    fn a_corrupted_trailing_checkpoint_line_is_skipped_not_treated_as_corruption() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("factledger");
        let mut obj = KirObject::new("orders", ObjectKind::Table);
        let id = obj.id;
        {
            let ledger = FactLedger::open(&path).unwrap();
            for i in 0..(CHECKPOINT_INTERVAL + 1) {
                obj.properties
                    .insert("row_count".into(), serde_json::json!(i));
                ledger.append_object(&obj).unwrap();
            }
        }

        // Append a torn/garbage trailing line, simulating a crash mid-write.
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(path.join("checkpoints.jsonl"))
            .unwrap();
        file.write_all(b"{not valid json\n").unwrap();
        drop(file);

        // Must still open cleanly and read correctly — a broken checkpoint line degrades to
        // "one fewer shortcut," never a wrong answer or an open-time failure.
        let ledger = FactLedger::open(&path).unwrap();
        let current = ledger.get_object(&id).unwrap().unwrap();
        assert_eq!(
            current.properties.get("row_count"),
            Some(&serde_json::json!(CHECKPOINT_INTERVAL))
        );
    }

    #[test]
    fn relationships_for_returns_both_directions() {
        let (ledger, _dir) = temp_ledger();
        let a = KirId::new();
        let b = KirId::new();
        let c = KirId::new();
        ledger
            .append_relationship(&KirRelationship::new(RelationshipKind::ForeignKey, a, b))
            .unwrap();
        ledger
            .append_relationship(&KirRelationship::new(RelationshipKind::Calls, c, a))
            .unwrap();
        assert_eq!(ledger.relationships_for(&a).unwrap().len(), 2);
        assert_eq!(ledger.relationships_for(&b).unwrap().len(), 1);
    }

    #[test]
    fn relationships_at_filters_by_time() {
        let (ledger, _dir) = temp_ledger();
        let a = KirId::new();
        let before = Utc::now() - Duration::seconds(60);
        ledger
            .append_relationship(&KirRelationship::new(
                RelationshipKind::ForeignKey,
                a,
                KirId::new(),
            ))
            .unwrap();
        assert!(ledger.relationships_at(&a, before).unwrap().is_empty());
        let after = Utc::now() + Duration::seconds(60);
        assert_eq!(ledger.relationships_at(&a, after).unwrap().len(), 1);
    }

    #[test]
    fn relationships_at_reconstructs_a_past_version_of_a_relationship_updated_since() {
        // RFC 0054: mirrors the SQLite backend's own regression test for the
        // identical bug — relationships_at previously reconstructed a
        // relationship's *current* state once it passed an (also
        // incorrect) visibility check, rather than its state at `at`.
        let (ledger, _dir) = temp_ledger();
        let a = KirId::new();
        let b = KirId::new();

        let mut rel = KirRelationship::new(RelationshipKind::Custom("Trusts".to_string()), a, b);
        rel.properties
            .insert("value".to_string(), serde_json::json!(0.5));
        ledger.append_relationship(&rel).unwrap();
        let after_first_write = Utc::now();

        std::thread::sleep(std::time::Duration::from_millis(5));
        rel.properties
            .insert("value".to_string(), serde_json::json!(0.9));
        ledger.append_relationship(&rel).unwrap();

        let historical = ledger.relationships_at(&a, after_first_write).unwrap();
        assert_eq!(
            historical.len(),
            1,
            "the relationship must still be found at this point in time"
        );
        assert_eq!(historical[0].properties["value"], serde_json::json!(0.5));

        let current = ledger.relationships_at(&a, Utc::now()).unwrap();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].properties["value"], serde_json::json!(0.9));
    }

    // ── RFC 0096: all_objects_at / all_relationships_at (bulk AS OF) ────────

    #[test]
    fn all_objects_at_returns_empty_before_anything_written() {
        let (ledger, _dir) = temp_ledger();
        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        let before = Utc::now() - Duration::seconds(60);
        assert!(ledger.all_objects_at(before).unwrap().is_empty());
    }

    #[test]
    fn all_objects_at_returns_the_version_current_at_that_time_not_later_updates() {
        let (ledger, _dir) = temp_ledger();
        let mut orders = KirObject::new("orders", ObjectKind::Table);
        let customers = KirObject::new("customers", ObjectKind::Table);
        ledger.append_object(&orders).unwrap();
        ledger.append_object(&customers).unwrap();
        std::thread::sleep(StdDuration::from_millis(2));
        let mid = Utc::now();
        std::thread::sleep(StdDuration::from_millis(2));

        orders
            .properties
            .insert("row_count".into(), serde_json::json!(99));
        ledger.append_object(&orders).unwrap();

        let snapshot = ledger.all_objects_at(mid).unwrap();
        assert_eq!(snapshot.len(), 2, "both objects existed by `mid`");
        let orders_at_mid = snapshot.iter().find(|o| o.name == "orders").unwrap();
        assert!(!orders_at_mid.properties.contains_key("row_count"));

        let now_snapshot = ledger.all_objects_at(Utc::now()).unwrap();
        let orders_now = now_snapshot.iter().find(|o| o.name == "orders").unwrap();
        assert_eq!(
            orders_now.properties.get("row_count"),
            Some(&serde_json::json!(99))
        );
    }

    #[test]
    fn all_relationships_at_filters_by_time_across_multiple_relationships() {
        let (ledger, _dir) = temp_ledger();
        let (a, b, c) = (KirId::new(), KirId::new(), KirId::new());
        ledger
            .append_relationship(&KirRelationship::new(RelationshipKind::ForeignKey, a, b))
            .unwrap();
        std::thread::sleep(StdDuration::from_millis(2));
        let mid = Utc::now();
        std::thread::sleep(StdDuration::from_millis(2));
        ledger
            .append_relationship(&KirRelationship::new(RelationshipKind::ForeignKey, a, c))
            .unwrap();

        assert_eq!(ledger.all_relationships_at(mid).unwrap().len(), 1);
        assert_eq!(ledger.all_relationships_at(Utc::now()).unwrap().len(), 2);
    }

    // ── RFC 0047: object_history / relationship_history ────────────────────

    #[test]
    fn object_history_returns_every_version_oldest_to_newest() {
        let (ledger, _dir) = temp_ledger();
        let mut obj = KirObject::new("orders", ObjectKind::Table);
        let id = obj.id;
        ledger.append_object(&obj).unwrap();

        obj.properties
            .insert("row_count".into(), serde_json::json!(1));
        ledger.append_object(&obj).unwrap();

        obj.properties
            .insert("row_count".into(), serde_json::json!(2));
        ledger.append_object(&obj).unwrap();

        let history = ledger.object_history(&id).unwrap();
        assert_eq!(history.len(), 3);
        assert!(!history[0].properties.contains_key("row_count"));
        assert_eq!(history[1].properties["row_count"], serde_json::json!(1));
        assert_eq!(history[2].properties["row_count"], serde_json::json!(2));
    }

    #[test]
    fn object_history_empty_for_unknown_id() {
        let (ledger, _dir) = temp_ledger();
        assert!(ledger.object_history(&KirId::new()).unwrap().is_empty());
    }

    #[test]
    fn relationship_history_returns_every_version_oldest_to_newest() {
        let (ledger, _dir) = temp_ledger();
        let (a, b) = (KirId::new(), KirId::new());
        let mut rel = KirRelationship::new(RelationshipKind::DependsOn, a, b);
        let id = rel.id;
        ledger.append_relationship(&rel).unwrap();

        rel.properties.insert("weight".into(), serde_json::json!(1));
        ledger.append_relationship(&rel).unwrap();

        let history = ledger.relationship_history(&id).unwrap();
        assert_eq!(history.len(), 2);
        assert!(!history[0].properties.contains_key("weight"));
        assert_eq!(history[1].properties["weight"], serde_json::json!(1));
    }

    #[test]
    fn relationship_history_is_scoped_to_the_relationships_own_id_not_its_endpoints() {
        let (ledger, _dir) = temp_ledger();
        let (a, b, c) = (KirId::new(), KirId::new(), KirId::new());
        let rel1 = KirRelationship::new(RelationshipKind::DependsOn, a, b);
        let rel2 = KirRelationship::new(RelationshipKind::DependsOn, a, c);
        ledger.append_relationship(&rel1).unwrap();
        ledger.append_relationship(&rel2).unwrap();

        assert_eq!(ledger.relationship_history(&rel1.id).unwrap().len(), 1);
        assert_eq!(ledger.relationship_history(&rel2.id).unwrap().len(), 1);
    }

    #[test]
    fn fts_semantics_prefix_content_and_ranking() {
        let (ledger, _dir) = temp_ledger();
        ledger
            .append_object(&KirObject::new("order_items", ObjectKind::Table))
            .unwrap();
        ledger
            .append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();
        let results = ledger.find_objects("order*").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "order_items");

        // Special characters must not error, just find nothing.
        assert!(ledger.find_objects("zzz-nonexistent").unwrap().is_empty());

        // Content excerpt matches (RFC 0014).
        let note = KirObject::new("note-17.md", ObjectKind::File).with_property(
            "excerpt",
            serde_json::json!("Lesson: coupling analysis is quadratic per commit"),
        );
        ledger.append_object(&note).unwrap();
        let results = ledger.find_objects("quadratic").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "note-17.md");

        // A name hit outranks a content-only mention.
        let mention = KirObject::new("random-notes.md", ObjectKind::File).with_property(
            "excerpt",
            serde_json::json!("this mentions orders in passing"),
        );
        ledger.append_object(&mention).unwrap();
        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        let results = ledger.find_objects("orders").unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].1, "orders", "name match must rank first");
    }

    /// RFC 0019: a symbol name is searchable even when it isn't in the excerpt.
    #[test]
    fn fts_finds_objects_by_harvested_symbol() {
        let (ledger, _dir) = temp_ledger();
        let src = KirObject::new("auth.rs", ObjectKind::File)
            .with_property("excerpt", serde_json::json!("// module preamble only"))
            .with_property("symbols", serde_json::json!(["authenticate_user"]));
        ledger.append_object(&src).unwrap();

        let results = ledger.find_objects("authenticate_user").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "auth.rs");
    }

    #[test]
    fn fts_follows_object_updates() {
        let (ledger, _dir) = temp_ledger();
        let mut obj = KirObject::new("pipeline-notes.md", ObjectKind::File)
            .with_property("excerpt", serde_json::json!("first draft about kafka"));
        ledger.append_object(&obj).unwrap();
        obj.properties.insert(
            "excerpt".into(),
            serde_json::json!("rewritten to cover flink"),
        );
        ledger.append_object(&obj).unwrap();

        assert!(ledger.find_objects("kafka").unwrap().is_empty());
        assert_eq!(ledger.find_objects("flink").unwrap().len(), 1);
    }

    #[test]
    fn diff_reports_updated_object_as_added_and_others_unchanged() {
        let (ledger, _dir) = temp_ledger();
        let mut updated = KirObject::new("orders", ObjectKind::Table);
        ledger.append_object(&updated).unwrap();
        ledger
            .append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();
        ledger
            .append_object(&KirObject::new("products", ObjectKind::Table))
            .unwrap();

        std::thread::sleep(StdDuration::from_millis(2));
        let t1 = Utc::now();
        std::thread::sleep(StdDuration::from_millis(2));
        updated
            .properties
            .insert("row_count".into(), serde_json::json!(7));
        ledger.append_object(&updated).unwrap();
        let t2 = Utc::now() + Duration::seconds(1);

        let diff = ledger.diff(t1, t2).unwrap();
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.unchanged, 2);
        assert_eq!(diff.touched, vec![updated.id.to_string()]);
    }

    #[test]
    fn branch_copy_is_readable_and_merges_like_sqlite() {
        let (main, dir) = temp_ledger();
        main.append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();

        // Branch = copy; then diverge and merge back.
        let branch_path = dir.path().join("branch");
        main.vacuum_into(&branch_path).unwrap();
        let branch = FactLedger::open(&branch_path).unwrap();
        assert_eq!(branch.object_count().unwrap(), 1);

        branch
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        let report = main.merge_from(&branch).unwrap();
        assert_eq!(report.objects_merged, 1);
        assert!(report.conflicts.is_empty());
        assert_eq!(main.object_count().unwrap(), 2);

        // Divergence on shared content is a conflict, not an overwrite.
        let mut shared = KirObject::new("orders", ObjectKind::Table);
        let (main2, dir2) = temp_ledger();
        let (branch2, _dir3) = temp_ledger();
        main2.append_object(&shared).unwrap();
        shared
            .properties
            .insert("row_count".into(), serde_json::json!(5));
        branch2.append_object(&shared).unwrap();
        let report = main2.merge_from(&branch2).unwrap();
        assert_eq!(report.objects_merged, 0);
        assert_eq!(report.conflicts.len(), 1);
        drop(dir2);
    }

    #[test]
    fn state_survives_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("factledger");
        let obj = KirObject::new("orders", ObjectKind::Table)
            .with_property("excerpt", serde_json::json!("searchable body zebra"));
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, obj.id, KirId::new());
        {
            let ledger = FactLedger::open(&path).unwrap();
            ledger.append_object(&obj).unwrap();
            ledger.append_relationship(&rel).unwrap();
        }
        let ledger = FactLedger::open(&path).unwrap();
        assert_eq!(ledger.entry_count().unwrap(), 2);
        assert_eq!(ledger.get_object(&obj.id).unwrap().unwrap().name, "orders");
        assert_eq!(ledger.relationships_for(&obj.id).unwrap().len(), 1);
        assert_eq!(ledger.find_objects("zebra").unwrap().len(), 1);
    }

    /// Phase 6: with a tiny seal threshold every append seals its segment,
    /// flushing the memtable into on-disk runs — reads (point, listing,
    /// graph, history, search) must serve identically from runs after
    /// reopen, when the memtable starts empty.
    #[test]
    fn reads_serve_from_runs_after_seal_and_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("factledger");
        let mut obj = KirObject::new("orders", ObjectKind::Table)
            .with_property("excerpt", serde_json::json!("axolotl inventory"));
        let other = KirObject::new("customers", ObjectKind::Table);
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, obj.id, other.id);
        let mid;
        {
            let ledger = FactLedger::open_with_seal_threshold(&path, 1).unwrap();
            ledger.append_object(&obj).unwrap();
            ledger.append_object(&other).unwrap();
            ledger.append_relationship(&rel).unwrap();
            std::thread::sleep(StdDuration::from_millis(2));
            mid = Utc::now();
            std::thread::sleep(StdDuration::from_millis(2));
            obj.properties
                .insert("row_count".into(), serde_json::json!(7));
            ledger.append_object(&obj).unwrap();
            assert!(ledger.run_count() >= 1, "seals must have flushed runs");
        }

        let ledger = FactLedger::open_with_seal_threshold(&path, 1).unwrap();
        // Point + listing reads.
        let current = ledger.get_object(&obj.id).unwrap().unwrap();
        assert_eq!(
            current.properties.get("row_count"),
            Some(&serde_json::json!(7))
        );
        assert_eq!(ledger.all_objects().unwrap().len(), 2);
        assert_eq!(ledger.object_count().unwrap(), 2);
        assert_eq!(ledger.entry_count().unwrap(), 4);
        // Graph read via AVET scans.
        assert_eq!(ledger.relationships_for(&obj.id).unwrap().len(), 1);
        // History read across runs.
        let historical = ledger.object_at(&obj.id, mid).unwrap().unwrap();
        assert!(!historical.properties.contains_key("row_count"));
        // Search.
        assert_eq!(ledger.find_objects("axolotl").unwrap().len(), 1);
        // Idempotence still holds against run-served state.
        assert!(!ledger.append_object(&obj).unwrap());
    }

    /// The search index is derived: deleting its directory and reopening
    /// rebuilds it from segments with nothing lost (RFC 0016 Phase 5).
    #[test]
    fn search_index_rebuilds_after_deletion() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("factledger");
        let note = KirObject::new("note.md", ObjectKind::File).with_property(
            "excerpt",
            serde_json::json!("the caribou migration dataset"),
        );
        {
            let ledger = FactLedger::open(&path).unwrap();
            ledger.append_object(&note).unwrap();
            assert_eq!(ledger.find_objects("caribou").unwrap().len(), 1);
        }
        std::fs::remove_dir_all(path.join("search")).unwrap();

        let ledger = FactLedger::open(&path).unwrap();
        let hits = ledger.find_objects("caribou").unwrap();
        assert_eq!(hits.len(), 1, "index must rebuild from segment truth");
        assert_eq!(hits[0].1, "note.md");

        // And the marker-based catch-up path: write while open, reopen, search.
        ledger
            .append_object(
                &KirObject::new("more.md", ObjectKind::File)
                    .with_property("excerpt", serde_json::json!("narwhal sightings log")),
            )
            .unwrap();
        drop(ledger);
        let ledger = FactLedger::open(&path).unwrap();
        assert_eq!(ledger.find_objects("narwhal").unwrap().len(), 1);
        assert_eq!(ledger.find_objects("caribou").unwrap().len(), 1);
    }

    /// The acceptance gate in miniature: the same corpus written to both
    /// backends yields identical payloads and content signatures.
    #[test]
    fn cross_backend_parity_with_sqlite_ledger() {
        let dir = tempdir().unwrap();
        let sqlite = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let facts = FactLedger::open(&dir.path().join("factledger")).unwrap();

        let mut objects = Vec::new();
        for i in 0..20 {
            let obj = KirObject::new(format!("table_{i}"), ObjectKind::Table)
                .with_property("size_bytes", serde_json::json!(i))
                .with_property("nested", serde_json::json!({"a": {"b": i}, "arr": [1, i]}))
                .with_evidence(KirId::new());
            sqlite.append_object(&obj).unwrap();
            facts.append_object(&obj).unwrap();
            objects.push(obj);
        }
        // One update so version history exists on both sides.
        let mut updated = objects[0].clone();
        updated
            .properties
            .insert("row_count".into(), serde_json::json!(9));
        sqlite.append_object(&updated).unwrap();
        facts.append_object(&updated).unwrap();

        assert_eq!(sqlite.entry_count().unwrap(), facts.entry_count().unwrap());
        assert_eq!(
            sqlite.object_count().unwrap(),
            facts.object_count().unwrap()
        );
        for obj in &objects {
            let a = sqlite.get_object(&obj.id).unwrap().unwrap();
            let b = facts.get_object(&obj.id).unwrap().unwrap();
            let av = serde_json::to_value(&a).unwrap();
            let bv = serde_json::to_value(&b).unwrap();
            assert_eq!(av, bv, "payload parity for {}", obj.name);
            assert_eq!(
                content_signature(&av),
                content_signature(&bv),
                "signature parity for {}",
                obj.name
            );
        }
    }
}
