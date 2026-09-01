# RFC 0125 — Phase 6 of RFC 0118: the SEARCH vector arm

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-09-01
**Implemented:** 2026-09-01 (`devlog_147`)
**Phase 6 of:** RFC 0118 · **builds on:** RFC 0119 (retrieval seam) + 0120 (RRF fusion) + 0016 (fact engine / `SearchIndex`) + 0088 (post-`commit` LLM pass pattern)
**Supersedes (scope):** RFC 0100's deferred "full embedding-based semantic search"

---

## Motivation

Phases 0–5 are BM25 + `ExactName` + fact/graph + a rules planner — all lexical. A question
phrased with none of the words in the target object ("the thing that sends welcome emails" → a
function called `dispatch_signup_notification`) has no lexical hook. RFC 0100 deferred a real
vector arm pending usage signal; RFC 0118 kept it gated as Phase 6. This RFC builds it.

The design constraints are non-negotiable and set by the existing architecture:

- **The `KnowledgeStore` trait stays sync.** The one async call — embedding the *query* — happens
  once, in `runtime`, above the trait; the result rides down as a pre-computed `Vec<f32>` on
  `RetrievalRequest.query_embedding` (the field RFC 0119 already reserved).
- **The index is derived + rebuildable**, with its **own `last_tx` watermark** — the embed pass
  is opt-in and lags BM25 by many commits. `arms_run` reports the downgrade when it's absent.
- **Offline still works with zero keys.** No provider configured → `query_embedding` is `None` →
  `retrieve` runs exactly today's BM25 + `ExactName` path. Every test that needs the arm uses
  `MockEmbeddingProvider`.
- **SQLite `Ledger` never gets a vector arm** — documented degradation path, same as its
  rank-only `retrieve`.

---

## Design

### 1. `EmbeddingProvider` — `crates/recovery/src/embed.rs`

```rust
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn model_name(&self) -> &str;
    fn dim(&self) -> usize;
    /// Embed a batch. Returns one L2-normalizable vector per input, same order, each `dim()` long.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError>;
}
```

Reuses `LlmError` (already carries `NoApiKey` / `Http` / `Api` / `Io` / `Json`). Four impls:

| impl | notes |
|---|---|
| `MockEmbeddingProvider { dim }` | Deterministic: hash each token → seed a tiny LCG → accumulate into a `dim`-length bucket vector, L2-normalize. Same text → same vector; texts sharing tokens are near. Zero deps, offline — the test + `--offline` provider. Default `dim` 64. |
| `OllamaEmbeddingProvider` | `POST {host}/api/embeddings {model, prompt}` per text (Ollama has no batch endpoint — sequential, bounded concurrency 4). Default model `nomic-embed-text` (`dim` 768). Host from `[llm] ollama-host` or `http://localhost:11434`. |
| `OpenAiEmbeddingProvider` | `POST https://api.openai.com/v1/embeddings {model, input: [texts]}` — real batch. Default `text-embedding-3-small` (`dim` 1536). Key from `[embeddings] api-key-env` (default `OPENAI_API_KEY`). |
| `CachedEmbeddingProvider<P>` | Wraps any provider + a content-addressed file cache under `.ekos/embed-cache/` (`sha256(model + "\0" + text).json` → the vector), mirroring `recovery`'s LLM disk cache. `embed` splits hits/misses, calls the inner provider only for misses. |

`build_embedding_provider(config, artifact_dir) -> Option<Arc<dyn EmbeddingProvider>>` — mirrors
`build_llm_provider`; `None` when `[embeddings]` is absent/disabled. Always wraps in
`CachedEmbeddingProvider` unless `cache = false`.

### 2. `VectorIndex` — `crates/ledger/src/vector.rs`

Sibling of `SearchIndex`, at `<ledger-dir>/vectors/`. Files (RFC 0118 §8.6, verbatim):

| file | contents |
|---|---|
| `meta.json` | `{ format_version: 1, dim, model, metric: "cosine", count, normalized: true }` |
| `ids.bin` | `count × 16B` — each row's `KirId` (`Uuid::as_bytes`) |
| `vectors.f32` | `count × dim × f32` LE, **L2-normalized at write** → query cosine = plain dot product |
| `tombstones.bin` | `count × 1B` — `1` = retracted/superseded row, skipped at query |
| `last_tx` | the embed pass's own watermark (a bare `TxId` file, like `SearchIndex`'s marker) |

- **Open:** mmap `vectors.f32` + read `ids.bin` / `tombstones.bin` into `Vec`s. `meta.dim` or
  `meta.model` mismatch against the configured provider → wipe + return empty (RFC 0103
  stale-schema pattern; the pass rebuilds from `last_tx = 0`).
- **`upsert(id, vec)`:** if `id` already present → tombstone the old row, append the new (append
  never rewrites the mmap'd body — growth + remap). L2-normalize on the way in.
- **`remove(id)`:** tombstone.
- **`query(&[f32], k) -> Vec<(KirId, f32)>`:** reject on dim mismatch (caller checks first);
  brute-force dot product over non-tombstoned rows, partial-sort top-k. ~20–40 ms at 100k × 768.
- **`compact()`:** drop tombstoned rows, rewrite all three data files. Triggered by the pass when
  `tombstoned / count > 0.3`.
- Only new mechanism: `f32::from_le_bytes` over mmap slices — no `bytemuck` dep.

### 3. `[embeddings]` config + the post-`commit` embed pass

```toml
[embeddings]
enabled = true
provider = "ollama"        # or "openai" | "mock"; falls back to [llm] provider if unset
model = "nomic-embed-text"  # optional override
api-key-env = "OPENAI_API_KEY"
cache = true
```

`EmbeddingsConfig` on `EkosConfig` (`#[serde(default)]`, `deny_unknown_fields`-compatible),
default `enabled: false` — exactly `LlmDescriptionConfig`'s shape.

**`recovery::embed_objects(ledger: &dyn KnowledgeStore, provider, index_dir, redaction) -> EmbedStats`:**
runs after `commit_rollups` / `commit_data_lineage` / `run_llm_description` in `commit.rs`, gated
on `config.embeddings.enabled`. For every object whose `updated tx > index.last_tx`:

- build its embedding text: `name` + kind + the object's `ai_overview` (RFC 0088) if present,
  else a redaction-passed content excerpt — the same text `SearchIndex` already indexes, so a
  vector hit and a BM25 hit describe the same document.
- batch through `provider.embed`, `index.upsert(id, vec)`.
- retracted objects → `index.remove`.
- write `last_tx`; `compact()` if over the tombstone ratio.

`EmbedStats { embedded, cached, errors, dim, model }` printed by `ekos commit` like the AI-desc line.

### 4. The vector arm in `retrieve` — `crates/ledger/src/fact_ledger.rs`

`FactLedger::retrieve` today fuses `ExactName` + `Bm25`. New: when
`req.query_embedding.is_some()` **and** a `VectorIndex` is on disk **and** its `dim` matches the
embedding length:

```
vector_hits = index.query(req.query_embedding.unwrap(), req.per_arm_limit)
rrf_fuse([ (ExactName, exact), (Bm25, bm25), (Vector, vector_hits) ], RRF_K, req.limit)
arms_run = ArmSet { bm25: true, vector: true }
```

Length/dim mismatch or no index → the arm is silently skipped, `arms_run.vector = false` — the
RFC 0119 contract. `Ledger` (SQLite) ignores `query_embedding` entirely.

`Hit` for a vector-only match carries a `Signal { source: Vector, rank, raw_score: cosine }`, so
`ekos_retrieve` / `--explain` show *why* it surfaced.

### 5. Surface — `--mode vector`

The query embedding is produced in `runtime`, not `ledger`. A new
`Retriever::embed_query(text) -> Option<Vec<f32>>` helper (holds the `Arc<dyn EmbeddingProvider>`,
does the one `.await`), and `RetrievalRequest` gains nothing — `query_embedding` already exists.

| surface | change |
|---|---|
| `ekos query find --mode <lexical\|vector\|hybrid>` | `lexical` (default) = today. `vector` / `hybrid` embed the query and set `query_embedding`; `vector` also drops the BM25 arm (`arms.bm25 = false`), `hybrid` keeps both. Errors clearly if `[embeddings]` is not configured. |
| MCP `ekos_search { query, limit?, mode? }` | same `mode` enum; `mode` defaults to `lexical`. The tool builds an `AiRuntime`-adjacent retriever with the configured provider. |
| `ekos_retrieve` | already dumps `arms_run` + per-`Hit` signals — now shows the `Vector` signal. |

`ekos ask` / EKL `SEMANTIC` staying lexical-only **this phase** — wiring them onto
`embed_query` is a one-liner each but changes cost (an embedding call per question); deferred to
a fast-follow once `--mode vector` has real mileage, same discipline as RFC 0124's demo-server
deferral.

### 6. Distributed — `publish_aux` / `fetch_aux`

`FactLedger` already calls `store.publish_aux("search")` after a seal and
`store.fetch_aux("search")` on open. Add the exact same two calls for `"vectors"` (the
`<ledger-dir>/vectors/` directory). This makes a compile-worker's vector index reach object
storage and a fresh gateway/query-worker pull it — no new protocol.

The **distributed query path** (`WorkerRequest::VectorSearch { partition, query_embedding, k }`
→ `WorkerResponse::ScoredHits`, RFC 0118 §8.6) is **deferred to RFC 0125b** — it needs the
gateway to embed the query and fan a `Vec<f32>` to every worker, which is a real protocol change.
Local + object-storage-backed single-node vector search is the whole of this phase.

---

## Non-goals

- **No ANN index.** Brute-force cosine over an mmap'd matrix; HNSW is a later isolated swap.
- **No distributed `VectorSearch` RPC** — RFC 0125b.
- **No `ekos ask` / EKL `SEMANTIC` vector wiring** — fast-follow.
- **No vector arm on SQLite `Ledger`** — degradation path, documented.
- **No re-embedding on every commit** — the pass is watermark-incremental and opt-in.
- **No new heavy dep** — `memmap2` is already a workspace dep; vectors are read with
  `f32::from_le_bytes`.

---

## Verification

- **`MockEmbeddingProvider`:** deterministic (same text → identical vector); token-overlap →
  higher cosine than disjoint text; output L2-norm ≈ 1.
- **`VectorIndex` units:** write N vectors → `query` returns the planted nearest first; `upsert`
  of an existing id tombstones the old row (count of live rows unchanged, old vector no longer
  returned); `compact()` at > 0.3 tombstones drops them; a `meta.json` dim mismatch wipes on open.
- **`embed_objects`:** on a seeded ledger with `[embeddings]` mock, `ekos commit` creates
  `vectors/meta.json` with the right `count`/`dim`; a second `commit` with no object changes
  embeds 0 (watermark).
- **`retrieve` fusion:** a `FactLedger` with a planted "sends welcome emails" `ai_overview` on an
  object named `dispatch_signup_notification` → `retrieve` with the mock query embedding for
  "welcome email" ranks it top; the same query with `query_embedding: None` does **not** (proves
  the arm, not a lexical coincidence). `arms_run.vector` is `true` with, `false` without.
- **`ekos query find --mode vector`** on the fixture finds the function whose name lacks the
  query words (RFC 0118 Phase 6 verification, mock provider). `--mode vector` without
  `[embeddings]` → a clear error.
- **Distributed:** a MinIO-backed seal publishes `vectors/…`; a fresh open `fetch_aux`-pulls it
  (reuse the existing `"search"` aux test shape).
- Full workspace gate + `tests/integration`.
