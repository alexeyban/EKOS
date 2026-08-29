//! RFC 0016 Phase 2 — the fact-segment storage format.
//!
//! Facts commit in **batches** (one `append_*` call = one batch = one `tx`).
//! Batches append as checksummed frames to the *active* segment file; when it
//! grows past the seal threshold it is fsynced, SHA-256-hashed, recorded in
//! the manifest, and never written again. On-disk layout:
//!
//! ```text
//! <root>/
//!   manifest.json          the only long-lived mutable file (tmp + atomic rename)
//!   HEAD                   committed-length watermark for the active segment
//!   segments/seg-000000.facts
//!   segments/seg-000001.facts
//! ```
//!
//! Frame layout (integers little-endian):
//!
//! ```text
//! [u32 frame_len]   bytes after this field
//! [u8  version=1]
//! [u64 tx]
//! [i64 wall_time_us]
//! [zstd+checksum(JSON-serialized fact ops)]
//! ```
//!
//! Durability and visibility (RFC 0016 §3, review note 1):
//!
//! - Every committed batch fsyncs the active segment **then** publishes the
//!   committed length to `HEAD` (write-temp + atomic rename). Readers see
//!   sealed segments plus the active segment *up to the watermark* — the
//!   visibility SQLite WAL gives today, without waiting for a seal.
//! - A crash between fsync and `HEAD` publish loses nothing: recovery scans
//!   the active segment past the watermark, keeps every valid frame, and
//!   republishes. A torn trailing frame is truncated away.
//! - Sealed segments are verified wholesale against their manifest hash.
//!
//! `tx` is the ordering authority (monotone, assigned by the store); wall
//! time is metadata carried per batch for as-of queries.

pub mod map;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

use crate::fact::{AttributeRegistry, Fact, FactOp, TxId};
use ekos_segment_backend::{BackendError, LocalFsBackend, SegmentBackend};
use map::MappedSegment;

/// The backend-relative key for sealed segment `seq`.
fn seg_key(seq: u32) -> String {
    format!("segments/seg-{seq:06}.facts")
}

/// Active segment seals at this size (RFC 0016 §3).
pub const SEGMENT_SEAL_BYTES: u64 = 8 * 1024 * 1024;
/// Batch frame format version.
const FRAME_VERSION: u8 = 2; // v2: body carries a dictionary-version byte
/// Fixed frame header past the length field: version + tx + wall_time_us.
const FRAME_HEADER: usize = 1 + 8 + 8;

#[derive(Debug, Error)]
pub enum SegmentError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("corrupt segment store: {0}")]
    Corrupt(String),
    #[error(transparent)]
    Backend(#[from] BackendError),
}

/// One committed transaction: the ordered assert/retract facts it wrote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Batch {
    pub tx: TxId,
    /// Wall-clock metadata for as-of queries; never an ordering authority.
    pub wall_time_us: i64,
    pub ops: Vec<(FactOp, Fact)>,
}

/// A sealed, immutable segment as recorded in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedSegment {
    pub seq: u32,
    pub len: u64,
    pub sha256: String,
    pub tx_min: TxId,
    pub tx_max: TxId,
    pub batches: u64,
}

/// One sealed segment's verification result (RFC 0105 Phase 2's repair-tool report) — `ok: false`
/// means real, unrecoverable local corruption (a hash mismatch or an unreadable file); `detail` is
/// human-readable, meant to be printed directly by a repair command, not parsed.
#[derive(Debug, Clone)]
pub struct SealedSegmentCheck {
    pub seq: u32,
    pub tx_min: TxId,
    pub tx_max: TxId,
    pub ok: bool,
    pub detail: String,
}

/// The store's only long-lived mutable file. Updated by write-temp + atomic
/// rename; everything it references is immutable.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    pub sealed: Vec<SealedSegment>,
    /// Attribute interner shared by every fact in the store (append-only).
    pub attributes: AttributeRegistry,
    /// Version of the batch-body compression dictionary in `dict.bin`
    /// (RFC 0016 §7); `None` → frames use dictionary byte 0 (plain zstd).
    #[serde(default)]
    pub dict_version: Option<u8>,
}

/// Committed-length watermark for the active segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Head {
    active_seq: u32,
    committed_len: u64,
}

/// A fact-segment store rooted at one directory: sealed history + one active
/// segment + watermark. Single writer (the caller ensures it, as with the
/// SQLite ledger today); readers open independently and see committed data.
pub struct SegmentStore {
    root: PathBuf,
    /// Where sealed segment objects are published and fetched (RFC 0113 B1). Defaults to
    /// [`LocalFsBackend`] — today's exact behaviour; a remote backend slots in for RFC 0111's
    /// Distributed mode. The active segment, `HEAD`, `manifest.json`, and `dict.bin` stay local
    /// regardless.
    backend: Arc<dyn SegmentBackend>,
    pub manifest: Manifest,
    head: Head,
    active: File,
    next_tx: u64,
    seal_bytes: u64,
    dict: Option<SegDict>,
}

/// Prepared batch-body compression dictionary (RFC 0016 §7).
struct SegDict {
    version: u8,
    enc: zstd::dict::EncoderDictionary<'static>,
    dec: zstd::dict::DecoderDictionary<'static>,
}

/// Batch bodies are written once; spend compression effort there.
const BODY_ZSTD_LEVEL: i32 = 19;

fn build_dict(version: u8, bytes: &[u8]) -> SegDict {
    SegDict {
        version,
        enc: zstd::dict::EncoderDictionary::copy(bytes, BODY_ZSTD_LEVEL),
        dec: zstd::dict::DecoderDictionary::copy(bytes),
    }
}

impl SegmentStore {
    /// Open (or create) a store, running crash recovery on the active
    /// segment: valid frames past the watermark are kept and republished; a
    /// torn tail is truncated.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, SegmentError> {
        Self::open_with_seal_threshold(root, SEGMENT_SEAL_BYTES)
    }

    /// `open` with a custom seal threshold — used by tests to exercise
    /// sealing without writing megabytes.
    pub fn open_with_seal_threshold(
        root: impl Into<PathBuf>,
        seal_bytes: u64,
    ) -> Result<Self, SegmentError> {
        let root = root.into();
        let backend: Arc<dyn SegmentBackend> = Arc::new(LocalFsBackend::new(&root));
        Self::open_with_backend(root, backend, seal_bytes)
    }

    /// `open_with_seal_threshold` with an explicit sealed-segment backend (RFC 0113 B1). Every
    /// other constructor defaults it to [`LocalFsBackend`] — today's exact behaviour.
    pub fn open_with_backend(
        root: impl Into<PathBuf>,
        backend: Arc<dyn SegmentBackend>,
        seal_bytes: u64,
    ) -> Result<Self, SegmentError> {
        let root = root.into();
        std::fs::create_dir_all(root.join("segments"))?;

        let manifest = load_manifest(&root)?;
        let dict = match manifest.dict_version {
            Some(v) => Some(build_dict(v, &std::fs::read(root.join("dict.bin"))?)),
            None => None,
        };
        let active_seq = manifest.sealed.last().map(|s| s.seq + 1).unwrap_or(0);

        let head_path = root.join("HEAD");
        let stored_head: Option<Head> = std::fs::read(&head_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        if let Some(h) = stored_head
            && h.active_seq != active_seq
        {
            return Err(SegmentError::Corrupt(format!(
                "HEAD names segment {} but the manifest implies active segment {}",
                h.active_seq, active_seq
            )));
        }

        let active_path = segment_path(&root, active_seq);
        let mut active = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&active_path)?;

        // Recovery: scan the whole active file. Every valid frame — including
        // ones committed after the last HEAD publish — is kept; the first
        // invalid byte truncates the rest. The active segment is read, never
        // mapped (it is the one file that grows and can be truncated here).
        let mut bytes = Vec::new();
        active.read_to_end(&mut bytes)?;
        let (valid_len, batches) = scan_slice(&bytes, dict.as_ref());
        if valid_len < active.metadata()?.len() {
            tracing::warn!(
                segment = active_seq,
                offset = valid_len,
                "truncating torn batch frame at active segment tail (crash recovery)"
            );
            active.set_len(valid_len)?;
            active.sync_all()?;
        }

        let head = Head {
            active_seq,
            committed_len: valid_len,
        };
        write_head(&root, head)?;

        let sealed_tx_max = manifest.sealed.last().map(|s| s.tx_max.0);
        let active_tx_max = batches.last().map(|b| b.tx.0);
        let next_tx = sealed_tx_max.max(active_tx_max).map(|t| t + 1).unwrap_or(0);

        Ok(Self {
            root,
            backend,
            manifest,
            head,
            active,
            next_tx,
            seal_bytes,
            dict,
        })
    }

    /// Install the batch-body compression dictionary (RFC 0016 §7). Must be
    /// called on an empty store — before the first batch — so every frame in
    /// the store decodes with one dictionary generation.
    pub fn set_dictionary(&mut self, bytes: Vec<u8>) -> Result<(), SegmentError> {
        if self.head.committed_len != 0 || !self.manifest.sealed.is_empty() {
            return Err(SegmentError::Corrupt(
                "dictionary must be installed before any batch is written".into(),
            ));
        }
        std::fs::write(self.root.join("dict.bin"), &bytes)?;
        self.manifest.dict_version = Some(1);
        save_manifest(&self.root, &self.manifest)?;
        self.dict = Some(build_dict(1, &bytes));
        Ok(())
    }

    /// Commit one batch: assign the next `tx`, append the frame, fsync,
    /// publish the watermark, and seal + roll if the threshold is crossed.
    pub fn append(
        &mut self,
        ops: Vec<(FactOp, Fact)>,
        wall_time_us: i64,
    ) -> Result<TxId, SegmentError> {
        self.append_with_seal(ops, wall_time_us).map(|(tx, _)| tx)
    }

    /// [`Self::append`], additionally reporting whether this commit sealed a
    /// segment — the owner's cue to flush derived indexes (RFC 0016 Phase 6).
    pub fn append_with_seal(
        &mut self,
        ops: Vec<(FactOp, Fact)>,
        wall_time_us: i64,
    ) -> Result<(TxId, bool), SegmentError> {
        let tx = TxId(self.next_tx);
        let batch = Batch {
            tx,
            wall_time_us,
            ops,
        };
        let frame = self.encode_frame(&batch)?;

        self.active.write_all(&frame)?;
        self.active.sync_all()?;
        self.next_tx += 1;
        self.head.committed_len += frame.len() as u64;
        write_head(&self.root, self.head)?;

        let mut sealed = false;
        if self.head.committed_len >= self.seal_bytes {
            self.seal_active()?;
            sealed = true;
        }
        Ok((tx, sealed))
    }

    /// Seal the active segment (hash it into the manifest) and start a fresh
    /// one. A no-op on an empty active segment.
    pub fn seal_active(&mut self) -> Result<(), SegmentError> {
        if self.head.committed_len == 0 {
            return Ok(());
        }
        self.active.sync_all()?;

        let seq = self.head.active_seq;
        let path = segment_path(&self.root, seq);
        let (sha256, len) = hash_file(&path)?;
        if len != self.head.committed_len {
            return Err(SegmentError::Corrupt(format!(
                "segment {seq} is {len} bytes but the watermark says {}",
                self.head.committed_len
            )));
        }

        let (tx_min, tx_max, batch_count) = {
            let map = MappedSegment::open(&path, len)?;
            let (_, headers) = scan_headers_slice(map.bytes());
            match (headers.first(), headers.last()) {
                (Some(first), Some(last)) => (first.0, last.0, headers.len() as u64),
                _ => {
                    return Err(SegmentError::Corrupt(format!(
                        "sealed segment {seq} has no batches"
                    )));
                }
            }
        };
        // The local active file is now frozen and validated — publish it as an immutable object
        // (RFC 0113 B1). LocalFsBackend: an fsync; a remote backend: a PUT.
        self.backend.publish_sealed(&seg_key(seq), &path)?;
        self.manifest.sealed.push(SealedSegment {
            seq,
            len,
            sha256,
            tx_min,
            tx_max,
            batches: batch_count,
        });
        save_manifest(&self.root, &self.manifest)?;

        let new_seq = seq + 1;
        self.active = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(segment_path(&self.root, new_seq))?;
        self.head = Head {
            active_seq: new_seq,
            committed_len: 0,
        };
        write_head(&self.root, self.head)?;
        Ok(())
    }

    /// Every committed batch in tx order: sealed segments (mmap'd) first,
    /// then the active segment (read, never mapped) up to the watermark.
    pub fn batches(&self) -> Result<Vec<Batch>, SegmentError> {
        self.batches_after(None)
    }

    /// Committed batches with `tx > after` (all of them when `None`).
    /// Frame headers are always walked, but batch *bodies* are only
    /// decompressed past the cutoff — the cheap catch-up read.
    pub fn batches_after(&self, after: Option<TxId>) -> Result<Vec<Batch>, SegmentError> {
        let mut out = Vec::new();
        let keep = |tx: TxId| after.is_none_or(|a| tx > a);
        for sealed in &self.manifest.sealed {
            // Whole sealed segments before the cutoff are skipped outright.
            if !keep(sealed.tx_max) {
                continue;
            }
            let map = MappedSegment::open(&self.backend.fetch(&seg_key(sealed.seq))?, sealed.len)?;
            let (valid, batches) = scan_batches_filtered(map.bytes(), &keep, self.dict.as_ref());
            if valid != sealed.len {
                return Err(SegmentError::Corrupt(format!(
                    "sealed segment {} has invalid frames at offset {valid}",
                    sealed.seq
                )));
            }
            out.extend(batches);
        }
        out.extend(self.active_batches(&keep)?);
        Ok(out)
    }

    /// Frame headers `(tx, wall_time_us)` of every committed batch, in tx
    /// order — no body decompression. This is how owners rebuild the
    /// time→tx map without replaying content (RFC 0016 §2).
    pub fn batch_headers(&self) -> Result<Vec<(TxId, i64)>, SegmentError> {
        let mut out = Vec::new();
        for sealed in &self.manifest.sealed {
            let map = MappedSegment::open(&self.backend.fetch(&seg_key(sealed.seq))?, sealed.len)?;
            let (valid, headers) = scan_headers_slice(map.bytes());
            if valid != sealed.len {
                return Err(SegmentError::Corrupt(format!(
                    "sealed segment {} has invalid frames at offset {valid}",
                    sealed.seq
                )));
            }
            out.extend(headers);
        }
        let bytes = self.read_active_committed()?;
        let (_, headers) = scan_headers_slice(&bytes);
        out.extend(headers);
        Ok(out)
    }

    fn read_active_committed(&self) -> Result<Vec<u8>, SegmentError> {
        let mut file = File::open(segment_path(&self.root, self.head.active_seq))?;
        let mut bytes = Vec::with_capacity(self.head.committed_len as usize);
        file.read_to_end(&mut bytes)?;
        bytes.truncate(self.head.committed_len as usize);
        Ok(bytes)
    }

    fn active_batches(&self, keep: &dyn Fn(TxId) -> bool) -> Result<Vec<Batch>, SegmentError> {
        let bytes = self.read_active_committed()?;
        let (_, batches) = scan_batches_filtered(&bytes, keep, self.dict.as_ref());
        Ok(batches)
    }

    /// Verify every sealed segment against its manifest hash — fails fast at
    /// the first bad one. Implemented on top of [`Self::verify_sealed_report`]
    /// (RFC 0105 Phase 2) so there is exactly one place that decides what "a
    /// segment fails verification" means, not two independently-maintained
    /// checks that could drift.
    pub fn verify_sealed(&self) -> Result<(), SegmentError> {
        match self.verify_sealed_report().into_iter().find(|c| !c.ok) {
            Some(check) => Err(SegmentError::Corrupt(check.detail)),
            None => Ok(()),
        }
    }

    /// Verify every sealed segment against its manifest hash, unconditionally checking all of
    /// them rather than stopping at the first failure (RFC 0105 Phase 2) — the report a real
    /// repair tool needs, distinct from [`Self::verify_sealed`]'s fail-fast contract (kept for
    /// existing internal callers/tests). One row per sealed segment, in manifest order.
    pub fn verify_sealed_report(&self) -> Vec<SealedSegmentCheck> {
        self.manifest
            .sealed
            .iter()
            .map(|sealed| {
                let path = match self.backend.fetch(&seg_key(sealed.seq)) {
                    Ok(p) => p,
                    Err(e) => {
                        return SealedSegmentCheck {
                            seq: sealed.seq,
                            tx_min: sealed.tx_min,
                            tx_max: sealed.tx_max,
                            ok: false,
                            detail: format!("sealed segment {} unreachable: {e}", sealed.seq),
                        };
                    }
                };
                match hash_file(&path) {
                    Ok((sha256, len)) if len == sealed.len && sha256 == sealed.sha256 => {
                        SealedSegmentCheck {
                            seq: sealed.seq,
                            tx_min: sealed.tx_min,
                            tx_max: sealed.tx_max,
                            ok: true,
                            detail: "OK".to_string(),
                        }
                    }
                    Ok((sha256, len)) => SealedSegmentCheck {
                        seq: sealed.seq,
                        tx_min: sealed.tx_min,
                        tx_max: sealed.tx_max,
                        ok: false,
                        detail: format!(
                            "sealed segment {} fails verification (len {} vs {}, hash mismatch: \
                             {})",
                            sealed.seq,
                            len,
                            sealed.len,
                            sha256 != sealed.sha256
                        ),
                    },
                    Err(e) => SealedSegmentCheck {
                        seq: sealed.seq,
                        tx_min: sealed.tx_min,
                        tx_max: sealed.tx_max,
                        ok: false,
                        detail: format!("sealed segment {} unreadable: {e}", sealed.seq),
                    },
                }
            })
            .collect()
    }

    /// Persist the manifest now (atomic rename). Called by owners whenever
    /// manifest state outside the seal path changes — e.g. the attribute
    /// registry grew during an append.
    pub fn persist_manifest(&self) -> Result<(), SegmentError> {
        save_manifest(&self.root, &self.manifest)
    }

    /// The store's root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The next transaction number this store will assign.
    pub fn next_tx(&self) -> TxId {
        TxId(self.next_tx)
    }

    /// Committed bytes in the active segment (the watermark).
    pub fn committed_len(&self) -> u64 {
        self.head.committed_len
    }
}

// ── Frame codec ─────────────────────────────────────────────────────────────

impl SegmentStore {
    fn encode_frame(&self, batch: &Batch) -> Result<Vec<u8>, SegmentError> {
        let json = serde_json::to_vec(&batch.ops)?;
        let (dict_byte, body) = match &self.dict {
            Some(d) => {
                let mut enc =
                    zstd::stream::write::Encoder::with_prepared_dictionary(Vec::new(), &d.enc)?;
                enc.include_checksum(true)?;
                enc.write_all(&json)?;
                (d.version, enc.finish()?)
            }
            None => {
                let mut enc = zstd::stream::write::Encoder::new(
                    Vec::new(),
                    ekos_common::compress::ZSTD_LEVEL,
                )?;
                enc.include_checksum(true)?;
                enc.write_all(&json)?;
                (0u8, enc.finish()?)
            }
        };

        let frame_len = (FRAME_HEADER + 1 + body.len()) as u32;
        let mut frame = Vec::with_capacity(4 + frame_len as usize);
        frame.extend_from_slice(&frame_len.to_le_bytes());
        frame.push(FRAME_VERSION);
        frame.extend_from_slice(&batch.tx.0.to_le_bytes());
        frame.extend_from_slice(&batch.wall_time_us.to_le_bytes());
        frame.push(dict_byte);
        frame.extend_from_slice(&body);
        Ok(frame)
    }
}

/// Walk frame boundaries in `bytes`, calling `visit(frame)` for each valid
/// frame; a `visit` returning `false` (or any invalid/truncated frame) ends
/// the walk at the last good boundary — never an error, because a torn tail
/// is the expected crash artifact. Returns the valid prefix length.
fn walk_frames(bytes: &[u8], mut visit: impl FnMut(&[u8]) -> bool) -> u64 {
    let end = bytes.len() as u64;
    let mut pos = 0u64;
    while pos + 4 <= end {
        let at = pos as usize;
        let frame_len = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as u64;
        if frame_len < FRAME_HEADER as u64 || pos + 4 + frame_len > end {
            break;
        }
        let frame = &bytes[at + 4..at + 4 + frame_len as usize];
        if !visit(frame) {
            break;
        }
        pos += 4 + frame_len;
    }
    pos
}

/// Decode every valid frame from the start of `bytes`.
fn scan_slice(bytes: &[u8], dict: Option<&SegDict>) -> (u64, Vec<Batch>) {
    scan_batches_filtered(bytes, &|_| true, dict)
}

/// Decode frames, decompressing bodies only for batches `keep` selects —
/// header validation still walks every frame.
fn scan_batches_filtered(
    bytes: &[u8],
    keep: &dyn Fn(TxId) -> bool,
    dict: Option<&SegDict>,
) -> (u64, Vec<Batch>) {
    let mut batches = Vec::new();
    let valid = walk_frames(bytes, |frame| {
        let Some((tx, _)) = decode_header(frame) else {
            return false;
        };
        if keep(tx) {
            match decode_frame(frame, dict) {
                Some(batch) => batches.push(batch),
                None => return false,
            }
        }
        true
    });
    (valid, batches)
}

/// Header-only walk: `(tx, wall_time_us)` per frame, no body decompression.
fn scan_headers_slice(bytes: &[u8]) -> (u64, Vec<(TxId, i64)>) {
    let mut headers = Vec::new();
    let valid = walk_frames(bytes, |frame| match decode_header(frame) {
        Some(h) => {
            headers.push(h);
            true
        }
        None => false,
    });
    (valid, headers)
}

fn decode_header(frame: &[u8]) -> Option<(TxId, i64)> {
    if frame.len() < FRAME_HEADER || frame[0] != FRAME_VERSION {
        return None;
    }
    let tx = u64::from_le_bytes(frame[1..9].try_into().ok()?);
    let wall_time_us = i64::from_le_bytes(frame[9..17].try_into().ok()?);
    Some((TxId(tx), wall_time_us))
}

fn decode_frame(frame: &[u8], dict: Option<&SegDict>) -> Option<Batch> {
    let (tx, wall_time_us) = decode_header(frame)?;
    let body = frame.get(FRAME_HEADER..)?;
    let (&dict_byte, compressed) = body.split_first()?;

    let mut json = Vec::new();
    match (dict_byte, dict) {
        (0, _) => {
            zstd::stream::read::Decoder::new(compressed)
                .ok()?
                .read_to_end(&mut json)
                .ok()?;
        }
        (v, Some(d)) if v == d.version => {
            zstd::stream::read::Decoder::with_prepared_dictionary(compressed, &d.dec)
                .ok()?
                .read_to_end(&mut json)
                .ok()?;
        }
        _ => return None, // unknown dictionary generation
    }
    let ops: Vec<(FactOp, Fact)> = serde_json::from_slice(&json).ok()?;
    Some(Batch {
        tx: TxId(tx.0),
        wall_time_us,
        ops,
    })
}

// ── Files ───────────────────────────────────────────────────────────────────

fn segment_path(root: &Path, seq: u32) -> PathBuf {
    root.join("segments").join(format!("seg-{seq:06}.facts"))
}

fn load_manifest(root: &Path) -> Result<Manifest, SegmentError> {
    let path = root.join("manifest.json");
    if !path.exists() {
        return Ok(Manifest {
            format_version: 1,
            ..Default::default()
        });
    }
    let mut manifest: Manifest = serde_json::from_slice(&std::fs::read(&path)?)?;
    manifest.attributes.reindex();
    Ok(manifest)
}

fn save_manifest(root: &Path, manifest: &Manifest) -> Result<(), SegmentError> {
    atomic_write(root, "manifest.json", &serde_json::to_vec(manifest)?)
}

fn write_head(root: &Path, head: Head) -> Result<(), SegmentError> {
    atomic_write(root, "HEAD", &serde_json::to_vec(&head)?)
}

/// Write-temp + fsync + atomic rename + directory fsync.
fn atomic_write(root: &Path, name: &str, bytes: &[u8]) -> Result<(), SegmentError> {
    let tmp = root.join(format!("{name}.tmp"));
    {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, root.join(name))?;
    if let Ok(dir) = File::open(root) {
        let _ = dir.sync_all();
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<(String, u64), SegmentError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut len = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        len += n as u64;
    }
    Ok((hex::encode(hasher.finalize()), len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact::{FactValue, decompose};
    use ekos_kir::{KirObject, ObjectKind};
    use ekos_segment_backend::LocalFsBackend;
    use std::sync::Mutex;
    use tempfile::tempdir;
    use uuid::Uuid;

    /// Wraps `LocalFsBackend`, counting the sealed-object calls `SegmentStore` makes through the
    /// seam — the RFC 0113 B1 acceptance check that sealed I/O really goes through `SegmentBackend`.
    struct CountingBackend {
        inner: LocalFsBackend,
        published: Mutex<Vec<String>>,
        fetched: Mutex<Vec<String>>,
    }
    impl CountingBackend {
        fn new(root: &Path) -> Self {
            Self {
                inner: LocalFsBackend::new(root),
                published: Mutex::new(Vec::new()),
                fetched: Mutex::new(Vec::new()),
            }
        }
    }
    impl SegmentBackend for CountingBackend {
        fn publish_sealed(&self, key: &str, staged: &Path) -> Result<(), BackendError> {
            self.published.lock().unwrap().push(key.to_string());
            self.inner.publish_sealed(key, staged)
        }
        fn fetch(&self, key: &str) -> Result<PathBuf, BackendError> {
            self.fetched.lock().unwrap().push(key.to_string());
            self.inner.fetch(key)
        }
        fn get(&self, key: &str) -> Result<Vec<u8>, BackendError> {
            self.inner.get(key)
        }
        fn get_range(
            &self,
            key: &str,
            range: std::ops::Range<u64>,
        ) -> Result<Vec<u8>, BackendError> {
            self.inner.get_range(key, range)
        }
        fn list(&self, p: &str) -> Result<Vec<String>, BackendError> {
            self.inner.list(p)
        }
        fn exists(&self, k: &str) -> Result<bool, BackendError> {
            self.inner.exists(k)
        }
        fn delete(&self, k: &str) -> Result<(), BackendError> {
            self.inner.delete(k)
        }
    }

    #[test]
    fn sealed_io_routes_through_the_segment_backend() {
        let dir = tempdir().unwrap();
        let backend = Arc::new(CountingBackend::new(dir.path()));
        let mut reg = AttributeRegistry::new();

        let mut store = SegmentStore::open_with_backend(dir.path(), backend.clone(), 1).unwrap(); // seal every batch
        for i in 0..3 {
            store.append(sample_ops(&mut reg, i), i as i64).unwrap();
        }
        // three batches, seal threshold 1 → three sealed segments, each published once
        assert_eq!(
            *backend.published.lock().unwrap(),
            vec![seg_key(0), seg_key(1), seg_key(2)]
        );

        // a replay reads every sealed segment through `fetch`
        backend.fetched.lock().unwrap().clear();
        let batches = store.batches().unwrap();
        assert_eq!(batches.len(), 3);
        let fetched = backend.fetched.lock().unwrap().clone();
        assert!(fetched.contains(&seg_key(0)) && fetched.contains(&seg_key(2)));

        // verify + reopen still work through the seam
        store.verify_sealed().unwrap();
        drop(store);
        let reopened = SegmentStore::open_with_backend(
            dir.path(),
            Arc::new(CountingBackend::new(dir.path())),
            1,
        )
        .unwrap();
        assert_eq!(reopened.batches().unwrap().len(), 3);
        assert_eq!(reopened.next_tx(), TxId(3));
    }

    /// RFC 0113 B2 acceptance: a full `SegmentStore` write / seal / drop / **reopen** / read
    /// round-trip works with the ledger's data living only in `object_store` (`InMemory` here) —
    /// sealed segments are `put` there and `fetch`ed back into a cache dir to be mmap'd, and the
    /// reopened store replays every batch and continues its tx sequence.
    #[test]
    fn segment_store_round_trips_on_object_store_backend() {
        use ekos_segment_backend::ObjectStoreBackend;
        use ekos_segment_backend::object_store::memory::InMemory;

        let store_root = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let object_store = std::sync::Arc::new(InMemory::new());
        let mk = || -> Arc<dyn SegmentBackend> {
            Arc::new(ObjectStoreBackend::new(object_store.clone(), "part-x", cache.path()).unwrap())
        };

        let mut reg = AttributeRegistry::new();
        let want: Vec<Vec<(FactOp, Fact)>> = {
            let mut s = SegmentStore::open_with_backend(store_root.path(), mk(), 1).unwrap();
            let mut out = Vec::new();
            for i in 0..4 {
                let ops = sample_ops(&mut reg, i);
                s.append(ops.clone(), 100 + i as i64).unwrap();
                out.push(ops);
            }
            out
        };

        // Nuke the local cache: the reopen must fetch every sealed segment from object storage.
        std::fs::remove_dir_all(cache.path()).unwrap();
        std::fs::create_dir_all(cache.path()).unwrap();

        let reopened = SegmentStore::open_with_backend(store_root.path(), mk(), 1).unwrap();
        reopened.verify_sealed().unwrap();
        let batches = reopened.batches().unwrap();

        assert_eq!(batches.len(), 4);
        for (i, (b, ops)) in batches.iter().zip(&want).enumerate() {
            assert_eq!(b.tx, TxId(i as u64));
            assert_eq!(b.wall_time_us, 100 + i as i64);
            assert_eq!(&b.ops, ops);
        }
        assert_eq!(reopened.next_tx(), TxId(4));
        assert!(mk().exists("segments/seg-000000.facts").unwrap());
    }

    fn sample_ops(registry: &mut AttributeRegistry, n: usize) -> Vec<(FactOp, Fact)> {
        let obj = KirObject::new(format!("table_{n}"), ObjectKind::Table)
            .with_property("size_bytes", serde_json::json!(n));
        decompose(obj.id.0, &serde_json::to_value(&obj).unwrap(), registry)
            .unwrap()
            .into_iter()
            .map(|f| (FactOp::Assert, f))
            .collect()
    }

    #[test]
    fn append_and_replay_across_reopen() {
        let dir = tempdir().unwrap();
        let mut reg = AttributeRegistry::new();
        {
            let mut store = SegmentStore::open(dir.path()).unwrap();
            for i in 0..5 {
                let tx = store
                    .append(sample_ops(&mut reg, i), 1_000 + i as i64)
                    .unwrap();
                assert_eq!(tx.0, i as u64, "tx is monotone from 0");
            }
        }
        let store = SegmentStore::open(dir.path()).unwrap();
        let batches = store.batches().unwrap();
        assert_eq!(batches.len(), 5);
        assert_eq!(batches[4].tx, TxId(4));
        assert_eq!(batches[2].wall_time_us, 1_002);
        assert_eq!(store.next_tx(), TxId(5), "tx continues after reopen");
        assert!(!batches[0].ops.is_empty());
    }

    #[test]
    fn seal_rolls_to_new_segment_and_verifies() {
        let dir = tempdir().unwrap();
        let mut reg = AttributeRegistry::new();
        // Tiny threshold: every batch seals its segment.
        let mut store = SegmentStore::open_with_seal_threshold(dir.path(), 1).unwrap();
        for i in 0..3 {
            store.append(sample_ops(&mut reg, i), i as i64).unwrap();
        }
        assert_eq!(store.manifest.sealed.len(), 3);
        assert_eq!(store.manifest.sealed[1].tx_min, TxId(1));
        store.verify_sealed().unwrap();
        assert_eq!(store.batches().unwrap().len(), 3);

        // Reopen: sealed history intact, next tx continues.
        drop(store);
        let store = SegmentStore::open_with_seal_threshold(dir.path(), 1).unwrap();
        assert_eq!(store.batches().unwrap().len(), 3);
        assert_eq!(store.next_tx(), TxId(3));
    }

    #[test]
    fn torn_tail_is_truncated_and_prior_batches_survive() {
        let dir = tempdir().unwrap();
        let mut reg = AttributeRegistry::new();
        {
            let mut store = SegmentStore::open(dir.path()).unwrap();
            for i in 0..3 {
                store.append(sample_ops(&mut reg, i), 0).unwrap();
            }
        }
        // Crash simulation: chop bytes off the active segment tail.
        let seg = dir.path().join("segments/seg-000000.facts");
        let len = std::fs::metadata(&seg).unwrap().len();
        OpenOptions::new()
            .write(true)
            .open(&seg)
            .unwrap()
            .set_len(len - 5)
            .unwrap();

        let mut store = SegmentStore::open(dir.path()).unwrap();
        let batches = store.batches().unwrap();
        assert_eq!(batches.len(), 2, "torn last batch dropped, rest intact");
        assert_eq!(store.next_tx(), TxId(2), "tx of the torn batch is reusable");
        // The store keeps working after recovery.
        store.append(sample_ops(&mut reg, 9), 0).unwrap();
        assert_eq!(store.batches().unwrap().len(), 3);
    }

    /// Review note 1: a crash after fsync but before the HEAD publish must
    /// lose nothing — recovery scans past the stale watermark.
    #[test]
    fn valid_frames_past_stale_watermark_are_recovered() {
        let dir = tempdir().unwrap();
        let mut reg = AttributeRegistry::new();
        {
            let mut store = SegmentStore::open(dir.path()).unwrap();
            store.append(sample_ops(&mut reg, 0), 0).unwrap();
            let after_first = store.committed_len();
            store.append(sample_ops(&mut reg, 1), 0).unwrap();
            // Simulate the crash: rewind HEAD to the first batch's watermark.
            write_head(
                dir.path(),
                Head {
                    active_seq: 0,
                    committed_len: after_first,
                },
            )
            .unwrap();
        }
        let store = SegmentStore::open(dir.path()).unwrap();
        assert_eq!(store.batches().unwrap().len(), 2, "second batch recovered");
        assert_eq!(store.next_tx(), TxId(2));
    }

    #[test]
    fn corrupted_sealed_segment_fails_verification() {
        let dir = tempdir().unwrap();
        let mut reg = AttributeRegistry::new();
        let mut store = SegmentStore::open_with_seal_threshold(dir.path(), 1).unwrap();
        store.append(sample_ops(&mut reg, 0), 0).unwrap();
        assert_eq!(store.manifest.sealed.len(), 1);

        // Flip a byte inside the sealed segment.
        let seg = dir.path().join("segments/seg-000000.facts");
        let mut bytes = std::fs::read(&seg).unwrap();
        let at = bytes.len() - 3;
        bytes[at] ^= 0xFF;
        std::fs::write(&seg, &bytes).unwrap();

        assert!(matches!(
            store.verify_sealed(),
            Err(SegmentError::Corrupt(_))
        ));
    }

    // ── RFC 0105 Phase 2: repair-tool report ────────────────────────────────

    #[test]
    fn verify_sealed_report_is_all_ok_on_a_healthy_store() {
        let dir = tempdir().unwrap();
        let mut reg = AttributeRegistry::new();
        let mut store = SegmentStore::open_with_seal_threshold(dir.path(), 1).unwrap();
        for i in 0..3 {
            store.append(sample_ops(&mut reg, i), i as i64).unwrap();
        }
        assert_eq!(store.manifest.sealed.len(), 3);

        let report = store.verify_sealed_report();
        assert_eq!(report.len(), 3);
        assert!(
            report.iter().all(|c| c.ok),
            "every segment should verify clean"
        );
    }

    #[test]
    fn verify_sealed_report_names_exactly_the_corrupt_segment_others_still_ok() {
        let dir = tempdir().unwrap();
        let mut reg = AttributeRegistry::new();
        let mut store = SegmentStore::open_with_seal_threshold(dir.path(), 1).unwrap();
        for i in 0..3 {
            store.append(sample_ops(&mut reg, i), i as i64).unwrap();
        }
        assert_eq!(store.manifest.sealed.len(), 3);
        let corrupt_seq = store.manifest.sealed[1].seq;

        // Flip a byte inside only the middle sealed segment.
        let seg = segment_path(dir.path(), corrupt_seq);
        let mut bytes = std::fs::read(&seg).unwrap();
        let at = bytes.len() - 3;
        bytes[at] ^= 0xFF;
        std::fs::write(&seg, &bytes).unwrap();

        let report = store.verify_sealed_report();
        assert_eq!(
            report.len(),
            3,
            "the report must still cover every segment, not stop at the first failure"
        );
        let bad: Vec<_> = report.iter().filter(|c| !c.ok).collect();
        assert_eq!(
            bad.len(),
            1,
            "exactly the one corrupted segment should fail"
        );
        assert_eq!(bad[0].seq, corrupt_seq);
        let ok_seqs: Vec<u32> = report.iter().filter(|c| c.ok).map(|c| c.seq).collect();
        assert_eq!(
            ok_seqs.len(),
            2,
            "the other two segments must still report OK — the report is partial, not all-or-nothing"
        );
    }

    #[test]
    fn manifest_attribute_registry_round_trips() {
        let dir = tempdir().unwrap();
        let mut store = SegmentStore::open(dir.path()).unwrap();
        let a = store.manifest.attributes.intern("name");
        let b = store.manifest.attributes.intern("properties.path");
        save_manifest(dir.path(), &store.manifest).unwrap();
        store.append(vec![], 0).unwrap(); // force a HEAD write alongside

        let store = SegmentStore::open(dir.path()).unwrap();
        let mut attrs = store.manifest.attributes.clone();
        assert_eq!(attrs.intern("name"), a, "interned ids survive reopen");
        assert_eq!(attrs.intern("properties.path"), b);
    }

    #[test]
    fn batch_round_trip_preserves_ops_exactly() {
        let dir = tempdir().unwrap();
        let mut store = SegmentStore::open(dir.path()).unwrap();
        let entity = Uuid::new_v4();
        let attr = store.manifest.attributes.intern("properties.size_bytes");
        let ops = vec![
            (
                FactOp::Retract,
                Fact {
                    entity,
                    attr,
                    pos: None,
                    value: FactValue::Number(100.into()),
                },
            ),
            (
                FactOp::Assert,
                Fact {
                    entity,
                    attr,
                    pos: None,
                    value: FactValue::Number(200.into()),
                },
            ),
        ];
        store.append(ops.clone(), 42).unwrap();

        let batches = store.batches().unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(
            batches[0].ops, ops,
            "assert/retract ops survive the frame codec"
        );
        assert_eq!(batches[0].wall_time_us, 42);
    }
}
