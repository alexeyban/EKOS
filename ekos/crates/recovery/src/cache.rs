//! Disk-backed LLM response cache (RFC 0008).
//!
//! Cache key = SHA-256(model ‖ 0x00 ‖ prompt_version ‖ 0x00 ‖ system ‖ 0x00 ‖ user)
//! Store layout: `<cache_root>/<2-hex>/<64-hex>.json` — same as artifact store.

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::llm::{LlmError, LlmProvider, LlmRequest, LlmResponse};

fn cache_key(model: &str, req: &LlmRequest<'_>) -> String {
    let mut h = Sha256::new();
    h.update(model.as_bytes());
    h.update([0u8]);
    h.update(req.prompt_version.as_bytes());
    h.update([0u8]);
    h.update(req.system.as_bytes());
    h.update([0u8]);
    h.update(req.user.as_bytes());
    hex::encode(h.finalize())
}

fn cache_path(root: &Path, key: &str) -> PathBuf {
    root.join(&key[..2]).join(format!("{key}.json"))
}

/// Wraps any `LlmProvider`, checking `.ekos/llm-cache/` before calling the inner provider.
pub struct CachedLlmProvider<T> {
    inner: T,
    cache_root: PathBuf,
}

impl<T: LlmProvider> CachedLlmProvider<T> {
    pub fn new(inner: T, cache_root: impl Into<PathBuf>) -> Self {
        Self {
            inner,
            cache_root: cache_root.into(),
        }
    }

    pub fn cache_root(&self) -> &std::path::Path {
        &self.cache_root
    }
}

#[async_trait]
impl<T: LlmProvider> LlmProvider for CachedLlmProvider<T> {
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    async fn complete(&self, req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        let key = cache_key(self.inner.model_name(), req);
        let path = cache_path(&self.cache_root, &key);

        // Cache hit.
        if path.exists() {
            let bytes = tokio::fs::read(&path).await?;
            let resp: LlmResponse = serde_json::from_slice(&bytes)?;
            tracing::debug!(key = %key[..8], "llm cache hit");
            return Ok(resp);
        }

        // Cache miss — call inner provider.
        tracing::debug!(key = %key[..8], "llm cache miss — calling api");
        let resp = self.inner.complete(req).await?;

        // Persist to cache.
        tokio::fs::create_dir_all(path.parent().unwrap()).await?;
        let json = serde_json::to_string_pretty(&resp)?;
        tokio::fs::write(&path, json.as_bytes()).await?;

        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockLlmProvider;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tempfile::TempDir;

    struct CountingMock {
        calls: Arc<AtomicU32>,
        response: String,
    }

    #[async_trait]
    impl LlmProvider for CountingMock {
        fn model_name(&self) -> &str {
            "counting-mock"
        }
        async fn complete(&self, _req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(LlmResponse {
                content: self.response.clone(),
                model: "counting-mock".into(),
                input_tokens: 0,
                output_tokens: 0,
            })
        }
    }

    #[tokio::test]
    async fn second_call_is_cache_hit() {
        let dir = TempDir::new().unwrap();
        let calls = Arc::new(AtomicU32::new(0));
        let provider = CachedLlmProvider::new(
            CountingMock {
                calls: calls.clone(),
                response: r#"{"result":"ok"}"#.into(),
            },
            dir.path(),
        );

        let req = LlmRequest {
            system: "you are helpful",
            user: "analyse this",
            prompt_version: "test-v1",
            max_tokens: 100,
        };

        let r1 = provider.complete(&req).await.unwrap();
        let r2 = provider.complete(&req).await.unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "inner provider must be called exactly once"
        );
        assert_eq!(r1.content, r2.content);
    }

    #[tokio::test]
    async fn different_prompt_versions_different_cache_entries() {
        let dir = TempDir::new().unwrap();
        let calls = Arc::new(AtomicU32::new(0));
        let provider = CachedLlmProvider::new(
            CountingMock {
                calls: calls.clone(),
                response: "resp".into(),
            },
            dir.path(),
        );

        let req_v1 = LlmRequest {
            system: "s",
            user: "u",
            prompt_version: "v1",
            max_tokens: 10,
        };
        let req_v2 = LlmRequest {
            system: "s",
            user: "u",
            prompt_version: "v2",
            max_tokens: 10,
        };

        provider.complete(&req_v1).await.unwrap();
        provider.complete(&req_v2).await.unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "different prompt versions must be separate cache entries"
        );
    }
}
