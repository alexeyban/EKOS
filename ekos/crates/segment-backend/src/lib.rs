//! RFC 0113 — the storage-backend seam for **sealed** fact-segment objects.
//!
//! [`SegmentBackend`] is the interface behind which `ekos_ledger::segment::SegmentStore` reads and
//! publishes sealed (already-immutable) segment objects, so the same fact-segment format can live
//! on local disk ([`LocalFsBackend`] — the default, RFC 0111 Local mode) or in object storage
//! ([`ObjectStoreBackend`], `object-store` feature, RFC 0111 Distributed mode). Sealed objects are
//! write-once / read-many — no method here ever does a read-modify-write.
//!
//! Sealed segments go through [`SegmentBackend::publish_sealed`] / [`SegmentBackend::fetch`]
//! (RFC 0113 B1); the small mutable metadata (`manifest.json`, `dict.bin`) through
//! [`SegmentBackend::publish`] / [`SegmentBackend::get`] (RFC 0113 B4), so an `ObjectStoreBackend`
//! partition is self-describing. What stays local on the writer regardless: `HEAD` (active-segment
//! watermark), the active/unsealed segment itself, and tantivy's `search/` directory.

use std::io;
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[cfg(feature = "object-store")]
mod object_store_backend;
/// Re-exported so callers building an [`ObjectStoreBackend`] don't need their own `object_store`
/// dependency (`object-store` feature).
#[cfg(feature = "object-store")]
pub use object_store;
#[cfg(feature = "object-store")]
pub use object_store_backend::ObjectStoreBackend;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("segment backend io at {key}: {source}")]
    Io {
        key: String,
        #[source]
        source: io::Error,
    },
    #[error("segment backend at {key}: {msg}")]
    Store { key: String, msg: String },
}

impl BackendError {
    pub(crate) fn io(key: impl Into<String>, source: io::Error) -> Self {
        BackendError::Io {
            key: key.into(),
            source,
        }
    }
    #[cfg_attr(not(feature = "object-store"), allow(dead_code))]
    pub(crate) fn store(key: impl Into<String>, msg: impl Into<String>) -> Self {
        BackendError::Store {
            key: key.into(),
            msg: msg.into(),
        }
    }
}

/// Reads and publishes sealed segment objects for one partition. Selected by config, same
/// dependency-injection pattern as `Observer` / `LlmProvider` / `CompilerPass`. `key` is
/// backend-relative, mirroring the local layout 1:1 within a partition: `segments/seg-<seq>.facts`,
/// `indexes/<order>/run-*.bin`, `search/*`.
pub trait SegmentBackend: Send + Sync {
    /// Make a just-sealed object durable. Its bytes currently live at `staged` (the writer's local
    /// file, already fsynced by the caller). `LocalFsBackend`: the file *is* the durable copy —
    /// fsync it and its directory. `ObjectStoreBackend`: PUT it; the staging copy may then be
    /// dropped.
    fn publish_sealed(&self, key: &str, staged: &Path) -> Result<(), BackendError>;

    /// Publish (or **overwrite**) a small mutable metadata object — `manifest.json`, `dict.bin`
    /// (RFC 0113 B4, making a partition self-describing in object storage). Unlike
    /// [`Self::publish_sealed`], the key here is expected to be rewritten. `bytes` is the full
    /// content. Default: write `bytes` to a temp key path and treat it as a sealed publish —
    /// backends with a real overwrite (`ObjectStoreBackend` PUT, `LocalFsBackend` atomic rename)
    /// override this.
    fn publish(&self, key: &str, bytes: &[u8]) -> Result<(), BackendError> {
        // A conservative default for a backend that only knows publish_sealed: stage locally then
        // hand off. Impls that can overwrite in place do so directly.
        let tmp = std::env::temp_dir().join(format!(
            "ekos-seg-{}-{}",
            std::process::id(),
            key.replace('/', "_")
        ));
        std::fs::write(&tmp, bytes).map_err(|e| BackendError::io(key, e))?;
        let r = self.publish_sealed(key, &tmp);
        let _ = std::fs::remove_file(&tmp);
        r
    }

    /// A readable **local path** for a sealed object — fetched into a bounded local cache first if
    /// the backend is remote. `LocalFsBackend` returns the file in place. Callers `mmap` the
    /// result, so this returns a path, not bytes.
    fn fetch(&self, key: &str) -> Result<PathBuf, BackendError>;

    /// The whole object's bytes (RFC 0113 B2 — the remote query path). For `LocalFsBackend` this
    /// reads the file; prefer [`Self::fetch`] + mmap when you have a `SegmentStore`.
    fn get(&self, key: &str) -> Result<Vec<u8>, BackendError>;

    /// A byte range of the object (RFC 0113 B4 — pull individual frames without a full download).
    fn get_range(&self, key: &str, range: Range<u64>) -> Result<Vec<u8>, BackendError>;

    /// Sealed object keys present under `prefix` (e.g. `"segments/"`). Used by Service B (B4) to
    /// pull a whole partition; `SegmentStore` discovers segments from the manifest, not this.
    fn list(&self, prefix: &str) -> Result<Vec<String>, BackendError>;

    fn exists(&self, key: &str) -> Result<bool, BackendError>;

    /// Compaction / vacuum only — sealed data is otherwise never removed.
    fn delete(&self, key: &str) -> Result<(), BackendError>;
}

/// The default backend: sealed objects are plain files under `root`, exactly as the fact engine
/// has always stored them. `publish_sealed` fsyncs, `fetch` returns the path unchanged — so a
/// `SegmentStore` on a `LocalFsBackend` is byte-for-byte and behaviour-for-behaviour identical to
/// one built before this seam existed.
pub struct LocalFsBackend {
    root: PathBuf,
}

impl LocalFsBackend {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path(&self, key: &str) -> PathBuf {
        self.root.join(key)
    }
}

impl SegmentBackend for LocalFsBackend {
    fn publish(&self, key: &str, bytes: &[u8]) -> Result<(), BackendError> {
        let dest = self.path(key);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| BackendError::io(key, e))?;
        }
        let tmp = dest.with_extension("tmp");
        {
            let mut f = std::fs::File::create(&tmp).map_err(|e| BackendError::io(key, e))?;
            f.write_all(bytes).map_err(|e| BackendError::io(key, e))?;
            f.sync_all().map_err(|e| BackendError::io(key, e))?;
        }
        std::fs::rename(&tmp, &dest).map_err(|e| BackendError::io(key, e))?;
        if let Some(parent) = dest.parent()
            && let Ok(dir) = std::fs::File::open(parent)
        {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    fn publish_sealed(&self, key: &str, staged: &Path) -> Result<(), BackendError> {
        // The staged file already lives at `self.path(key)` (it was the active segment). Just make
        // the seal durable: fsync the file, then its parent directory.
        let f = std::fs::File::open(staged).map_err(|e| BackendError::io(key, e))?;
        f.sync_all().map_err(|e| BackendError::io(key, e))?;
        if let Some(parent) = staged.parent()
            && let Ok(dir) = std::fs::File::open(parent)
        {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    fn fetch(&self, key: &str) -> Result<PathBuf, BackendError> {
        Ok(self.path(key))
    }

    fn get(&self, key: &str) -> Result<Vec<u8>, BackendError> {
        std::fs::read(self.path(key)).map_err(|e| BackendError::io(key, e))
    }

    fn get_range(&self, key: &str, range: Range<u64>) -> Result<Vec<u8>, BackendError> {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(self.path(key)).map_err(|e| BackendError::io(key, e))?;
        f.seek(SeekFrom::Start(range.start))
            .map_err(|e| BackendError::io(key, e))?;
        let mut buf = vec![0u8; (range.end - range.start) as usize];
        f.read_exact(&mut buf)
            .map_err(|e| BackendError::io(key, e))?;
        Ok(buf)
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, BackendError> {
        let dir = self.path(prefix);
        let mut out = Vec::new();
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(BackendError::io(prefix, e)),
        };
        let base = if prefix.ends_with('/') || prefix.is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix}/")
        };
        for entry in entries {
            let entry = entry.map_err(|e| BackendError::io(prefix, e))?;
            if entry.path().is_file()
                && let Some(name) = entry.file_name().to_str()
            {
                out.push(format!("{base}{name}"));
            }
        }
        out.sort();
        Ok(out)
    }

    fn exists(&self, key: &str) -> Result<bool, BackendError> {
        Ok(self.path(key).exists())
    }

    fn delete(&self, key: &str) -> Result<(), BackendError> {
        match std::fs::remove_file(self.path(key)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(BackendError::io(key, e)),
        }
    }
}

/// An in-memory backend — sealed objects held in a map, `fetch` materialises them into a cache
/// dir. Not for production; it exercises the "remote backend: publish bytes, download to cache,
/// mmap the cache" flow (the same shape [`ObjectStoreBackend`] uses) with no external dependency,
/// and is the fixture the distributed-mode harness (RFC 0113 B3/B4) builds on.
pub struct MemBackend {
    objects: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    cache: PathBuf,
}

impl MemBackend {
    pub fn new(cache: impl Into<PathBuf>) -> Self {
        Self {
            objects: Default::default(),
            cache: cache.into(),
        }
    }
}

impl SegmentBackend for MemBackend {
    fn publish(&self, key: &str, bytes: &[u8]) -> Result<(), BackendError> {
        self.objects
            .lock()
            .unwrap()
            .insert(key.to_string(), bytes.to_vec());
        // Keep any materialised cache copy consistent.
        let _ = std::fs::remove_file(self.cache.join(key));
        Ok(())
    }

    fn publish_sealed(&self, key: &str, staged: &Path) -> Result<(), BackendError> {
        let bytes = std::fs::read(staged).map_err(|e| BackendError::io(key, e))?;
        self.objects.lock().unwrap().insert(key.to_string(), bytes);
        Ok(())
    }

    fn fetch(&self, key: &str) -> Result<PathBuf, BackendError> {
        let dest = self.cache.join(key);
        if dest.exists() {
            return Ok(dest);
        }
        let bytes = self.get(key)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| BackendError::io(key, e))?;
        }
        std::fs::write(&dest, &bytes).map_err(|e| BackendError::io(key, e))?;
        Ok(dest)
    }

    fn get(&self, key: &str) -> Result<Vec<u8>, BackendError> {
        self.objects
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .ok_or_else(|| BackendError::store(key, "not found"))
    }

    fn get_range(&self, key: &str, range: Range<u64>) -> Result<Vec<u8>, BackendError> {
        let bytes = self.get(key)?;
        Ok(bytes[range.start as usize..range.end as usize].to_vec())
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, BackendError> {
        let mut out: Vec<String> = self
            .objects
            .lock()
            .unwrap()
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        out.sort();
        Ok(out)
    }

    fn exists(&self, key: &str) -> Result<bool, BackendError> {
        Ok(self.objects.lock().unwrap().contains_key(key))
    }

    fn delete(&self, key: &str) -> Result<(), BackendError> {
        self.objects.lock().unwrap().remove(key);
        let _ = std::fs::remove_file(self.cache.join(key));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn write(path: &Path, bytes: &[u8]) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::File::create(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }

    /// The seam contract, run against every non-cloud backend.
    fn round_trip(b: &dyn SegmentBackend, staged_dir: &Path) {
        let key = "segments/seg-000000.facts";
        assert!(!b.exists(key).unwrap());

        let staged = staged_dir.join(key);
        write(&staged, b"the-sealed-bytes");
        b.publish_sealed(key, &staged).unwrap();

        assert!(b.exists(key).unwrap());
        assert_eq!(
            std::fs::read(b.fetch(key).unwrap()).unwrap(),
            b"the-sealed-bytes"
        );
        assert_eq!(b.get(key).unwrap(), b"the-sealed-bytes");
        assert_eq!(b.get_range(key, 4..10).unwrap(), b"sealed");
        assert_eq!(b.list("segments/").unwrap(), vec![key.to_string()]);
        assert!(b.list("nope/").unwrap().is_empty());

        b.delete(key).unwrap();
        assert!(!b.exists(key).unwrap());
        b.delete(key).unwrap(); // idempotent
    }

    #[test]
    fn local_fs_backend_satisfies_the_contract() {
        let root = tempdir().unwrap();
        round_trip(&LocalFsBackend::new(root.path()), root.path());
    }

    #[test]
    fn mem_backend_satisfies_the_contract() {
        let staged = tempdir().unwrap();
        let cache = tempdir().unwrap();
        round_trip(&MemBackend::new(cache.path()), staged.path());
    }
}
