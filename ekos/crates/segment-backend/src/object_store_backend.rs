//! RFC 0113 B2 — [`SegmentBackend`] over the `object_store` crate (S3 / Azure ADLS Gen2 /
//! S3-compatible / local FS / in-memory, one trait for all).
//!
//! `object_store` is async; `SegmentBackend` is sync (it is called from sync `SegmentStore` /
//! compiler passes). The bridge is a **dedicated OS thread that owns a current-thread `tokio`
//! runtime** ([`DedicatedRt`]); every call `spawn`s its future onto that runtime and blocks the
//! caller on a `std::sync::mpsc` reply. Because the runtime lives (and is dropped) entirely on
//! that private thread, an `ObjectStoreBackend` is safe to **construct, call, and drop from any
//! context** — a plain sync test, a `spawn_blocking` pipeline thread, *and* directly inside the
//! `#[tokio::main]` CLI's async context (RFC 0113 — `ekos build`/`commit`/`open_store` reach the
//! backend without a `spawn_blocking` hop). The earlier design owned a `tokio::runtime::Runtime`
//! inline and panicked (`Cannot drop a runtime in a context where blocking is not allowed`) when
//! `open_store` built one from `#[tokio::main]`.

use std::future::Future;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::StreamExt;
use object_store::path::Path as ObjPath;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload};

use crate::{BackendError, SegmentBackend};

/// A `tokio` current-thread runtime confined to one private OS thread. `block_on` dispatches a
/// future to it and waits on a std channel, so it works identically whether the caller is sync or
/// already inside another runtime. Dropping this signals the thread and joins it — the `Runtime`
/// itself is only ever dropped on its own thread, where blocking is allowed.
struct DedicatedRt {
    handle: tokio::runtime::Handle,
    stop: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl DedicatedRt {
    fn new() -> Result<Self, BackendError> {
        let (handle_tx, handle_rx) = std::sync::mpsc::channel();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("ekos-objstore-rt".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        // Report the failure back; `new` turns the closed channel into an error.
                        drop(handle_tx);
                        tracing::error!(%e, "ekos-objstore-rt: runtime build failed");
                        return;
                    }
                };
                if handle_tx.send(rt.handle().clone()).is_err() {
                    return; // constructor gave up already
                }
                // Park the runtime alive until asked to stop; `block_on` keeps driving spawned
                // tasks while a blocking-pool thread waits on the stop signal.
                rt.block_on(async move {
                    let _ = tokio::task::spawn_blocking(move || {
                        let _ = stop_rx.recv();
                    })
                    .await;
                });
            })
            .map_err(|e| BackendError::store("<objstore-rt>", e.to_string()))?;
        let handle = handle_rx
            .recv()
            .map_err(|_| BackendError::store("<objstore-rt>", "runtime thread failed to start"))?;
        Ok(Self {
            handle,
            stop: Some(stop_tx),
            thread: Some(thread),
        })
    }

    /// Run `fut` on the dedicated runtime, blocking the caller until it resolves.
    ///
    /// `fut` runs on this struct's private runtime thread, wholly independent of whatever the
    /// caller is (a plain sync test, a `spawn_blocking` pipeline thread, a current-thread runtime,
    /// or the multi-threaded `#[tokio::main]` CLI), so a plain channel wait can never deadlock.
    /// We deliberately do **not** reach for `block_in_place` — it panics on a current-thread
    /// runtime, which is exactly what `ekos compile-worker` drives its pipeline on.
    fn block_on<F>(&self, fut: F) -> F::Output
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.handle.spawn(async move {
            let _ = tx.send(fut.await);
        });
        rx.recv()
            .expect("ekos-objstore-rt stopped while a call was in flight")
    }
}

impl Drop for DedicatedRt {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub struct ObjectStoreBackend {
    store: Arc<dyn ObjectStore>,
    /// Key prefix within the store (e.g. a partition id); `""` for none.
    prefix: String,
    /// Local dir sealed objects are materialised into by [`SegmentBackend::fetch`].
    cache: PathBuf,
    rt: DedicatedRt,
}

impl ObjectStoreBackend {
    /// `store` is any `object_store` implementation; `prefix` scopes keys within it; `cache` is a
    /// local directory `fetch` downloads into.
    pub fn new(
        store: Arc<dyn ObjectStore>,
        prefix: impl Into<String>,
        cache: impl Into<PathBuf>,
    ) -> Result<Self, BackendError> {
        Ok(Self {
            store,
            prefix: prefix.into(),
            cache: cache.into(),
            rt: DedicatedRt::new()?,
        })
    }

    /// Build a backend from a URL — `memory://`, `file:///abs/path`, `s3://bucket/prefix`,
    /// `az://container/prefix`, `gs://bucket/prefix` (whatever `object_store::parse_url` supports).
    /// Credentials come from the standard provider env vars / instance metadata, never a URL.
    /// `cache` is the local directory [`SegmentBackend::fetch`] downloads into.
    pub fn from_url(url: &str, cache: impl Into<PathBuf>) -> Result<Self, BackendError> {
        let (store, prefix) = Self::parse_url(url)?;
        Self::new(Arc::from(store), prefix, cache)
    }

    /// Parse + validate a backend URL without constructing a runtime — used both by [`Self::new`]
    /// and by callers that only need to check a configured URL is well-formed (`open_store`).
    ///
    /// `object_store::parse_url` on its own reads **no** configuration, so an `s3://` URL against
    /// MinIO or any non-AWS endpoint would never authenticate. We therefore forward every
    /// `AWS_* / AZURE_* / GOOGLE_* / OBJECT_STORE_*` process variable (lowercased) as a builder
    /// option — `object_store` silently ignores keys a given scheme doesn't recognise, so this is
    /// safe for every backend. Standard names work: `AWS_ENDPOINT` (or `AWS_ENDPOINT_URL`),
    /// `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION`, `AWS_ALLOW_HTTP=true`.
    pub fn parse_url(url: &str) -> Result<(Box<dyn ObjectStore>, String), BackendError> {
        let parsed = url::Url::parse(url).map_err(|e| BackendError::store(url, e.to_string()))?;
        let opts: Vec<(String, String)> = std::env::vars()
            .filter(|(k, _)| {
                let k = k.as_str();
                k.starts_with("AWS_")
                    || k.starts_with("AZURE_")
                    || k.starts_with("GOOGLE_")
                    || k.starts_with("OBJECT_STORE_")
            })
            .map(|(k, v)| (k.to_ascii_lowercase(), v))
            .collect();
        let (store, path) = object_store::parse_url_opts(&parsed, opts)
            .map_err(|e| BackendError::store(url, e.to_string()))?;
        Ok((store, path.as_ref().to_string()))
    }

    fn obj_path(&self, key: &str) -> ObjPath {
        if self.prefix.is_empty() {
            ObjPath::from(key)
        } else {
            ObjPath::from(format!("{}/{}", self.prefix.trim_end_matches('/'), key))
        }
    }
}

impl SegmentBackend for ObjectStoreBackend {
    fn publish(&self, key: &str, bytes: &[u8]) -> Result<(), BackendError> {
        let path = self.obj_path(key);
        let store = self.store.clone();
        let payload = PutPayload::from(bytes.to_vec());
        self.rt
            .block_on(async move { store.put(&path, payload).await })
            .map_err(|e| BackendError::store(key, e.to_string()))?;
        // Drop any stale cached copy so a later `fetch` re-downloads the new content.
        let _ = std::fs::remove_file(self.cache.join(key));
        Ok(())
    }

    fn publish_sealed(&self, key: &str, staged: &Path) -> Result<(), BackendError> {
        let bytes = std::fs::read(staged).map_err(|e| BackendError::io(key, e))?;
        let path = self.obj_path(key);
        let store = self.store.clone();
        let payload = PutPayload::from(bytes);
        self.rt
            .block_on(async move { store.put(&path, payload).await })
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
        let store = self.store.clone();
        let bytes = self
            .rt
            .block_on(async move {
                let r = store.get(&path).await?;
                r.bytes().await
            })
            .map_err(|e| BackendError::store(key, e.to_string()))?;
        Ok(bytes.to_vec())
    }

    fn get_range(&self, key: &str, range: Range<u64>) -> Result<Vec<u8>, BackendError> {
        let path = self.obj_path(key);
        let store = self.store.clone();
        let bytes = self
            .rt
            .block_on(async move { store.get_range(&path, range).await })
            .map_err(|e| BackendError::store(key, e.to_string()))?;
        Ok(bytes.to_vec())
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, BackendError> {
        let path = self.obj_path(prefix.trim_end_matches('/'));
        let store = self.store.clone();
        let key_prefix = prefix.to_string();
        let self_prefix = self.prefix.clone();
        let mut out = self.rt.block_on(async move {
            let mut stream = store.list(Some(&path));
            let mut out = Vec::new();
            while let Some(meta) = stream.next().await {
                let meta = meta.map_err(|e| BackendError::store(&key_prefix, e.to_string()))?;
                let full = meta.location.as_ref();
                let stripped = if self_prefix.is_empty() {
                    full
                } else {
                    full.strip_prefix(&format!("{}/", self_prefix.trim_end_matches('/')))
                        .unwrap_or(full)
                };
                out.push(stripped.to_string());
            }
            Ok::<_, BackendError>(out)
        })?;
        out.sort();
        Ok(out)
    }

    fn exists(&self, key: &str) -> Result<bool, BackendError> {
        let path = self.obj_path(key);
        let store = self.store.clone();
        let key_owned = key.to_string();
        self.rt.block_on(async move {
            match store.head(&path).await {
                Ok(_) => Ok(true),
                Err(object_store::Error::NotFound { .. }) => Ok(false),
                Err(e) => Err(BackendError::store(&key_owned, e.to_string())),
            }
        })
    }

    fn delete(&self, key: &str) -> Result<(), BackendError> {
        let path = self.obj_path(key);
        let store = self.store.clone();
        let key_owned = key.to_string();
        self.rt.block_on(async move {
            match store.delete(&path).await {
                Ok(()) => Ok(()),
                Err(object_store::Error::NotFound { .. }) => Ok(()),
                Err(e) => Err(BackendError::store(&key_owned, e.to_string())),
            }
        })
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

    /// The regression this file's rewrite fixes: an `ObjectStoreBackend` must be safe to build,
    /// use, and drop from *within* a `#[tokio::main]`-style async context — that is exactly how
    /// `open_store` reaches it from the CLI.
    #[tokio::test(flavor = "multi_thread")]
    async fn usable_from_within_an_async_runtime() {
        let cache = tempdir().unwrap();
        let b = ObjectStoreBackend::new(Arc::new(InMemory::new()), "p", cache.path()).unwrap();
        b.publish("m/manifest.json", b"{}").unwrap();
        assert_eq!(b.get("m/manifest.json").unwrap(), b"{}");
        drop(b); // must not panic
    }
}
