# Devlog 147 — RFC 0125: the SEARCH vector arm — `EmbeddingProvider` + `VectorIndex` + `--mode vector`

**Date:** 2026-09-01
**PRs:** commit on branch `rfc/0118-compiled-knowledge-query-engine` → `main`
**Branch:** `rfc/0118-compiled-knowledge-query-engine` → `main`

---

## Summary

Phase 6 of RFC 0118 — the last retrieval arm. Phases 0–5 were all lexical (BM25 + `ExactName` +
fact/graph + a rules planner). A question phrased with none of the target object's words ("the
thing that sends welcome emails" → a function called `dispatch_signup_notification`) had no
lexical hook. RFC 0100 deferred a real vector arm pending usage signal; RFC 0118 kept it gated as
Phase 6. This ships it, within the constraints the existing architecture already set:

- **`KnowledgeStore` stays sync.** The one async call — embedding the *query* — happens once, in
  the CLI, above the trait; the result rides down as a pre-computed `Vec<f32>` on the
  `RetrievalRequest.query_embedding` field RFC 0119 already reserved.
- **The index is derived + rebuildable**, opt-in, and lags BM25 by many commits. Absent or
  dim-mismatched → the arm is silently skipped and `arms_run.vector` stays `false` (RFC 0119
  contract), never an error.
- **Offline still works with zero keys** — no `[embeddings]` table → `query_embedding` is `None`
  → `retrieve` runs exactly today's BM25 + `ExactName` path. `MockEmbeddingProvider` is
  deterministic and covers every test plus `provider = "mock"`.
- **SQLite `Ledger` never gets a vector arm** — documented degradation path, same as its
  rank-only `retrieve`. Single-node only this phase (no-op on a partitioned workspace).

---

## PR — RFC 0125 (Phase 6 of RFC 0118)

### What was built

| Area | Change |
|---|---|
| `recovery/src/embed.rs` (new) | `EmbeddingProvider` trait (`model_name`/`dim`/`async embed(&[String]) -> Vec<Vec<f32>>`, reuses `LlmError`). Impls: `MockEmbeddingProvider` (hashed-token → per-token LCG scatter → L2-normalize; deterministic, offline, default `dim` 64), `OllamaEmbeddingProvider` (`POST /api/embeddings`, sequential — Ollama has no batch endpoint; `nomic-embed-text`, `dim` 768), `OpenAiEmbeddingProvider` (`POST /v1/embeddings`, real batch; `text-embedding-3-small`, `dim` 1536), `CachedEmbeddingProvider<P>` (content-addressed `<root>/<2-hex>/<64-hex>.json` disk cache, splits hits/misses, mirrors `CachedLlmProvider`). `l2_normalize`/`cosine` helpers. `embed_objects(store, provider, index_dir, redaction) -> EmbedStats` — the post-`commit` pass. |
| `ledger/src/vector.rs` (new) | `VectorIndex` — sibling of `SearchIndex` at `<ledger-dir>/vectors/`. Files: `meta.json` (`format_version`/`dim`/`model`/`metric`/`count`/`normalized`), `ids.bin` (`count × 16B` `KirId`), `vectors.f32` (`count × dim` LE f32, L2-normalized at write), `tombstones.bin` (`count × 1B`), `last_tx` (bare `TxId`). `open` (stale `dim`/`model`/`format_version` → wipe + fresh, RFC 0103), `open_existing` (query-path open using the on-disk header, `Ok(None)` when no index), `upsert` (replace = tombstone old + append new, body never rewritten), `remove`, `query(&[f32], k)` (normalize query, brute-force dot product over live rows, partial-sort top-k), `should_compact`/`compact` (drop tombstones past 0.3, rewrite), `flush`. Reads vectors with `f32::from_le_bytes` over the file bytes — no `bytemuck`, no ANN dep. |
| `compiler-core/src/config.rs` | `EmbeddingsConfig` on `EkosConfig` — `enabled` (default `false`), `provider` (`Option`, falls back to `[llm] provider` then `"mock"`), `model`, `api_key_env`, `cache` (default `true`). Exactly `LlmDescriptionConfig`'s opt-in-table shape. |
| `cli/commands/commit.rs` | `run_embed` — the post-`commit` pass, gated on `config.embeddings.enabled`, runs **last** (after `run_llm_description`, so it embeds the `ai_overview` prose that step wrote). No spend prompt (embeddings are cheap + cached, unlike `[llm-description]` generation). No-op with a `note:` on SQLite or partitioned workspaces. `build_embedding_provider` (mirrors `build_llm_provider`; wraps in `CachedEmbeddingProvider` under `.ekos/embed-cache/` unless `cache = false`). `embed_query_blocking` — bridges the sync CLI call sites into the async provider (same `block_in_place`/`Handle::block_on` pattern as `run_clickhouse_query_blocking`); clear error when `[embeddings]` is not configured. `EmbedStats` line in the `ekos commit` summary. |
| `ledger/src/fact_ledger.rs` | The vector arm in `retrieve`: when `req.query_embedding.is_some()` **and** `VectorIndex::open_existing` yields an index whose `dim` matches **and** it's non-empty → `idx.query(emb, per_arm_limit)`, hits whose object was retracted since embedding are dropped (`get_object` returns `None`), fused as a third `(SignalSource::Vector, …)` list through the existing `rrf_fuse`. `arms_run` becomes `ArmSet { bm25: run_bm25, vector: vector_ran }`. `req.arms.bm25 == false` (i.e. `--mode vector`) skips the BM25 arm entirely. Distributed: `publish_aux("vectors")` after a seal (when `vectors/meta.json` exists), `fetch_aux("vectors")` on open (when a backend is configured and no local `vectors/`) — the exact same aux channel as `"search"`. |
| `cli` surface | `ekos query find --mode <lexical\|vector\|hybrid>` (`lexical` default = today; `vector` embeds the query + drops BM25; `hybrid` embeds + keeps both). Prints ` (vector)` / ` (bm25 + vector)` when the arm ran, a `note:` when `--mode vector\|hybrid` was asked but no index was on disk. MCP `ekos_search { query, limit?, mode? }` — same enum, `mode` defaults to `lexical`; response gains `arms_run: { bm25, vector }`. |

### Implementation details worth remembering

- **The watermark (`last_tx`) is informational in this design.** `embed_objects` is incremental
  by object **id** (`index.contains(&o.id)`), not by tx range — a re-run embeds only objects not
  already in the index. `last_tx` is written and carried forward but nothing reads it as a
  filter yet; it's there for a future tx-range rebuild and for `arms_run` freshness reporting.
  Retracted objects are **not** pruned from the index here — `retrieve` drops a stale hit at
  query time instead (the index only ever grows until `compact()`).
- **`embed.rs` lives in `recovery`, not `ledger`.** `ledger` stays sync and dependency-light;
  `recovery` already owns the `LlmProvider` boundary, `reqwest`, `sha2`, `async-trait`, and a
  `KnowledgeStore` dependency (for `llm_description`). The embed pass is the same kind of thing:
  a post-`commit` step that writes derived data through `&dyn KnowledgeStore`, not a
  `CompilerPass`.
- **Two `l2_normalize` functions, on purpose.** `ekos_recovery::l2_normalize` (public) and
  `ledger::vector`'s private `ekos_normalize` — the crates don't depend on each other and each
  normalizes at its own boundary (provider output vs. index write/query). Duplication is one
  five-line function; a shared crate for it would not pull its weight.
- **`MockEmbeddingProvider` is genuinely near-semantic for tests.** Per token: `sha256` → seed a
  64-bit LCG → scatter a unit of mass across all `dim` slots → sum across tokens → L2-normalize.
  So token overlap ⇒ higher cosine, deterministically, with zero deps. The RFC 0118 Phase 6
  verification ("finds the function whose name lacks the query words") runs on it.
- **`open_existing` vs `open`.** The retrieve path must **not** wipe-on-mismatch — a query with
  the wrong-dim embedding should skip the arm, not destroy the index. `open_existing` reads the
  on-disk `dim`/`model` and opens against *those*; the stale-check + rebuild only happens in the
  embed pass via `open(dir, provider.dim(), provider.model_name())`.
- **`arms_run` in the `ekos_search` MCP response.** Added so an agent can tell a vector/hybrid
  search silently degraded to lexical (no index built yet) rather than assuming semantic
  matching happened.

### Decisions (alternatives considered, why this choice)

- **Brute-force cosine, no ANN.** An mmap'd (here: read-into-`Vec`) `count × dim` matrix, dot
  product over live rows, partial sort. ~20–40 ms at 100k × 768 — fine for every workspace that
  exists today. HNSW is a later isolated swap behind the same `VectorIndex::query` signature.
- **Opt-in, `enabled = false` default.** Same discipline as `[llm-description]`. A workspace
  that never sets `[embeddings]` embeds nothing, ships no `vectors/`, and `retrieve` is
  byte-identical to Phase 5.
- **`ekos ask` / EKL `SEMANTIC` stay lexical-only this phase.** Wiring them onto `embed_query`
  is a one-liner each but changes per-question cost (an embedding call per `ask`). Deferred to a
  fast-follow once `--mode vector` has real mileage — same discipline as RFC 0124's demo-server
  deferral.
- **Distributed `VectorSearch` RPC deferred to RFC 0125b.** It needs the gateway to embed the
  query and fan a `Vec<f32>` to every worker — a real protocol change. Local + object-storage-
  backed single-node vector search (via the `publish_aux`/`fetch_aux` reuse) is the whole of
  this phase.
- **No spend confirmation on the embed pass.** Unlike `[llm-description]`'s generation calls,
  embeddings are cheap and disk-cached; a re-run with no object changes costs nothing.

---

## Knowledge Captured

- **A retrieval arm that can silently no-op must report it.** `arms_run.vector = false` is the
  RFC 0119 contract for "asked but couldn't run" (no index / dim mismatch / no query embedding).
  The CLI turns that into a visible `note:` and the MCP tool into an `arms_run` field — an agent
  or user who asked for semantic search needs to know they got lexical results instead.
- **`open` vs `open_existing` is the whole safety story for a derived index with a stale-wipe.**
  The write path (embed pass) opens against the *configured provider's* `dim`/`model` and wipes
  on mismatch (self-heal). The read path (`retrieve`) opens against the *on-disk* header and
  never wipes — a bad query embedding skips the arm. Conflating the two would let a query
  destroy the index.
- **`f32::from_le_bytes` over `chunks_exact(4)` is enough** for a vector store — no `bytemuck`,
  no `safe-transmute`. `memmap2` is already a workspace dep if the read-into-`Vec` ever needs to
  become a real mmap; the query loop doesn't care which it is.
- **Incremental-by-id beats incremental-by-tx-range for an embed pass.** Object ids are stable;
  a tx-range filter would need careful handling of re-embeds after an `ai_overview` changes. The
  index just tracks "have I seen this id" and a `dim`/`model` bump forces a full rebuild anyway.
- **`MockEmbeddingProvider`'s hashed-token-LCG-scatter trick** gives you deterministic,
  dependency-free, genuinely-token-similar embeddings for tests. Worth copying for any future
  "I need a fake embedder that isn't garbage" need.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0125-vector-search-arm.md` | New RFC — Accepted, implemented same day. |
| `ekos/crates/recovery/src/embed.rs` | New — `EmbeddingProvider` trait + 4 impls, `embed_objects` pass, `l2_normalize`/`cosine`, `EmbedStats`. |
| `ekos/crates/recovery/src/lib.rs` | `pub mod embed` + re-exports. |
| `ekos/crates/ledger/src/vector.rs` | New — `VectorIndex` (open/open_existing/upsert/remove/query/compact/flush). |
| `ekos/crates/ledger/src/lib.rs` | `pub mod vector`. |
| `ekos/crates/ledger/src/fact_ledger.rs` | Vector arm in `retrieve`; `publish_aux`/`fetch_aux` for `"vectors"`; 2 new tests. |
| `ekos/crates/compiler-core/src/config.rs` | `EmbeddingsConfig` + `default_true`. |
| `ekos/crates/cli/src/commands/commit.rs` | `run_embed`, `build_embedding_provider`, `embed_query_blocking`, `EmbedStats` summary line. |
| `ekos/crates/cli/src/commands/query.rs` | `find(.., mode)` — `--mode` handling + arm reporting. |
| `ekos/crates/cli/src/commands/mcp.rs` | `ekos_search` `mode` param + `arms_run` in the response. |
| `ekos/crates/cli/src/bin/ekos.rs` | `QueryCommands::Find { mode }` arg. |
| `TODO.md` | 0125 checked under the RFC 0118 block. |
| `README.md` | `--mode` + `[embeddings]` note. |
| `docs/generated/ekos-self-documentation.html` | Vector-search capability section. |
