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
use ekos_kir::{KirEvidence, KirId, KirObject, KirRelationship};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
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
}

/// The fact-segment ledger — RFC 0016's replacement for the SQLite backend,
/// behind the same API shape (`&self` methods, like `Ledger`).
pub struct FactLedger {
    inner: Mutex<Inner>,
    root: PathBuf,
}

impl FactLedger {
    /// Open (or create) a fact ledger rooted at `root` (a directory).
    pub fn open(root: &Path) -> Result<Self, LedgerError> {
        Self::open_with_seal_threshold(root, SEGMENT_SEAL_BYTES)
    }

    /// `open` with a custom segment seal threshold (tests exercise the
    /// seal → run-flush path without writing megabytes).
    pub fn open_with_seal_threshold(root: &Path, seal_bytes: u64) -> Result<Self, LedgerError> {
        let store = SegmentStore::open_with_seal_threshold(root, seal_bytes)?;
        let runs = FactIndexes::open(root.join("indexes"))?;
        let runs_marker = std::fs::read_to_string(root.join("indexes/last_tx"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(TxId);
        let (search, search_marker) = SearchIndex::open(&root.join("search"))?;

        let mut inner = Inner {
            batch_times: store.batch_headers()?,
            memtable: entries_from_batches(&store.batches_after(runs_marker)?),
            store,
            runs,
            runs_marker,
            sig_cache: HashMap::new(),
            search,
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

    /// Write a KirRelationship. Returns `true` when a new version was recorded.
    pub fn append_relationship(&self, rel: &KirRelationship) -> Result<bool, LedgerError> {
        self.append_payload(rel.id.0, serde_json::to_value(rel)?)
    }

    fn append_payload(
        &self,
        entity: Uuid,
        payload: serde_json::Value,
    ) -> Result<bool, LedgerError> {
        let mut inner = self.inner.lock().unwrap();
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

        let wall = Utc::now().timestamp_micros();
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

    /// All objects currently tracked.
    pub fn all_objects(&self) -> Result<Vec<KirObject>, LedgerError> {
        let inner = self.inner.lock().unwrap();
        let mut out = Vec::new();
        for id in inner.entities_with_attr("name")? {
            if let Some(payload) = inner.reconstruct_at(id, TxId(u64::MAX))?
                && kind_of_payload(&payload) == EntityKind::Object
            {
                out.push(serde_json::from_value(payload)?);
            }
        }
        Ok(out)
    }

    /// All relationships currently tracked.
    pub fn all_relationships(&self) -> Result<Vec<KirRelationship>, LedgerError> {
        let inner = self.inner.lock().unwrap();
        let mut out = Vec::new();
        for id in inner.entities_with_attr("from")? {
            if let Some(payload) = inner.reconstruct_at(id, TxId(u64::MAX))?
                && kind_of_payload(&payload) == EntityKind::Relationship
            {
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

    /// Relationships involving `id` whose **current version** was committed
    /// at or before `at` — the same pointer-table semantics as the SQLite
    /// backend (RFC 0011 limitation, kept for parity).
    pub fn relationships_at(
        &self,
        id: &KirId,
        at: DateTime<Utc>,
    ) -> Result<Vec<KirRelationship>, LedgerError> {
        let inner = self.inner.lock().unwrap();
        let cut = inner.tx_at(at);
        let mut out = Vec::new();
        for rel in inner.relationship_candidates(id.0)? {
            let latest = inner.entity_entries(rel)?.iter().map(|e| e.tx).max();
            let visible = matches!((latest, cut), (Some(t), Some(c)) if t <= c);
            if visible
                && let Some(payload) = inner.reconstruct_at(rel, TxId(u64::MAX))?
                && kind_of_payload(&payload) == EntityKind::Relationship
            {
                out.push(serde_json::from_value(payload)?);
            }
        }
        Ok(out)
    }

    // ── Search (tantivy, RFC 0016 Phase 5) ────────────────────────────────

    /// Ranked BM25 search over object names, kinds, and content excerpts.
    /// Terms are ANDed; a trailing `*` prefix-matches a token; name hits
    /// outrank kind hits outrank content hits (10/4/1 boosts, as RFC 0014
    /// tuned). Buffered upserts group-commit here — read-your-writes without
    /// per-append commit cost.
    pub fn find_objects(&self, query: &str) -> Result<Vec<(KirId, String)>, LedgerError> {
        let mut inner = self.inner.lock().unwrap();
        let last_tx = inner.batch_times.last().map(|(t, _)| *t);
        inner.search.commit(last_tx)?;
        let hits = inner.search.query(query, 50)?;
        Ok(hits
            .into_iter()
            .map(|(id, name)| (KirId(id), name))
            .collect())
    }

    // ── Counters ──────────────────────────────────────────────────────────

    /// Total version count (committed batches) — mirrors the SQLite
    /// backend's `entries` row count.
    pub fn entry_count(&self) -> Result<usize, LedgerError> {
        Ok(self.inner.lock().unwrap().batch_times.len())
    }

    /// Number of distinct objects currently tracked.
    pub fn object_count(&self) -> Result<usize, LedgerError> {
        Ok(self.all_objects()?.len())
    }

    /// Number of distinct relationships currently tracked.
    pub fn relationship_count(&self) -> Result<usize, LedgerError> {
        Ok(self.all_relationships()?.len())
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
    /// All history entries of one entity: run scans + memtable tail.
    fn entity_entries(&self, entity: Uuid) -> Result<Vec<IndexEntry>, LedgerError> {
        let mut entries = self.runs.scan(&ScanPrefix::Entity { entity, attr: None })?;
        entries.extend(self.memtable.iter().filter(|e| e.entity == entity).cloned());
        Ok(entries)
    }

    /// Fold an entity's history (up to `cut`, if given) into its live fact
    /// set: the latest op per (attr, pos) wins; a retract removes the slot.
    fn state_at(&self, entity: Uuid, cut: Option<TxId>) -> Result<Vec<Fact>, LedgerError> {
        let entries = self.entity_entries(entity)?;
        let mut live: HashMap<(AttrId, Option<u32>), (TxId, FactOp, &FactValue)> = HashMap::new();
        for e in &entries {
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
        Ok(facts)
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
    fn index_object(&mut self, entity: Uuid, payload: &serde_json::Value) {
        let name = payload
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let kind = payload
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let content = payload
            .get("properties")
            .and_then(|p| p.get("excerpt"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        self.search.upsert(entity, name, kind, content);
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
    use ekos_kir::{ObjectKind, RelationshipKind, SourceLocation};
    use std::time::Duration as StdDuration;
    use tempfile::tempdir;

    fn temp_ledger() -> (FactLedger, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("factledger");
        (FactLedger::open(&path).unwrap(), dir)
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
