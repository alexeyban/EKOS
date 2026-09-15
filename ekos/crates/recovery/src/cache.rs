//! Disk-backed LLM response cache (RFC 0008).
//!
//! Cache key = SHA-256(model ‖ 0x00 ‖ prompt_version ‖ 0x00 ‖ system ‖ 0x00 ‖ user
//! ‖ 0x00 ‖ history[0].role ‖ 0x00 ‖ history[0].content ‖ 0x00 ‖ history[1].role ‖ …)
//! Store layout: `<cache_root>/<2-hex>/<64-hex>.json` — same as artifact store.

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::llm::{LlmError, LlmProvider, LlmRequest, LlmResponse};

/// RFC 0099: `history` is folded into the key so two requests that differ
/// only in prior conversation turns never collide on the same cache entry.
/// Every single-shot caller (analyzer passes, `docs --prose`, `marketing`,
/// the NL-to-SQL bridge, …) passes an empty `history`, so this is a no-op
/// extension of the existing hash for every pre-RFC-0099 call site — same
/// key as before, byte for byte, when `history` is empty.
///
/// RFC 0145: `namespace` (see `LlmProvider::cache_namespace`) is appended last, and only when
/// `Some` — the same no-op-when-absent extension as `history`.
fn cache_key(model: &str, namespace: Option<&str>, req: &LlmRequest<'_>) -> String {
    let mut h = Sha256::new();
    h.update(model.as_bytes());
    h.update([0u8]);
    h.update(req.prompt_version.as_bytes());
    h.update([0u8]);
    h.update(req.system.as_bytes());
    h.update([0u8]);
    h.update(req.user.as_bytes());
    for turn in req.history {
        h.update([0u8]);
        h.update(turn.role.as_bytes());
        h.update([0u8]);
        h.update(turn.content.as_bytes());
    }
    if let Some(ns) = namespace {
        h.update([0u8]);
        h.update(b"ns:");
        h.update(ns.as_bytes());
    }
    hex::encode(h.finalize())
}

/// `max_tokens` is deliberately **not** part of the cache key (that would invalidate every
/// existing entry, re-spending real money on analyzer passes). Instead each entry records the
/// limit it was generated under, and [`truncated_below`] refuses to replay an entry that hit that
/// limit when the caller now allows more. Found live 2026-09-15: raising `[ai] max-tokens` from
/// 1024 to 2048 for a more verbose cloud model kept replaying the same answer cut off mid-word,
/// missing its `cited_evidence` block.
const MAX_TOKENS_FIELD: &str = "request_max_tokens";

/// `true` when `entry` was cut off at its own generation limit and `wanted` is larger — replaying
/// it would return an answer truncated shorter than the caller asked for. Entries written before
/// the limit was recorded have no field and always replay (unchanged behavior).
fn truncated_below(entry: &serde_json::Value, resp: &LlmResponse, wanted: u32) -> bool {
    entry
        .get(MAX_TOKENS_FIELD)
        .and_then(|v| v.as_u64())
        .is_some_and(|limit| resp.output_tokens as u64 >= limit && (wanted as u64) > limit)
}

fn cache_path(root: &Path, key: &str) -> PathBuf {
    root.join(&key[..2]).join(format!("{key}.json"))
}

/// Wraps any `LlmProvider`, checking `.ekos/llm-cache/` before calling the inner provider.
pub struct CachedLlmProvider<T> {
    inner: T,
    cache_root: PathBuf,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl<T: LlmProvider> CachedLlmProvider<T> {
    pub fn new(inner: T, cache_root: impl Into<PathBuf>) -> Self {
        Self {
            inner,
            cache_root: cache_root.into(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
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
        let namespace = self.inner.cache_namespace();
        let key = cache_key(self.inner.model_name(), namespace.as_deref(), req);
        let path = cache_path(&self.cache_root, &key);

        // Cache hit.
        if path.exists() {
            let bytes = tokio::fs::read(&path).await?;
            let entry: serde_json::Value = serde_json::from_slice(&bytes)?;
            let resp: LlmResponse = serde_json::from_value(entry.clone())?;
            if truncated_below(&entry, &resp, req.max_tokens) {
                tracing::debug!(key = %key[..8], "llm cache entry truncated below the new max_tokens — refreshing");
            } else {
                tracing::debug!(key = %key[..8], "llm cache hit");
                self.hits.fetch_add(1, Ordering::Relaxed);
                return Ok(resp);
            }
        }

        // Cache miss — call inner provider.
        tracing::debug!(key = %key[..8], "llm cache miss — calling api");
        let resp = self.inner.complete(req).await?;
        self.misses.fetch_add(1, Ordering::Relaxed);

        // Persist to cache.
        tokio::fs::create_dir_all(path.parent().unwrap()).await?;
        let mut entry = serde_json::to_value(&resp)?;
        entry[MAX_TOKENS_FIELD] = serde_json::json!(req.max_tokens);
        let json = serde_json::to_string_pretty(&entry)?;
        tokio::fs::write(&path, json.as_bytes()).await?;

        Ok(resp)
    }

    fn cache_namespace(&self) -> Option<String> {
        self.inner.cache_namespace()
    }

    fn cache_stats(&self) -> Option<(u64, u64)> {
        Some((
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
        ))
    }

    /// Deliberately **not** cached (RFC 0098): the disk cache needs one
    /// complete `LlmResponse` to hash and persist, which a stream doesn't
    /// have until it's already finished — and per-turn context for a
    /// streamed call is typically unique anyway, so the cache-hit rate
    /// would be near zero regardless. Delegates straight to the inner
    /// provider.
    async fn complete_stream(
        &self,
        req: &LlmRequest<'_>,
        on_chunk: &mut (dyn FnMut(String) + Send),
    ) -> Result<LlmResponse, LlmError> {
        self.inner.complete_stream(req, on_chunk).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// RFC 0145: a `None` namespace must keep every pre-existing key byte-identical; `Some` must
    /// separate entries.
    #[test]
    fn namespace_only_changes_the_key_when_present() {
        let req = LlmRequest {
            system: "s",
            user: "u",
            prompt_version: "v1",
            max_tokens: 10,
            history: &[],
        };
        let mut h = Sha256::new();
        for part in ["m", "v1", "s"] {
            h.update(part.as_bytes());
            h.update([0u8]);
        }
        h.update(b"u");
        let legacy = hex::encode(h.finalize());
        assert_eq!(cache_key("m", None, &req), legacy);
        let a = cache_key("m", Some("num_ctx=4096"), &req);
        let b = cache_key("m", Some("num_ctx=8192"), &req);
        assert_ne!(a, legacy);
        assert_ne!(a, b);
    }

    #[test]
    fn truncated_entries_only_refresh_when_the_limit_grows() {
        let resp = |out| LlmResponse {
            content: String::new(),
            model: "m".into(),
            input_tokens: 0,
            output_tokens: out,
        };
        let entry = |limit: Option<u32>| match limit {
            Some(l) => serde_json::json!({ MAX_TOKENS_FIELD: l }),
            None => serde_json::json!({}),
        };
        assert!(truncated_below(&entry(Some(1024)), &resp(1024), 2048));
        assert!(!truncated_below(&entry(Some(1024)), &resp(1024), 1024));
        assert!(
            !truncated_below(&entry(Some(1024)), &resp(300), 2048),
            "complete answer"
        );
        assert!(
            !truncated_below(&entry(None), &resp(1024), 2048),
            "legacy entry replays"
        );
    }

    #[tokio::test]
    async fn a_response_truncated_at_a_lower_limit_is_regenerated_not_replayed() {
        struct Capped(Arc<AtomicU32>);
        #[async_trait]
        impl LlmProvider for Capped {
            fn model_name(&self) -> &str {
                "capped"
            }
            async fn complete(&self, req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(LlmResponse {
                    content: "x".repeat(req.max_tokens as usize),
                    model: "capped".into(),
                    input_tokens: 1,
                    output_tokens: req.max_tokens,
                })
            }
        }
        let dir = TempDir::new().unwrap();
        let calls = Arc::new(AtomicU32::new(0));
        let provider = CachedLlmProvider::new(Capped(calls.clone()), dir.path());
        let req = |max_tokens| LlmRequest {
            system: "s",
            user: "u",
            prompt_version: "v1",
            max_tokens,
            history: &[],
        };
        provider.complete(&req(10)).await.unwrap();
        provider.complete(&req(10)).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "same limit replays");
        let longer = provider.complete(&req(20)).await.unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "a larger limit regenerates"
        );
        assert_eq!(longer.output_tokens, 20);
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
            history: &[],
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
            history: &[],
        };
        let req_v2 = LlmRequest {
            system: "s",
            user: "u",
            prompt_version: "v2",
            max_tokens: 10,
            history: &[],
        };

        provider.complete(&req_v1).await.unwrap();
        provider.complete(&req_v2).await.unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "different prompt versions must be separate cache entries"
        );
    }

    // ── RFC 0099: history folded into the cache key ─────────────────────

    #[tokio::test]
    async fn different_history_different_cache_entries() {
        use crate::llm::Message;
        let dir = TempDir::new().unwrap();
        let calls = Arc::new(AtomicU32::new(0));
        let provider = CachedLlmProvider::new(
            CountingMock {
                calls: calls.clone(),
                response: "resp".into(),
            },
            dir.path(),
        );

        let no_history = LlmRequest {
            system: "s",
            user: "u",
            prompt_version: "v1",
            max_tokens: 10,
            history: &[],
        };
        let with_history = LlmRequest {
            system: "s",
            user: "u",
            prompt_version: "v1",
            max_tokens: 10,
            history: &[Message {
                role: "user",
                content: "an earlier turn",
            }],
        };

        provider.complete(&no_history).await.unwrap();
        provider.complete(&with_history).await.unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "identical system/user/prompt_version but different history must not collide"
        );
    }

    #[tokio::test]
    async fn same_request_and_history_is_still_a_cache_hit() {
        use crate::llm::Message;
        let dir = TempDir::new().unwrap();
        let calls = Arc::new(AtomicU32::new(0));
        let provider = CachedLlmProvider::new(
            CountingMock {
                calls: calls.clone(),
                response: "resp".into(),
            },
            dir.path(),
        );
        let history = [Message {
            role: "user",
            content: "an earlier turn",
        }];
        let req = LlmRequest {
            system: "s",
            user: "u",
            prompt_version: "v1",
            max_tokens: 10,
            history: &history,
        };

        provider.complete(&req).await.unwrap();
        provider.complete(&req).await.unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "identical history on both calls must still hit the cache"
        );
    }
}
