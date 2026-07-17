use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::ArtifactId;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("corrupt artifact store: {0}")]
    Corrupt(String),
}

/// Abstract artifact storage backend.
///
/// All code outside `ekos-artifact` must use this trait — never touch the
/// filesystem layout directly. This is what makes the v1.0 backend swap
/// a single-crate change.
pub trait ArtifactStore: Send + Sync {
    /// Write an artifact. Returns `true` if written, `false` if already present (cache hit).
    fn write(&self, id: &ArtifactId, artifact: &serde_json::Value) -> Result<bool, StoreError>;
    fn read(&self, id: &ArtifactId) -> Result<Option<serde_json::Value>, StoreError>;
    fn exists(&self, id: &ArtifactId) -> bool;
    /// Return all artifact IDs currently in the store.
    fn list(&self) -> Result<Vec<ArtifactId>, StoreError>;
}

/// Filesystem artifact store using Git object-store layout:
/// `<root>/<first-2-hex>/<full-64-hex>.json`
pub struct FileSystemArtifactStore {
    root: PathBuf,
}

impl FileSystemArtifactStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn artifact_path(&self, id: &ArtifactId) -> PathBuf {
        self.root
            .join(id.prefix())
            .join(format!("{}.json", id.as_str()))
    }
}

impl ArtifactStore for FileSystemArtifactStore {
    fn write(&self, id: &ArtifactId, artifact: &serde_json::Value) -> Result<bool, StoreError> {
        let path = self.artifact_path(id);
        if path.exists() {
            return Ok(false);
        }
        std::fs::create_dir_all(path.parent().unwrap())?;
        // RFC 0015: compact JSON — artifacts are machine-read, and the id is
        // derived from canonical content, not file bytes.
        let json = serde_json::to_string(artifact)?;
        std::fs::write(&path, json.as_bytes())?;
        Ok(true)
    }

    fn read(&self, id: &ArtifactId) -> Result<Option<serde_json::Value>, StoreError> {
        let path = self.artifact_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path)?;
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        Ok(Some(value))
    }

    fn exists(&self, id: &ArtifactId) -> bool {
        self.artifact_path(id).exists()
    }

    fn list(&self) -> Result<Vec<ArtifactId>, StoreError> {
        let mut ids = Vec::new();
        if !self.root.exists() {
            return Ok(ids);
        }
        for prefix_entry in std::fs::read_dir(&self.root)? {
            let prefix_dir = prefix_entry?.path();
            if !prefix_dir.is_dir() {
                continue;
            }
            for file_entry in std::fs::read_dir(&prefix_dir)? {
                let path = file_entry?.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json")
                    && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                {
                    ids.push(ArtifactId(stem.to_string()));
                }
            }
        }
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_store() -> (FileSystemArtifactStore, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        (FileSystemArtifactStore::new(dir.path()), dir)
    }

    #[test]
    fn write_and_read_round_trip() {
        let (store, _dir) = make_store();
        let id = ArtifactId("ab".repeat(32)); // 64 hex chars
        let value = serde_json::json!({"hello": "world"});

        let written = store.write(&id, &value).unwrap();
        assert!(written, "first write should return true");

        let read_back = store.read(&id).unwrap().unwrap();
        assert_eq!(read_back["hello"], "world");
    }

    #[test]
    fn second_write_is_cache_hit() {
        let (store, _dir) = make_store();
        let id = ArtifactId("cd".repeat(32));
        let value = serde_json::json!({"x": 1});

        assert!(store.write(&id, &value).unwrap());
        assert!(
            !store.write(&id, &value).unwrap(),
            "second write must be cache hit"
        );
    }

    #[test]
    fn read_missing_returns_none() {
        let (store, _dir) = make_store();
        let id = ArtifactId("ef".repeat(32));
        assert!(store.read(&id).unwrap().is_none());
        assert!(!store.exists(&id));
    }

    #[test]
    fn git_object_layout() {
        let (store, dir) = make_store();
        let id = ArtifactId("ab1234".repeat(10) + "abcd"); // starts with "ab"
        store.write(&id, &serde_json::json!({})).unwrap();
        let expected = dir.path().join("ab").join(format!("{}.json", id.as_str()));
        assert!(
            expected.exists(),
            "artifact must be at <prefix>/<full-id>.json"
        );
    }

    #[test]
    fn list_returns_stored_ids() {
        let (store, _dir) = make_store();
        let id1 = ArtifactId("aa".repeat(32));
        let id2 = ArtifactId("bb".repeat(32));
        store.write(&id1, &serde_json::json!({})).unwrap();
        store.write(&id2, &serde_json::json!({})).unwrap();
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 2);
    }
}
