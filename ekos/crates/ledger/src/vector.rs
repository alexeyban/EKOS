//! RFC 0125 (Phase 6 of RFC 0118) — the vector index.
//!
//! A sibling of [`crate::search::SearchIndex`], at `<ledger-dir>/vectors/`. **Derived and
//! rebuildable** with its own `last_tx` watermark — the opt-in post-`commit` embed pass lags BM25
//! by many commits, so the vector arm's freshness is tracked separately.
//!
//! Layout (RFC 0118 §8.6):
//!
//! | file | contents |
//! |---|---|
//! | `meta.json` | `{ format_version, dim, model, metric: "cosine", count, normalized: true }` |
//! | `ids.bin` | `count × 16B` — each row's `KirId` |
//! | `vectors.f32` | `count × dim × f32` LE, **L2-normalized at write** → query cosine = dot product |
//! | `tombstones.bin` | `count × 1B` — `1` = superseded/retracted, skipped at query |
//! | `last_tx` | the embed pass's own watermark |
//!
//! Growth is append-only (a re-embedded object tombstones its old row and appends a new one);
//! `compact()` rewrites without the tombstoned rows. A `dim`/`model` mismatch against the
//! configured provider wipes the directory and rebuilds from `TxId(0)` — the RFC 0103
//! stale-schema self-heal, exactly as `SearchIndex` does for a changed tantivy schema.

use crate::LedgerError;
use crate::fact::TxId;
use ekos_kir::KirId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const FORMAT_VERSION: u32 = 1;
/// `compact()` is worth running past this fraction of tombstoned rows.
pub const COMPACT_TOMBSTONE_RATIO: f32 = 0.3;

fn verr(e: impl std::fmt::Display) -> LedgerError {
    LedgerError::Corrupt(format!("vector index: {e}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Meta {
    format_version: u32,
    dim: usize,
    model: String,
    metric: String,
    count: usize,
    normalized: bool,
}

/// The on-disk vector index of a [`crate::FactLedger`]. Load once, query many; `upsert`/`remove`
/// mutate in memory and `flush()` persists.
pub struct VectorIndex {
    dir: PathBuf,
    dim: usize,
    model: String,
    ids: Vec<KirId>,
    /// Row-major, `ids.len() * dim`, L2-normalized.
    vectors: Vec<f32>,
    /// `1` = tombstoned.
    tombstones: Vec<u8>,
    id_pos: HashMap<KirId, usize>,
    dirty: bool,
}

impl VectorIndex {
    /// Open (or create) the index at `dir` for a provider producing `dim`-length `model` vectors.
    /// A mismatch on either wipes and returns a fresh empty index (watermark `None`).
    pub fn open(dir: &Path, dim: usize, model: &str) -> Result<(Self, Option<TxId>), LedgerError> {
        std::fs::create_dir_all(dir).map_err(LedgerError::Io)?;
        let meta_path = dir.join("meta.json");

        let stale = match std::fs::read(&meta_path) {
            Ok(bytes) => match serde_json::from_slice::<Meta>(&bytes) {
                Ok(m) => m.format_version != FORMAT_VERSION || m.dim != dim || m.model != model,
                Err(_) => true,
            },
            Err(_) => false, // no meta yet = brand new, not stale
        };
        if stale {
            for f in [
                "meta.json",
                "ids.bin",
                "vectors.f32",
                "tombstones.bin",
                "last_tx",
            ] {
                let _ = std::fs::remove_file(dir.join(f));
            }
        }

        let fresh = stale || !meta_path.exists();
        if fresh {
            let idx = Self {
                dir: dir.to_path_buf(),
                dim,
                model: model.to_string(),
                ids: Vec::new(),
                vectors: Vec::new(),
                tombstones: Vec::new(),
                id_pos: HashMap::new(),
                dirty: true,
            };
            return Ok((idx, None));
        }

        let id_bytes = std::fs::read(dir.join("ids.bin")).map_err(LedgerError::Io)?;
        let vec_bytes = std::fs::read(dir.join("vectors.f32")).map_err(LedgerError::Io)?;
        let tomb = std::fs::read(dir.join("tombstones.bin")).unwrap_or_default();

        let count = id_bytes.len() / 16;
        if vec_bytes.len() != count * dim * 4 {
            return Err(verr(format!(
                "vectors.f32 is {} bytes, expected {} ({count} × {dim} × 4)",
                vec_bytes.len(),
                count * dim * 4
            )));
        }
        let ids: Vec<KirId> = id_bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|c| KirId(uuid::Uuid::from_bytes(*c)))
            .collect();
        let vectors: Vec<f32> = vec_bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect();
        let mut tombstones = tomb;
        tombstones.resize(count, 0);

        let id_pos = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();

        let marker = std::fs::read_to_string(dir.join("last_tx"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(TxId);

        Ok((
            Self {
                dir: dir.to_path_buf(),
                dim,
                model: model.to_string(),
                ids,
                vectors,
                tombstones,
                id_pos,
                dirty: false,
            },
            marker,
        ))
    }

    /// Open an existing index for querying using its own on-disk `dim`/`model` — no stale check,
    /// no rebuild. `Ok(None)` when there is no index at `dir` (the common case: no embed pass has
    /// run). The query path in [`crate::FactLedger::retrieve`] uses this.
    pub fn open_existing(dir: &Path) -> Result<Option<Self>, LedgerError> {
        let Ok(bytes) = std::fs::read(dir.join("meta.json")) else {
            return Ok(None);
        };
        let meta: Meta = serde_json::from_slice(&bytes).map_err(verr)?;
        let (idx, _) = Self::open(dir, meta.dim, &meta.model)?;
        Ok(Some(idx))
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Whether `id` has a live (non-tombstoned) row.
    pub fn contains(&self, id: &KirId) -> bool {
        self.id_pos.contains_key(id)
    }

    /// Live (non-tombstoned) row count.
    pub fn len(&self) -> usize {
        self.tombstones.iter().filter(|&&t| t == 0).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn tombstoned(&self) -> usize {
        self.tombstones.iter().filter(|&&t| t == 1).count()
    }

    /// Insert or replace `id`'s vector. `vec` is L2-normalized on the way in; a replacement
    /// tombstones the old row and appends a new one (the file body is never rewritten in place).
    pub fn upsert(&mut self, id: KirId, mut vec: Vec<f32>) -> Result<(), LedgerError> {
        if vec.len() != self.dim {
            return Err(verr(format!(
                "vector for {id} has dim {}, index dim is {}",
                vec.len(),
                self.dim
            )));
        }
        ekos_normalize(&mut vec);
        if let Some(&old) = self.id_pos.get(&id) {
            self.tombstones[old] = 1;
        }
        let pos = self.ids.len();
        self.ids.push(id);
        self.vectors.extend_from_slice(&vec);
        self.tombstones.push(0);
        self.id_pos.insert(id, pos);
        self.dirty = true;
        Ok(())
    }

    /// Tombstone `id`'s current row, if present.
    pub fn remove(&mut self, id: &KirId) {
        if let Some(&pos) = self.id_pos.get(id) {
            self.tombstones[pos] = 1;
            self.id_pos.remove(id);
            self.dirty = true;
        }
    }

    /// Brute-force top-`k` by cosine (= dot product, since everything is L2-normalized), best
    /// first, tombstoned rows skipped. Returns `Err` on a query-dim mismatch — the caller
    /// (`FactLedger::retrieve`) checks first and skips the arm rather than propagating.
    pub fn query(&self, q: &[f32], k: usize) -> Result<Vec<(KirId, f32)>, LedgerError> {
        if q.len() != self.dim {
            return Err(verr(format!(
                "query dim {} != index dim {}",
                q.len(),
                self.dim
            )));
        }
        // Rows are stored L2-normalized; normalize the query too so the dot product is cosine
        // regardless of what the embedding provider returned.
        let mut qn = q.to_vec();
        ekos_normalize(&mut qn);
        let mut scored: Vec<(KirId, f32)> = Vec::new();
        for (i, id) in self.ids.iter().enumerate() {
            if self.tombstones[i] == 1 {
                continue;
            }
            let row = &self.vectors[i * self.dim..(i + 1) * self.dim];
            let dot: f32 = row.iter().zip(&qn).map(|(a, b)| a * b).sum();
            scored.push((*id, dot));
        }
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        Ok(scored)
    }

    /// Whether `compact()` is worth calling.
    pub fn should_compact(&self) -> bool {
        !self.ids.is_empty()
            && self.tombstoned() as f32 / self.ids.len() as f32 > COMPACT_TOMBSTONE_RATIO
    }

    /// Drop every tombstoned row and renumber. Persists.
    pub fn compact(&mut self) -> Result<(), LedgerError> {
        let mut ids = Vec::new();
        let mut vectors = Vec::new();
        for (i, id) in self.ids.iter().enumerate() {
            if self.tombstones[i] == 0 {
                ids.push(*id);
                vectors.extend_from_slice(&self.vectors[i * self.dim..(i + 1) * self.dim]);
            }
        }
        self.tombstones = vec![0; ids.len()];
        self.id_pos = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        self.ids = ids;
        self.vectors = vectors;
        self.dirty = true;
        self.flush(None)
    }

    /// Write every file. `last_tx`, if given, records how far the embed pass has processed.
    pub fn flush(&mut self, last_tx: Option<TxId>) -> Result<(), LedgerError> {
        let id_bytes: Vec<u8> = self.ids.iter().flat_map(|id| *id.0.as_bytes()).collect();
        let vec_bytes: Vec<u8> = self.vectors.iter().flat_map(|f| f.to_le_bytes()).collect();

        std::fs::write(self.dir.join("ids.bin"), &id_bytes).map_err(LedgerError::Io)?;
        std::fs::write(self.dir.join("vectors.f32"), &vec_bytes).map_err(LedgerError::Io)?;
        std::fs::write(self.dir.join("tombstones.bin"), &self.tombstones)
            .map_err(LedgerError::Io)?;

        let meta = Meta {
            format_version: FORMAT_VERSION,
            dim: self.dim,
            model: self.model.clone(),
            metric: "cosine".to_string(),
            count: self.ids.len(),
            normalized: true,
        };
        std::fs::write(
            self.dir.join("meta.json"),
            serde_json::to_vec_pretty(&meta).map_err(verr)?,
        )
        .map_err(LedgerError::Io)?;

        if let Some(tx) = last_tx {
            std::fs::write(self.dir.join("last_tx"), tx.0.to_string()).map_err(LedgerError::Io)?;
        }
        self.dirty = false;
        Ok(())
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

/// L2-normalize in place — kept private-ish to avoid a name clash with `ekos_recovery::l2_normalize`
/// (the two crates don't depend on each other).
fn ekos_normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn v(vals: &[f32]) -> Vec<f32> {
        vals.to_vec()
    }

    #[test]
    fn query_returns_planted_nearest_first() {
        let dir = tempdir().unwrap();
        let (mut idx, marker) = VectorIndex::open(dir.path(), 3, "m").unwrap();
        assert!(marker.is_none());

        let a = KirId::new();
        let b = KirId::new();
        let c = KirId::new();
        idx.upsert(a, v(&[1.0, 0.0, 0.0])).unwrap();
        idx.upsert(b, v(&[0.0, 1.0, 0.0])).unwrap();
        idx.upsert(c, v(&[0.9, 0.1, 0.0])).unwrap();
        idx.flush(Some(TxId(5))).unwrap();

        let hits = idx.query(&[1.0, 0.0, 0.0], 2).unwrap();
        assert_eq!(hits[0].0, a);
        assert_eq!(hits[1].0, c);
    }

    #[test]
    fn reopen_round_trips_and_keeps_watermark() {
        let dir = tempdir().unwrap();
        let id = KirId::new();
        {
            let (mut idx, _) = VectorIndex::open(dir.path(), 4, "m").unwrap();
            idx.upsert(id, v(&[1.0, 2.0, 3.0, 4.0])).unwrap();
            idx.flush(Some(TxId(9))).unwrap();
        }
        let (idx, marker) = VectorIndex::open(dir.path(), 4, "m").unwrap();
        assert_eq!(marker, Some(TxId(9)));
        assert_eq!(idx.len(), 1);
        let hits = idx.query(&[1.0, 2.0, 3.0, 4.0], 1).unwrap();
        assert_eq!(hits[0].0, id);
        assert!((hits[0].1 - 1.0).abs() < 1e-5, "normalized self-cosine ≈ 1");
    }

    #[test]
    fn upsert_tombstones_the_old_row() {
        let dir = tempdir().unwrap();
        let (mut idx, _) = VectorIndex::open(dir.path(), 2, "m").unwrap();
        let id = KirId::new();
        idx.upsert(id, v(&[1.0, 0.0])).unwrap();
        idx.upsert(id, v(&[0.0, 1.0])).unwrap();
        assert_eq!(idx.len(), 1, "still one live row");
        let hits = idx.query(&[0.0, 1.0], 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, id);
        assert!(hits[0].1 > 0.99, "returns the new vector");
    }

    #[test]
    fn compact_drops_tombstones() {
        let dir = tempdir().unwrap();
        let (mut idx, _) = VectorIndex::open(dir.path(), 2, "m").unwrap();
        let keep = KirId::new();
        idx.upsert(keep, v(&[1.0, 0.0])).unwrap();
        for _ in 0..5 {
            let g = KirId::new();
            idx.upsert(g, v(&[0.0, 1.0])).unwrap();
            idx.remove(&g);
        }
        assert!(idx.should_compact());
        idx.compact().unwrap();
        assert_eq!(idx.ids.len(), 1);
        assert!(!idx.should_compact());
        assert_eq!(idx.query(&[1.0, 0.0], 5).unwrap()[0].0, keep);
    }

    #[test]
    fn dim_or_model_mismatch_wipes_on_open() {
        let dir = tempdir().unwrap();
        {
            let (mut idx, _) = VectorIndex::open(dir.path(), 3, "old-model").unwrap();
            idx.upsert(KirId::new(), v(&[1.0, 0.0, 0.0])).unwrap();
            idx.flush(Some(TxId(3))).unwrap();
        }
        // same dim, new model → wipe
        let (idx, marker) = VectorIndex::open(dir.path(), 3, "new-model").unwrap();
        assert!(marker.is_none());
        assert_eq!(idx.len(), 0);

        // new dim → wipe
        let (idx, marker) = VectorIndex::open(dir.path(), 8, "new-model").unwrap();
        assert!(marker.is_none());
        assert_eq!(idx.len(), 0);
    }
}
