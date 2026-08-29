//! RFC 0113 B2 — [`SegmentBackend`] over the `object_store` crate (S3 / Azure ADLS Gen2 /
//! S3-compatible / local FS / in-memory, one trait for all).
//!
//! `object_store` is async; `SegmentBackend` is sync (it is called from sync `SegmentStore` /
//! compiler passes). The bridge is a dedicated current-thread `tokio` runtime per backend —
//! `block_on`-ing each call. This is safe from a `spawn_blocking` thread (RFC 0113 §B2 — Service
//! A/B run the sync passes on blocking threads) and from a plain sync test; it must not be called
//! from *within* another runtime's async context.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::StreamExt;
use object_store::path::Path as ObjPath;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload};

use crate::{BackendError, SegmentBackend};

pub struct ObjectStoreBackend {
    store: Arc<dyn ObjectStore>,
    /// Key prefix within the store (e.g. a partition id); `""` for none.
    prefix: String,
    /// Local dir sealed objects are materialised into by [`SegmentBackend::fetch`].
    cache: PathBuf,
    rt: tokio::runtime::Runtime,
}

impl ObjectStoreBackend {
    /// `store` is any `object_store` implementation; `prefix` scopes keys within it; `cache` is a
    /// local directory `fetch` downloads into.
    pub fn new(
        store: Arc<dyn ObjectStore>,
        prefix: impl Into<String>,
        cache: impl Into<PathBuf>,
    ) -> Result<Self, BackendError> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| BackendError::store("<runtime>", e.to_string()))?;
        Ok(Self {
            store,
            prefix: prefix.into(),
            cache: cache.into(),
            rt,
        })
    }

    fn obj_path(&self, key: &str) -> ObjPath {
        if self.prefix.is_empty() {
            ObjPath::from(key)
        } else {
            ObjPath::from(format!("{}/{}", self.prefix.trim_end_matches('/'), key))
        }
    }

    fn strip_prefix<'a>(&self, full: &'a str) -> &'a str {
        if self.prefix.is_empty() {
            full
        } else {
            full.strip_prefix(&format!("{}/", self.prefix.trim_end_matches('/')))
                .unwrap_or(full)
        }
    }
}

impl SegmentBackend for ObjectStoreBackend {
    fn publish_sealed(&self, key: &str, staged: &Path) -> Result<(), BackendError> {
        let bytes = std::fs::read(staged).map_err(|e| BackendError::io(key, e))?;
        let path = self.obj_path(key);
        self.rt
            .block_on(self.store.put(&path, PutPayload::from(bytes)))
            .map_err(|e| BackendError::store(key, e.to_string()))?;
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
        // temp + rename so a concurrent reader never sees a half-written cache file
        let tmp = dest.with_extension("download");
        std::fs::write(&tmp, &bytes).map_err(|e| BackendError::io(key, e))?;
        std::fs::rename(&tmp, &dest).map_err(|e| BackendError::io(key, e))?;
        Ok(dest)
    }

    fn get(&self, key: &str) -> Result<Vec<u8>, BackendError> {
        let path = self.obj_path(key);
        let bytes = self
            .rt
            .block_on(async {
                let r = self.store.get(&path).await?;
                r.bytes().await
            })
            .map_err(|e| BackendError::store(key, e.to_string()))?;
        Ok(bytes.to_vec())
    }

    fn get_range(&self, key: &str, range: Range<u64>) -> Result<Vec<u8>, BackendError> {
        let path = self.obj_path(key);
        let bytes = self
            .rt
            .block_on(self.store.get_range(&path, range))
            .map_err(|e| BackendError::store(key, e.to_string()))?;
        Ok(bytes.to_vec())
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, BackendError> {
        let path = self.obj_path(prefix.trim_end_matches('/'));
        let mut out = Vec::new();
        self.rt.block_on(async {
            let mut stream = self.store.list(Some(&path));
            while let Some(meta) = stream.next().await {
                let meta = meta.map_err(|e| BackendError::store(prefix, e.to_string()))?;
                out.push(self.strip_prefix(meta.location.as_ref()).to_string());
            }
            Ok::<_, BackendError>(())
        })?;
        out.sort();
        Ok(out)
    }

    fn exists(&self, key: &str) -> Result<bool, BackendError> {
        let path = self.obj_path(key);
        match self.rt.block_on(self.store.head(&path)) {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(BackendError::store(key, e.to_string())),
        }
    }

    fn delete(&self, key: &str) -> Result<(), BackendError> {
        let path = self.obj_path(key);
        match self.rt.block_on(self.store.delete(&path)) {
            Ok(()) => Ok(()),
            Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(e) => Err(BackendError::store(key, e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use object_store::memory::InMemory;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn object_store_backend_satisfies_the_contract_against_in_memory() {
        let cache = tempdir().unwrap();
        let staged_dir = tempdir().unwrap();
        let b = ObjectStoreBackend::new(
            Arc::new(InMemory::new()),
            "part-Table-2026-08",
            cache.path(),
        )
        .unwrap();

        let key = "segments/seg-000000.facts";
        assert!(!b.exists(key).unwrap());

        let staged = staged_dir.path().join(key);
        std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
        std::fs::File::create(&staged)
            .unwrap()
            .write_all(b"the-sealed-bytes")
            .unwrap();
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
}
