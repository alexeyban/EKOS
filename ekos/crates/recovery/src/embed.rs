//! RFC 0125 (Phase 6 of RFC 0118) — the query/document embedding provider.
//!
//! Mirrors [`crate::llm`]: a provider-agnostic trait, real HTTP impls (Ollama, OpenAI), a
//! deterministic offline [`MockEmbeddingProvider`], and a content-addressed disk cache
//! ([`CachedEmbeddingProvider`]). The `KnowledgeStore` trait stays sync — this is the one async
//! seam, called once per query in `runtime` (and per batch in the post-`commit` embed pass), never
//! from inside the ledger.

use crate::llm::LlmError;
use async_trait::async_trait;
use ekos_common::redaction::{RedactionConfig, redact};
use ekos_kir::KirObject;
use ekos_ledger::KnowledgeStore;
use ekos_ledger::vector::VectorIndex;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// So `CachedEmbeddingProvider<Arc<dyn EmbeddingProvider>>` works — the CLI builds a trait object,
/// then wraps it in the cache.
#[async_trait]
impl EmbeddingProvider for Arc<dyn EmbeddingProvider> {
    fn model_name(&self) -> &str {
        (**self).model_name()
    }
    fn dim(&self) -> usize {
        (**self).dim()
    }
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
        (**self).embed(texts).await
    }
}

/// Provider-agnostic text → vector embedding. Every vector returned is `dim()` long and
/// L2-normalizable (callers normalize before storing — query-time cosine is then a dot product).
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn model_name(&self) -> &str;
    fn dim(&self) -> usize;
    /// Embed a batch; one vector per input, same order.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError>;
}

/// L2-normalize in place; a zero vector is left as-is (its cosine against anything is 0).
pub fn l2_normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

// ── Mock: deterministic, offline ───────────────────────────────────────────

/// Deterministic hashed-token embedding — no network, no key. Same text → identical vector;
/// texts sharing tokens land near each other. The test provider and the `provider = "mock"`
/// offline option.
pub struct MockEmbeddingProvider {
    dim: usize,
}

impl MockEmbeddingProvider {
    pub const DEFAULT_DIM: usize = 64;

    pub fn new(dim: usize) -> Self {
        Self { dim: dim.max(1) }
    }

    /// Synchronous single-text embed — the async [`EmbeddingProvider::embed`] is just a batch of
    /// these. Handy for sync call sites (the RFC 0126 eval harness embeds one query at a time).
    pub fn embed_sync(&self, text: &str) -> Vec<f32> {
        self.embed_one(text)
    }

    fn embed_one(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0.0f32; self.dim];
        for token in text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
        {
            // Hash the lowercased token, then run a tiny LCG seeded by it to scatter a unit of
            // mass across the vector — deterministic and token-additive.
            let mut h = Sha256::new();
            h.update(token.to_lowercase().as_bytes());
            let digest = h.finalize();
            let mut state = u64::from_le_bytes(digest[..8].try_into().unwrap()).max(1);
            for slot in v.iter_mut() {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                // map the top bits to [-1, 1)
                let f = ((state >> 33) as f32) / (1u64 << 31) as f32 - 1.0;
                *slot += f;
            }
        }
        l2_normalize(&mut v);
        v
    }
}

impl Default for MockEmbeddingProvider {
    fn default() -> Self {
        Self::new(Self::DEFAULT_DIM)
    }
}

#[async_trait]
impl EmbeddingProvider for MockEmbeddingProvider {
    fn model_name(&self) -> &str {
        "mock-embed"
    }
    fn dim(&self) -> usize {
        self.dim
    }
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
        Ok(texts.iter().map(|t| self.embed_one(t)).collect())
    }
}

// ── Ollama ────────────────────────────────────────────────────────────────

const OLLAMA_DEFAULT_BASE_URL: &str = "http://localhost:11434";
/// `nomic-embed-text` output dimensionality.
pub const OLLAMA_DEFAULT_DIM: usize = 768;

pub struct OllamaEmbeddingProvider {
    model: String,
    dim: usize,
    base_url: String,
    client: reqwest::Client,
}

impl OllamaEmbeddingProvider {
    pub fn new(model: impl Into<String>, dim: usize, base_url: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            dim,
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }

    /// `model` defaulting to `nomic-embed-text`, base URL from `OLLAMA_BASE_URL`.
    pub fn from_env(model: Option<String>) -> Self {
        let base_url = std::env::var("OLLAMA_BASE_URL")
            .unwrap_or_else(|_| OLLAMA_DEFAULT_BASE_URL.to_string());
        Self::new(
            model.unwrap_or_else(|| "nomic-embed-text".to_string()),
            OLLAMA_DEFAULT_DIM,
            base_url,
        )
    }
}

#[derive(serde::Serialize)]
struct OllamaEmbedReq<'a> {
    model: &'a str,
    prompt: &'a str,
}
#[derive(serde::Deserialize)]
struct OllamaEmbedResp {
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingProvider for OllamaEmbeddingProvider {
    fn model_name(&self) -> &str {
        &self.model
    }
    fn dim(&self) -> usize {
        self.dim
    }
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
        // Ollama has no batch embeddings endpoint — sequential, one request per text.
        let mut out = Vec::with_capacity(texts.len());
        for text in texts {
            let resp = self
                .client
                .post(format!("{}/api/embeddings", self.base_url))
                .json(&OllamaEmbedReq {
                    model: &self.model,
                    prompt: text,
                })
                .send()
                .await?;
            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                return Err(LlmError::Api { status, body });
            }
            let parsed: OllamaEmbedResp = resp.json().await?;
            out.push(parsed.embedding);
        }
        Ok(out)
    }
}

// ── OpenAI ────────────────────────────────────────────────────────────────

/// `text-embedding-3-small` output dimensionality.
pub const OPENAI_DEFAULT_DIM: usize = 1536;

pub struct OpenAiEmbeddingProvider {
    model: String,
    dim: usize,
    api_key: String,
    client: reqwest::Client,
}

impl OpenAiEmbeddingProvider {
    pub fn new(model: impl Into<String>, dim: usize, api_key: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            dim,
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        }
    }

    /// `model` defaulting to `text-embedding-3-small`; key read from `key_env`.
    pub fn from_env(model: Option<String>, key_env: &str) -> Result<Self, LlmError> {
        let api_key =
            std::env::var(key_env).map_err(|_| LlmError::NoApiKey(key_env.to_string()))?;
        Ok(Self::new(
            model.unwrap_or_else(|| "text-embedding-3-small".to_string()),
            OPENAI_DEFAULT_DIM,
            api_key,
        ))
    }
}

#[derive(serde::Serialize)]
struct OpenAiEmbedReq<'a> {
    model: &'a str,
    input: &'a [String],
}
#[derive(serde::Deserialize)]
struct OpenAiEmbedResp {
    data: Vec<OpenAiEmbedDatum>,
}
#[derive(serde::Deserialize)]
struct OpenAiEmbedDatum {
    embedding: Vec<f32>,
    index: usize,
}

#[async_trait]
impl EmbeddingProvider for OpenAiEmbeddingProvider {
    fn model_name(&self) -> &str {
        &self.model
    }
    fn dim(&self) -> usize {
        self.dim
    }
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let resp = self
            .client
            .post("https://api.openai.com/v1/embeddings")
            .bearer_auth(&self.api_key)
            .json(&OpenAiEmbedReq {
                model: &self.model,
                input: texts,
            })
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::Api { status, body });
        }
        let mut parsed: OpenAiEmbedResp = resp.json().await?;
        // The API documents order-preservation, but sort by `index` to be safe.
        parsed.data.sort_by_key(|d| d.index);
        Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
    }
}

// ── Cache ─────────────────────────────────────────────────────────────────

fn cache_key(model: &str, text: &str) -> String {
    let mut h = Sha256::new();
    h.update(model.as_bytes());
    h.update([0u8]);
    h.update(text.as_bytes());
    hex::encode(h.finalize())
}

/// Wraps any [`EmbeddingProvider`] with a content-addressed disk cache under
/// `<root>/<2-hex>/<64-hex>.json`, mirroring [`crate::CachedLlmProvider`]. Only cache misses hit
/// the inner provider.
pub struct CachedEmbeddingProvider<P> {
    inner: P,
    root: PathBuf,
}

impl<P: EmbeddingProvider> CachedEmbeddingProvider<P> {
    pub fn new(inner: P, root: impl Into<PathBuf>) -> Self {
        Self {
            inner,
            root: root.into(),
        }
    }

    fn read(&self, key: &str) -> Option<Vec<f32>> {
        let path = self.root.join(&key[..2]).join(format!("{key}.json"));
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn write(&self, key: &str, vec: &[f32]) {
        let dir = self.root.join(&key[..2]);
        if std::fs::create_dir_all(&dir).is_ok()
            && let Ok(json) = serde_json::to_vec(vec)
        {
            let _ = std::fs::write(dir.join(format!("{key}.json")), json);
        }
    }
}

#[async_trait]
impl<P: EmbeddingProvider> EmbeddingProvider for CachedEmbeddingProvider<P> {
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }
    fn dim(&self) -> usize {
        self.inner.dim()
    }
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
        let model = self.inner.model_name().to_string();
        let mut out: Vec<Option<Vec<f32>>> = Vec::with_capacity(texts.len());
        let mut miss_idx = Vec::new();
        let mut miss_text = Vec::new();
        for (i, text) in texts.iter().enumerate() {
            match self.read(&cache_key(&model, text)) {
                Some(v) => out.push(Some(v)),
                None => {
                    out.push(None);
                    miss_idx.push(i);
                    miss_text.push(text.clone());
                }
            }
        }
        if !miss_text.is_empty() {
            let fresh = self.inner.embed(&miss_text).await?;
            for (slot, vec) in miss_idx.iter().zip(fresh) {
                self.write(&cache_key(&model, &texts[*slot]), &vec);
                out[*slot] = Some(vec);
            }
        }
        Ok(out.into_iter().map(|o| o.unwrap()).collect())
    }
}

/// Cosine similarity of two equal-length vectors. `0.0` on a length mismatch or a zero vector.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na < f32::EPSILON || nb < f32::EPSILON {
        0.0
    } else {
        dot / (na * nb)
    }
}

// ── the post-`commit` embed pass ──────────────────────────────────────────

/// What [`embed_objects`] did — printed by `ekos commit` like the AI-description line.
#[derive(Debug, Default, Clone)]
pub struct EmbedStats {
    pub embedded: usize,
    /// Objects already in the index (by id) — left untouched.
    pub already_indexed: usize,
    pub errors: usize,
    pub dim: usize,
    pub model: String,
}

/// The text an object is embedded from — the same signal `SearchIndex` indexes, so a vector hit
/// and a BM25 hit describe the same document: `name`, kind, its `ai_overview` (RFC 0088) if it
/// has one, else a redacted content excerpt.
fn embedding_text(obj: &KirObject, redaction: &RedactionConfig) -> String {
    let mut parts = vec![obj.name.clone(), obj.kind.to_string()];
    if let Some(s) = obj
        .properties
        .get("ai_overview")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        parts.push(s.to_string());
    } else if let Some(s) = obj
        .properties
        .get("excerpt")
        .or_else(|| obj.properties.get("content"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        let excerpt: String = s.chars().take(2000).collect();
        parts.push(redact(&excerpt, redaction));
    }
    parts.join("\n")
}

const EMBED_BATCH: usize = 64;

/// Embed every object not already in the on-disk [`VectorIndex`] at `index_dir` and persist it.
/// Incremental by object **id** (a re-run embeds nothing new); a full rebuild happens on a
/// `dim`/`model` change (auto-wipe) or by deleting `vectors/`. Retracted objects are not pruned
/// here — `FactLedger::retrieve` drops a hit whose object no longer exists.
pub async fn embed_objects(
    store: &dyn KnowledgeStore,
    provider: &dyn EmbeddingProvider,
    index_dir: &Path,
    redaction: &RedactionConfig,
) -> Result<EmbedStats, LlmError> {
    let (mut index, marker) = VectorIndex::open(index_dir, provider.dim(), provider.model_name())
        .map_err(|e| LlmError::other(format!("vector index open: {e}")))?;

    let objects = store
        .all_objects()
        .map_err(|e| LlmError::other(format!("listing objects: {e}")))?;

    let mut stats = EmbedStats {
        dim: provider.dim(),
        model: provider.model_name().to_string(),
        ..Default::default()
    };

    let pending: Vec<&KirObject> = objects
        .iter()
        .filter(|o| {
            if index.contains(&o.id) {
                stats.already_indexed += 1;
                false
            } else {
                true
            }
        })
        .collect();

    for chunk in pending.chunks(EMBED_BATCH) {
        let texts: Vec<String> = chunk.iter().map(|o| embedding_text(o, redaction)).collect();
        match provider.embed(&texts).await {
            Ok(vecs) => {
                for (obj, vec) in chunk.iter().zip(vecs) {
                    match index.upsert(obj.id, vec) {
                        Ok(()) => stats.embedded += 1,
                        Err(_) => stats.errors += 1,
                    }
                }
            }
            Err(e) => {
                stats.errors += chunk.len();
                tracing::warn!("embedding batch failed: {e}");
            }
        }
    }
    if index.should_compact() {
        index
            .compact()
            .map_err(|e| LlmError::other(format!("vector compact: {e}")))?;
    }
    // The watermark is informational in this incremental-by-id design — carry it forward.
    index
        .flush(marker)
        .map_err(|e| LlmError::other(format!("vector flush: {e}")))?;

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_is_deterministic_and_token_additive() {
        let p = MockEmbeddingProvider::default();
        let a = &p.embed(&["send welcome emails".into()]).await.unwrap()[0];
        let a2 = &p.embed(&["send welcome emails".into()]).await.unwrap()[0];
        assert_eq!(a, a2, "same text → identical vector");

        let near = &p.embed(&["welcome email dispatch".into()]).await.unwrap()[0];
        let far = &p
            .embed(&["quarterly revenue projections".into()])
            .await
            .unwrap()[0];
        assert!(
            cosine(a, near) > cosine(a, far),
            "token overlap ⇒ higher cosine: near={} far={}",
            cosine(a, near),
            cosine(a, far)
        );

        let norm: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "L2-normalized, got {norm}");
    }

    #[tokio::test]
    async fn embed_objects_is_incremental_by_id() {
        use ekos_kir::{KirObject, ObjectKind};
        use ekos_ledger::Ledger;

        let dir = tempfile::tempdir().unwrap();
        let store = Ledger::open(&dir.path().join("l.db")).unwrap();
        store
            .append_object(&KirObject::new("alpha", ObjectKind::Table))
            .unwrap();
        store
            .append_object(&KirObject::new("beta", ObjectKind::Table))
            .unwrap();

        let provider = MockEmbeddingProvider::default();
        let idx_dir = dir.path().join("vectors");
        let redaction = RedactionConfig::default();

        let s1 = embed_objects(&store, &provider, &idx_dir, &redaction)
            .await
            .unwrap();
        assert_eq!(s1.embedded, 2);
        assert_eq!(s1.already_indexed, 0);
        assert!(idx_dir.join("meta.json").exists());

        // second run, no new objects → embeds nothing
        let s2 = embed_objects(&store, &provider, &idx_dir, &redaction)
            .await
            .unwrap();
        assert_eq!(s2.embedded, 0);
        assert_eq!(s2.already_indexed, 2);

        // a new object → only it is embedded
        store
            .append_object(&KirObject::new("gamma", ObjectKind::Table))
            .unwrap();
        let s3 = embed_objects(&store, &provider, &idx_dir, &redaction)
            .await
            .unwrap();
        assert_eq!(s3.embedded, 1);
        assert_eq!(s3.already_indexed, 2);
    }

    #[tokio::test]
    async fn cache_serves_hits_without_calling_inner() {
        struct CountingMock {
            inner: MockEmbeddingProvider,
            calls: std::sync::atomic::AtomicUsize,
        }
        #[async_trait]
        impl EmbeddingProvider for CountingMock {
            fn model_name(&self) -> &str {
                "counting-mock"
            }
            fn dim(&self) -> usize {
                self.inner.dim()
            }
            async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
                self.calls
                    .fetch_add(texts.len(), std::sync::atomic::Ordering::SeqCst);
                self.inner.embed(texts).await
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let counting = CountingMock {
            inner: MockEmbeddingProvider::default(),
            calls: Default::default(),
        };
        let cached = CachedEmbeddingProvider::new(counting, dir.path());

        let texts = vec!["alpha".to_string(), "beta".to_string()];
        let first = cached.embed(&texts).await.unwrap();
        let second = cached.embed(&texts).await.unwrap();
        assert_eq!(first, second);
        assert_eq!(
            cached.inner.calls.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "second call is fully cache-served"
        );
    }
}
