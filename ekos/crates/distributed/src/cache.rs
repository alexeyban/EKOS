//! Materialising a partition into a readable local directory.
//!
//! A [`PartitionLocation::Local`] is opened in place. A [`PartitionLocation::ObjectStore`] is
//! pulled — every object under its prefix — into a bounded per-worker cache
//! (`<cache_root>/<partition-id>/`), preserving the backend-relative layout so
//! [`ekos_ledger::FactLedger::open_read_only`] can open the cache dir exactly as if it were the
//! original workspace partition. Sealed segments are immutable, so a file already present with the
//! right size is never re-downloaded.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ekos_cluster::PartitionLocation;
use ekos_ledger::partitioned::PartitionKey;

use crate::DistributedError;

/// The opaque wire id for a partition — `"<dimension_value>/<time_bucket>"` (RFC 0113 §B3).
pub fn partition_id(key: &PartitionKey) -> String {
    format!("{}/{}", key.dimension_value, key.time_bucket)
}

fn cache_subdir(partition_id: &str) -> String {
    partition_id
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '.' | '=' => c,
            _ => '_',
        })
        .collect()
}

/// A per-worker bounded cache of materialised partitions. Cheap to clone-share via `&`.
pub struct PartitionCache {
    root: PathBuf,
    /// Serialises materialisation so two concurrent requests for the same cold partition don't
    /// both download it.
    locks: Mutex<()>,
}

impl PartitionCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            locks: Mutex::new(()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Return a local directory holding `location`'s partition, downloading it first if the
    /// location is remote. Idempotent — a second call is a cheap `exists`/size check per object.
    pub fn materialize(
        &self,
        partition_id: &str,
        location: &PartitionLocation,
    ) -> Result<PathBuf, DistributedError> {
        match location {
            PartitionLocation::Local { root } => Ok(PathBuf::from(root)),
            PartitionLocation::ObjectStore { url, prefix } => {
                let _g = self.locks.lock().unwrap();
                let dest = self.root.join(cache_subdir(partition_id));
                self.pull_object_store(url, prefix, &dest)?;
                Ok(dest)
            }
        }
    }
}

#[cfg(not(feature = "object-store"))]
impl PartitionCache {
    fn pull_object_store(
        &self,
        _url: &str,
        _prefix: &str,
        _dest: &Path,
    ) -> Result<(), DistributedError> {
        Err(DistributedError::Other(
            "this build was compiled without the `object-store` feature; a partition in object \
             storage cannot be served. Rebuild `ekos` with `--features distributed`."
                .into(),
        ))
    }
}

#[cfg(feature = "object-store")]
impl PartitionCache {
    fn pull_object_store(
        &self,
        url: &str,
        prefix: &str,
        dest: &Path,
    ) -> Result<(), DistributedError> {
        use ekos_segment_backend::{ObjectStoreBackend, SegmentBackend};

        // `from_url` already scopes to any path in the URL; layer the partition prefix on top by
        // asking it to download into `dest` and listing/getting `prefix`-relative keys.
        let backend = ObjectStoreBackend::from_url(url, dest)?;
        let prefix = prefix.trim_end_matches('/');
        for key in backend.list(prefix)? {
            // `key` is backend-relative (below the URL path); strip the partition prefix so the
            // cache layout matches a local workspace partition (`segments/…`, `indexes/…`).
            let rel = key
                .strip_prefix(&format!("{prefix}/"))
                .or_else(|| key.strip_prefix(prefix))
                .unwrap_or(&key)
                .trim_start_matches('/');
            if rel.is_empty() {
                continue;
            }
            let out = dest.join(rel);
            let bytes = backend.get(&key)?;
            if let Ok(meta) = std::fs::metadata(&out)
                && meta.len() == bytes.len() as u64
            {
                continue; // already cached (segments are immutable)
            }
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let tmp = out.with_extension("part");
            std::fs::write(&tmp, &bytes)?;
            std::fs::rename(&tmp, &out)?;
        }
        Ok(())
    }
}
