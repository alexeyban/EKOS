//! EKOS Pack v1 — packed artifact segments (RFC 0015).
//!
//! The loose-object store (`<2hex>/<64hex>.json`, one file per artifact)
//! wastes a filesystem block per sub-KB artifact and stores JSON
//! uncompressed. Pack v1 stores artifacts as zstd-compressed frames appended
//! to immutable segment files, mirroring git's loose→packfile evolution:
//!
//! ```text
//! <root>/pack-0000.seg     append-only, rolls at SEGMENT_ROLL_BYTES
//! <root>/pack-0001.seg
//! ```
//!
//! Frame layout (integers little-endian):
//!
//! ```text
//! [u32 frame_len = 32 + body_len][32-byte raw artifact id][zstd(compact JSON)]
//! ```
//!
//! - The embedded id guards against misdirected reads; the zstd frame carries
//!   its own checksum, so corruption is detected on read.
//! - The in-memory index is *derived*: rebuilt on every open by a
//!   header-only scan of the segments (reads 36 bytes per frame). A torn
//!   trailing frame — the only kind a crash can produce — is truncated away;
//!   everything before it is intact by construction.
//! - Loose files in the same root remain readable as a fallback, so a store
//!   can be repacked at any time (`repack_loose`) without a flag day.
//!
//! Single writer (enforced by EKOS's build flow), many readers.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::store::{ArtifactStore, StoreError};
use crate::{ArtifactId, FileSystemArtifactStore};

/// Segment size at which a new segment file is started.
const SEGMENT_ROLL_BYTES: u64 = 64 * 1024 * 1024;
/// zstd level for artifact frames. Level 3 packs the live estate in ~4.5 s;
/// level 19 was measured to take minutes for a ~1.2× ratio gain — artifacts
/// are written during builds, so write speed wins here (unlike ledger rows,
/// which are far fewer per build).
const PACK_ZSTD_LEVEL: i32 = 3;
/// Raw SHA-256 id length inside a frame.
const ID_LEN: usize = 32;
/// Smallest legal frame body: the id plus a non-empty zstd frame.
const MIN_FRAME_LEN: u32 = ID_LEN as u32 + 1;

#[derive(Debug, Clone, Copy)]
struct FrameLoc {
    segment: u32,
    /// Byte offset of the frame's length prefix within the segment.
    offset: u64,
    /// Compressed body length (frame_len minus the 32-byte id).
    body_len: u32,
}

struct PackInner {
    /// hex artifact id → frame location.
    index: HashMap<String, FrameLoc>,
    /// Current byte length of each segment file.
    segment_lens: Vec<u64>,
    /// Lazily opened append handle for the active segment.
    writer: Option<(u32, File)>,
}

/// Packed artifact store with loose-file read fallback.
pub struct PackArtifactStore {
    root: PathBuf,
    loose: FileSystemArtifactStore,
    inner: Mutex<PackInner>,
}

impl PackArtifactStore {
    /// Open (or create) a pack store rooted at `root`, deriving the frame
    /// index by scanning segment headers. Torn trailing frames left by a
    /// crash are truncated away.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;

        let mut index = HashMap::new();
        let mut segment_lens = Vec::new();
        for (segment, path) in segment_paths(&root)? {
            let len = scan_segment(&path, segment, &mut index)?;
            // Segments are numbered densely from 0.
            debug_assert_eq!(segment as usize, segment_lens.len());
            segment_lens.push(len);
        }

        Ok(Self {
            loose: FileSystemArtifactStore::new(&root),
            root,
            inner: Mutex::new(PackInner {
                index,
                segment_lens,
                writer: None,
            }),
        })
    }

    fn segment_path(&self, segment: u32) -> PathBuf {
        self.root.join(format!("pack-{segment:04}.seg"))
    }

    /// Number of packed frames (excludes loose fallback files).
    pub fn packed_count(&self) -> usize {
        self.inner.lock().unwrap().index.len()
    }

    /// Migrate every loose artifact file into pack segments. Three phases so
    /// a crash at any point loses nothing: pack + verify everything, fsync
    /// the segments, and only then delete the loose files.
    /// Returns `(migrated, already_packed)`.
    pub fn repack_loose(&self) -> Result<(usize, usize), StoreError> {
        let mut migrated = 0usize;
        let mut already = 0usize;

        let ids = self.loose.list()?;
        for id in &ids {
            let Some(value) = self.loose.read(id)? else {
                continue;
            };

            let packed = {
                let inner = self.inner.lock().unwrap();
                inner.index.contains_key(id.as_str())
            };
            if packed {
                already += 1;
                continue;
            }
            self.write_packed(id, &value)?;
            let back = self
                .read(id)?
                .ok_or_else(|| StoreError::Corrupt(format!("repacked artifact {id} unreadable")))?;
            if back != value {
                return Err(StoreError::Corrupt(format!(
                    "repacked artifact {id} does not round-trip"
                )));
            }
            migrated += 1;
        }

        // Everything is packed and verified — make it durable before any
        // loose file disappears.
        self.sync()?;
        for id in &ids {
            std::fs::remove_file(self.loose_path(id)).ok();
        }
        prune_empty_dirs(&self.root)?;
        Ok((migrated, already))
    }

    fn loose_path(&self, id: &ArtifactId) -> PathBuf {
        self.root
            .join(id.prefix())
            .join(format!("{}.json", id.as_str()))
    }

    /// Append one frame to the active segment.
    fn write_packed(
        &self,
        id: &ArtifactId,
        artifact: &serde_json::Value,
    ) -> Result<(), StoreError> {
        let raw_id = hex_id_to_raw(id)?;
        let json = serde_json::to_vec(artifact)?;
        let body = compress_frame_body(&json)?;
        let frame_len = (ID_LEN + body.len()) as u32;

        let mut inner = self.inner.lock().unwrap();

        // Pick (or roll to) the active segment.
        let segment = match inner.segment_lens.last() {
            Some(&len) if len < SEGMENT_ROLL_BYTES => inner.segment_lens.len() as u32 - 1,
            Some(_) => {
                // Fsync the finished segment before rolling past it.
                if let Some((_, file)) = inner.writer.take() {
                    file.sync_all()?;
                }
                inner.segment_lens.push(0);
                inner.segment_lens.len() as u32 - 1
            }
            None => {
                inner.segment_lens.push(0);
                0
            }
        };

        if inner.writer.as_ref().map(|(seg, _)| *seg) != Some(segment) {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.segment_path(segment))?;
            inner.writer = Some((segment, file));
        }

        let offset = inner.segment_lens[segment as usize];
        {
            let (_, file) = inner.writer.as_mut().unwrap();
            let mut frame = Vec::with_capacity(4 + ID_LEN + body.len());
            frame.extend_from_slice(&frame_len.to_le_bytes());
            frame.extend_from_slice(&raw_id);
            frame.extend_from_slice(&body);
            file.write_all(&frame)?;
        }

        inner.segment_lens[segment as usize] += 4 + frame_len as u64;
        inner.index.insert(
            id.as_str().to_string(),
            FrameLoc {
                segment,
                offset,
                body_len: body.len() as u32,
            },
        );
        Ok(())
    }

    /// Fsync the active segment. Called on drop; cheap when nothing was written.
    pub fn sync(&self) -> Result<(), StoreError> {
        let inner = self.inner.lock().unwrap();
        if let Some((_, file)) = inner.writer.as_ref() {
            file.sync_all()?;
        }
        Ok(())
    }
}

impl Drop for PackArtifactStore {
    fn drop(&mut self) {
        let _ = self.sync();
    }
}

impl ArtifactStore for PackArtifactStore {
    fn write(&self, id: &ArtifactId, artifact: &serde_json::Value) -> Result<bool, StoreError> {
        if self.exists(id) {
            return Ok(false);
        }
        self.write_packed(id, artifact)?;
        Ok(true)
    }

    fn read(&self, id: &ArtifactId) -> Result<Option<serde_json::Value>, StoreError> {
        let loc = {
            let inner = self.inner.lock().unwrap();
            inner.index.get(id.as_str()).copied()
        };
        let Some(loc) = loc else {
            return self.loose.read(id); // transition fallback
        };

        let mut file = File::open(self.segment_path(loc.segment))?;
        file.seek(SeekFrom::Start(loc.offset + 4))?;

        let mut raw_id = [0u8; ID_LEN];
        file.read_exact(&mut raw_id)?;
        if hex::encode(raw_id) != id.as_str() {
            return Err(StoreError::Corrupt(format!(
                "frame at segment {} offset {} holds a different artifact than indexed ({})",
                loc.segment, loc.offset, id
            )));
        }

        let mut body = vec![0u8; loc.body_len as usize];
        file.read_exact(&mut body)?;
        // Stream decode verifies the frame's embedded checksum.
        let mut json = Vec::new();
        zstd::stream::read::Decoder::new(&body[..])
            .and_then(|mut d| d.read_to_end(&mut json))
            .map_err(|e| StoreError::Corrupt(format!("artifact {id} frame corrupt: {e}")))?;
        Ok(Some(serde_json::from_slice(&json)?))
    }

    fn exists(&self, id: &ArtifactId) -> bool {
        let packed = {
            let inner = self.inner.lock().unwrap();
            inner.index.contains_key(id.as_str())
        };
        packed || self.loose.exists(id)
    }

    fn list(&self) -> Result<Vec<ArtifactId>, StoreError> {
        let mut ids: Vec<ArtifactId> = {
            let inner = self.inner.lock().unwrap();
            inner.index.keys().map(|k| ArtifactId(k.clone())).collect()
        };
        for loose_id in self.loose.list()? {
            if !{
                let inner = self.inner.lock().unwrap();
                inner.index.contains_key(loose_id.as_str())
            } {
                ids.push(loose_id);
            }
        }
        Ok(ids)
    }
}

fn compress_frame_body(json: &[u8]) -> Result<Vec<u8>, StoreError> {
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), PACK_ZSTD_LEVEL)?;
    encoder.include_checksum(true)?;
    encoder.write_all(json)?;
    Ok(encoder.finish()?)
}

fn hex_id_to_raw(id: &ArtifactId) -> Result<[u8; ID_LEN], StoreError> {
    let bytes = hex::decode(id.as_str())
        .map_err(|e| StoreError::Corrupt(format!("artifact id {id} is not hex: {e}")))?;
    bytes
        .try_into()
        .map_err(|_| StoreError::Corrupt(format!("artifact id {id} is not a 32-byte hash")))
}

/// Existing segment files in numeric order, verified dense from 0.
fn segment_paths(root: &Path) -> Result<Vec<(u32, PathBuf)>, StoreError> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Some(num) = name
            .strip_prefix("pack-")
            .and_then(|n| n.strip_suffix(".seg"))
            && let Ok(segment) = num.parse::<u32>()
        {
            found.push((segment, path));
        }
    }
    found.sort_by_key(|(n, _)| *n);
    for (i, (n, path)) in found.iter().enumerate() {
        if i as u32 != *n {
            return Err(StoreError::Corrupt(format!(
                "segment numbering gap: expected pack-{i:04}.seg, found {}",
                path.display()
            )));
        }
    }
    Ok(found)
}

/// Header-only scan of one segment, filling `index`. A torn trailing frame is
/// truncated away; returns the (possibly reduced) segment length.
fn scan_segment(
    path: &Path,
    segment: u32,
    index: &mut HashMap<String, FrameLoc>,
) -> Result<u64, StoreError> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let file_len = file.metadata()?.len();
    let mut pos = 0u64;

    while pos < file_len {
        let mut torn = pos + 4 > file_len;
        let mut frame_len = 0u32;
        if !torn {
            let mut len_bytes = [0u8; 4];
            file.seek(SeekFrom::Start(pos))?;
            file.read_exact(&mut len_bytes)?;
            frame_len = u32::from_le_bytes(len_bytes);
            torn = frame_len < MIN_FRAME_LEN || pos + 4 + frame_len as u64 > file_len;
        }
        if torn {
            tracing::warn!(
                segment,
                offset = pos,
                "truncating torn frame at segment tail (crash recovery)"
            );
            file.set_len(pos)?;
            return Ok(pos);
        }

        let mut raw_id = [0u8; ID_LEN];
        file.read_exact(&mut raw_id)?;
        index.insert(
            hex::encode(raw_id),
            FrameLoc {
                segment,
                offset: pos,
                body_len: frame_len - ID_LEN as u32,
            },
        );
        pos += 4 + frame_len as u64;
    }
    Ok(pos)
}

/// Remove now-empty 2-hex prefix directories after a repack.
fn prune_empty_dirs(root: &Path) -> Result<(), StoreError> {
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() && std::fs::read_dir(&path)?.next().is_none() {
            std::fs::remove_dir(&path).ok();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn id_of(n: u8) -> ArtifactId {
        ArtifactId(hex::encode([n; 32]))
    }

    fn sample(n: u8) -> serde_json::Value {
        // Shaped like a real file observation: path, size, and a ~600-char
        // excerpt (RFC 0014) — the size distribution the pack format targets.
        let excerpt = format!(
            "//! Handler {n}: reconciles inbound events against the projection \
             store. Retries with exponential backoff; poison messages park in \
             the dead-letter queue after five attempts. {}",
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(8)
        );
        serde_json::json!({
            "artifact_type": "observation",
            "connector_name": "file",
            "target": format!("src/module/file_{n}.rs"),
            "data": {"size_bytes": 1000 + n as u64, "excerpt": excerpt},
        })
    }

    #[test]
    fn write_read_round_trip_and_cache_hit() {
        let dir = tempdir().unwrap();
        let store = PackArtifactStore::open(dir.path()).unwrap();

        assert!(store.write(&id_of(1), &sample(1)).unwrap());
        assert!(
            !store.write(&id_of(1), &sample(1)).unwrap(),
            "second write is a cache hit"
        );
        assert_eq!(store.read(&id_of(1)).unwrap().unwrap(), sample(1));
        assert!(store.exists(&id_of(1)));
        assert!(store.read(&id_of(9)).unwrap().is_none());
    }

    #[test]
    fn index_survives_reopen_without_sidecar() {
        let dir = tempdir().unwrap();
        {
            let store = PackArtifactStore::open(dir.path()).unwrap();
            for n in 0..20 {
                store.write(&id_of(n), &sample(n)).unwrap();
            }
        }
        let reopened = PackArtifactStore::open(dir.path()).unwrap();
        assert_eq!(reopened.packed_count(), 20);
        assert_eq!(reopened.read(&id_of(13)).unwrap().unwrap(), sample(13));
        assert_eq!(reopened.list().unwrap().len(), 20);
    }

    #[test]
    fn torn_tail_is_truncated_and_prior_frames_survive() {
        let dir = tempdir().unwrap();
        {
            let store = PackArtifactStore::open(dir.path()).unwrap();
            for n in 0..5 {
                store.write(&id_of(n), &sample(n)).unwrap();
            }
        }
        // Simulate a crash mid-append: chop bytes off the segment tail.
        let seg = dir.path().join("pack-0000.seg");
        let len = std::fs::metadata(&seg).unwrap().len();
        let file = OpenOptions::new().write(true).open(&seg).unwrap();
        file.set_len(len - 7).unwrap();
        drop(file);

        let store = PackArtifactStore::open(dir.path()).unwrap();
        assert_eq!(
            store.packed_count(),
            4,
            "torn last frame dropped, rest intact"
        );
        assert_eq!(store.read(&id_of(3)).unwrap().unwrap(), sample(3));
        assert!(store.read(&id_of(4)).unwrap().is_none());

        // The torn artifact can simply be written again.
        assert!(store.write(&id_of(4), &sample(4)).unwrap());
        assert_eq!(store.read(&id_of(4)).unwrap().unwrap(), sample(4));
    }

    #[test]
    fn corrupt_frame_body_is_detected_on_read() {
        let dir = tempdir().unwrap();
        {
            let store = PackArtifactStore::open(dir.path()).unwrap();
            store.write(&id_of(1), &sample(1)).unwrap();
        }
        // Flip a byte inside the compressed body (past the 4+32 header).
        let seg = dir.path().join("pack-0000.seg");
        let mut bytes = std::fs::read(&seg).unwrap();
        let corrupt_at = bytes.len() - 3;
        bytes[corrupt_at] ^= 0xFF;
        std::fs::write(&seg, &bytes).unwrap();

        let store = PackArtifactStore::open(dir.path()).unwrap();
        assert!(matches!(store.read(&id_of(1)), Err(StoreError::Corrupt(_))));
    }

    #[test]
    fn reads_fall_back_to_loose_files_and_repack_migrates_them() {
        let dir = tempdir().unwrap();
        let loose = FileSystemArtifactStore::new(dir.path());
        loose.write(&id_of(1), &sample(1)).unwrap();
        loose.write(&id_of(2), &sample(2)).unwrap();

        let store = PackArtifactStore::open(dir.path()).unwrap();
        assert_eq!(
            store.read(&id_of(1)).unwrap().unwrap(),
            sample(1),
            "loose fallback"
        );
        assert_eq!(store.list().unwrap().len(), 2);

        let (migrated, already) = store.repack_loose().unwrap();
        assert_eq!((migrated, already), (2, 0));
        assert_eq!(store.packed_count(), 2);
        assert_eq!(store.read(&id_of(2)).unwrap().unwrap(), sample(2));
        // Loose files and their prefix dirs are gone.
        assert!(loose.list().unwrap().is_empty());
    }

    #[test]
    fn packed_storage_is_smaller_than_loose() {
        let dir_loose = tempdir().unwrap();
        let dir_pack = tempdir().unwrap();
        let loose = FileSystemArtifactStore::new(dir_loose.path());
        let pack = PackArtifactStore::open(dir_pack.path()).unwrap();

        for n in 0..100 {
            loose.write(&id_of(n), &sample(n)).unwrap();
            pack.write(&id_of(n), &sample(n)).unwrap();
        }

        let loose_bytes: u64 = walk_bytes(dir_loose.path());
        let pack_bytes: u64 = walk_bytes(dir_pack.path());
        assert!(
            pack_bytes * 2 < loose_bytes,
            "expected ≥2× smaller: loose={loose_bytes} pack={pack_bytes}"
        );
    }

    fn walk_bytes(dir: &Path) -> u64 {
        let mut total = 0;
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += walk_bytes(&path);
            } else {
                total += entry.metadata().unwrap().len();
            }
        }
        total
    }
}
