//! RFC 0113 B1 — the storage-backend seam.
//!
//! [`SegmentBackend`] is the interface behind which [`crate::segment::SegmentStore`] reads and
//! publishes **sealed** (already-immutable) segment objects. It exists so the same fact-segment
//! format can live on local disk (Local mode — [`LocalFsBackend`], the default and today's exact
//! behaviour) or, in RFC 0111's Distributed mode, in object storage (`ObjectStoreBackend`, added
//! in B2). Sealed objects are write-once / read-many — no method here ever does a
//! read-modify-write.
//!
//! What does **not** go through this seam: the active (unsealed) segment, `HEAD`, `manifest.json`,
//! `dict.bin`, and tantivy's own `search/` directory — those stay local on the writer (see RFC
//! 0113 §B1).

use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("segment backend io at {key}: {source}")]
    Io {
        key: String,
        #[source]
        source: io::Error,
    },
    #[error("segment backend: {0}")]
    Other(String),
}

impl BackendError {
    fn io(key: impl Into<String>, source: io::Error) -> Self {
        BackendError::Io {
            key: key.into(),
            source,
        }
    }
}

/// Reads and publishes sealed segment objects for one partition root. Selected by config, same
/// dependency-injection pattern as `Observer` / `LlmProvider` / `CompilerPass`.
pub trait SegmentBackend: Send + Sync {
    /// Make a just-sealed object durable. Its bytes currently live at `staged` (the writer's local
    /// file, already fsynced by the caller). `LocalFsBackend`: the file *is* the durable copy —
    /// fsync it and its directory. A remote backend PUTs it and may then drop the staging copy.
    fn publish_sealed(&self, key: &str, staged: &Path) -> Result<(), BackendError>;

    /// A readable **local path** for a sealed object — fetched into a bounded local cache first if
    /// the backend is remote. `LocalFsBackend` returns the file in place. Callers `mmap` the
    /// result, so this returns a path, not bytes.
    fn fetch(&self, key: &str) -> Result<PathBuf, BackendError>;

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
    fn publish_sealed(&self, key: &str, staged: &Path) -> Result<(), BackendError> {
        // The staged file already lives at `self.path(key)` (it was the active segment). Just make
        // the seal durable: fsync the file, then its parent directory so the (already-existing)
        // entry survives a crash.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn local_fs_backend_round_trips() {
        let dir = tempdir().unwrap();
        let b = LocalFsBackend::new(dir.path());
        std::fs::create_dir_all(dir.path().join("segments")).unwrap();

        let key = "segments/seg-000000.facts";
        assert!(!b.exists(key).unwrap());

        let staged = dir.path().join(key);
        std::fs::File::create(&staged)
            .unwrap()
            .write_all(b"sealed")
            .unwrap();

        b.publish_sealed(key, &staged).unwrap();
        assert!(b.exists(key).unwrap());
        assert_eq!(b.fetch(key).unwrap(), staged);
        assert_eq!(std::fs::read(b.fetch(key).unwrap()).unwrap(), b"sealed");

        assert_eq!(b.list("segments/").unwrap(), vec![key.to_string()]);
        assert_eq!(b.list("segments").unwrap(), vec![key.to_string()]);
        assert!(b.list("nonexistent/").unwrap().is_empty());

        b.delete(key).unwrap();
        assert!(!b.exists(key).unwrap());
        b.delete(key).unwrap(); // idempotent
    }
}
