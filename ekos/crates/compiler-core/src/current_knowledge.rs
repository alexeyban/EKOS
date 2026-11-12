//! Which knowledge artifacts the latest `ekos recover` stands behind.
//!
//! The artifact store is append-only: every knowledge artifact a pass ever wrote stays in it. A
//! file deleted from the workspace registers no pass any more, but its last knowledge artifact is
//! still there, and `compile` used to read it — a removed Kettle job or routine kept explaining
//! writes and a deleted table kept its page. A multi-input pass (Pentaho, the dependency scan)
//! writes one artifact per run whose inputs differ from the previous run's, so even the newest-per-
//! input dedup kept its earlier runs.
//!
//! `recover` now records, per pass, the knowledge artifacts that pass wrote (in the pass's cache
//! manifest, so a cached pass still names its output) and writes the union for the run:
//! `<artifact dir>/current-knowledge.json`. `compile` and `resolve` read only those. A pass whose
//! outputs are not known (its manifest predates this) is listed in `unknown_passes`, and its
//! artifacts are kept by the old rule until it next runs.

use ekos_artifact::{ArtifactId, ArtifactStore, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The file name, in the artifact directory.
pub const FILE: &str = "current-knowledge.json";

/// What the latest recover ran and produced.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentKnowledge {
    /// Every pass registered in the run, whether it ran or was served from cache.
    pub passes: BTreeSet<String>,
    /// The knowledge artifacts those passes stand behind.
    pub ids: BTreeSet<String>,
    /// Passes whose outputs are not known (a cache manifest written before outputs were recorded,
    /// or a pass that failed): their artifacts are kept by pass name.
    pub unknown_passes: BTreeSet<String>,
}

impl CurrentKnowledge {
    /// Whether a knowledge artifact (`id`, written by `pass`) is part of the current knowledge.
    pub fn admits(&self, id: &str, pass: &str) -> bool {
        self.ids.contains(id) || (self.unknown_passes.contains(pass) && self.passes.contains(pass))
    }

    pub fn path(artifact_dir: &Path) -> PathBuf {
        artifact_dir.join(FILE)
    }

    /// The latest recover's record, or `None` before the first recover that writes one (every
    /// artifact is then read, as before).
    pub fn read(artifact_dir: &Path) -> Option<Self> {
        let raw = std::fs::read(Self::path(artifact_dir)).ok()?;
        serde_json::from_slice(&raw).ok()
    }

    pub fn write(&self, artifact_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(artifact_dir)?;
        let path = Self::path(artifact_dir);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)
    }
}

/// An [`ArtifactStore`] that remembers the knowledge artifacts written through it — one pass's
/// output. A write of an id already present (identical content) still counts: it is this run's
/// output too.
pub struct RecordingStore {
    inner: Arc<dyn ArtifactStore>,
    written: Mutex<BTreeSet<String>>,
}

impl RecordingStore {
    pub fn new(inner: Arc<dyn ArtifactStore>) -> Self {
        Self {
            inner,
            written: Mutex::new(BTreeSet::new()),
        }
    }

    pub fn knowledge_written(&self) -> BTreeSet<String> {
        self.written.lock().map(|w| w.clone()).unwrap_or_default()
    }
}

impl ArtifactStore for RecordingStore {
    fn write(&self, id: &ArtifactId, artifact: &serde_json::Value) -> Result<bool, StoreError> {
        let written = self.inner.write(id, artifact)?;
        if artifact["artifact_type"].as_str() == Some("knowledge")
            && let Ok(mut w) = self.written.lock()
        {
            w.insert(id.to_string());
        }
        Ok(written)
    }
    fn read(&self, id: &ArtifactId) -> Result<Option<serde_json::Value>, StoreError> {
        self.inner.read(id)
    }
    fn exists(&self, id: &ArtifactId) -> bool {
        self.inner.exists(id)
    }
    fn list(&self) -> Result<Vec<ArtifactId>, StoreError> {
        self.inner.list()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_current_ids_are_admitted_and_unknown_passes_by_name() {
        let c = CurrentKnowledge {
            passes: ["sql:a.sql".to_string(), "pentaho".to_string()].into(),
            ids: ["k1".to_string()].into(),
            unknown_passes: ["pentaho".to_string()].into(),
        };
        assert!(c.admits("k1", "sql:a.sql"));
        assert!(
            !c.admits("k0", "sql:a.sql"),
            "an older output of a current pass"
        );
        assert!(
            !c.admits("k9", "sql:deleted.sql"),
            "a pass no longer registered"
        );
        assert!(
            c.admits("k7", "pentaho"),
            "a pass whose outputs are not known yet"
        );
    }

    #[test]
    fn the_recording_store_names_knowledge_writes_only() {
        let dir = tempfile::tempdir().unwrap();
        let inner: Arc<dyn ArtifactStore> =
            Arc::new(ekos_artifact::FileSystemArtifactStore::new(dir.path()));
        let r = RecordingStore::new(inner);
        let k = serde_json::json!({"artifact_type": "knowledge", "x": 1});
        let o = serde_json::json!({"artifact_type": "observation"});
        r.write(&ArtifactId("a".repeat(64)), &k).unwrap();
        r.write(&ArtifactId("a".repeat(64)), &k).unwrap();
        r.write(&ArtifactId("b".repeat(64)), &o).unwrap();
        assert_eq!(r.knowledge_written(), ["a".repeat(64)].into());
        let c = CurrentKnowledge {
            ids: r.knowledge_written(),
            ..Default::default()
        };
        c.write(dir.path()).unwrap();
        assert_eq!(CurrentKnowledge::read(dir.path()), Some(c));
    }
}
