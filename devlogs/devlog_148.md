# Devlog 148 — RFC 0126: retrieval eval harness + per-arm telemetry (RFC 0118 complete)

**Date:** 2026-09-01
**PRs:** commit on branch `rfc/0118-compiled-knowledge-query-engine` → `main`
**Branch:** `rfc/0118-compiled-knowledge-query-engine` → `main`

---

## Summary

Phase 7 — the last — of RFC 0118. Every prior phase (0119–0125) was gated on "the eval harness
showing Recall@10 / MRR non-regression" (RFC 0118 §8.1), but that harness was scaffolded ad-hoc
per phase and never checked in. There was no single command answering "did retrieval quality
just regress?", and the RFC 0114 usage log recorded one `duration_ms` for a whole call — no way
to see the vector arm is the slow half of a hybrid search.

This ships both, permanently:

1. **`ekos_runtime::retrieval_eval`** — a checked-in graded query set + reference estate + pure
   metric math + a `BASELINE` const, with a CI gate test (`crates/runtime/tests/retrieval_eval.rs`)
   that fails the normal `cargo test` job on a > 2% drop, and a `benchmark/benches/retrieval_eval.rs`
   that prints the scoreboard and times understand/retrieve.
2. **`RankedResults.arm_timings`** — per-arm wall-clock + candidate counts, populated only by
   `FactLedger::retrieve`, surfaced on the `ekos_search` / `ekos_retrieve` MCP results, lifted
   into `query_log::LogEntry`, and printed by `ekos query find --explain`.

RFC 0118 is now Accepted with all eight phases shipped. The only documented follow-ons are the
optional `contextual_score` identity signal and the distributed `VectorSearch` RPC (RFC 0125b).

---

## PR — RFC 0126 (Phase 7 of RFC 0118)

### What was built

| Area | Change |
|---|---|
| `ledger::retrieval` | `ArmTiming { source: SignalSource, elapsed_ms: f64, candidates: usize }` (+ `Serialize` on `SignalSource`). `RankedResults` gains `arm_timings: Vec<ArmTiming>` — `from_ranked_pairs` / SQLite / partitioned / gateway leave it empty. Re-exported from the crate root. |
| `ledger::fact_ledger` | `FactLedger::retrieve` brackets each of the BM25, `ExactName`, and vector arms with an `Instant` and pushes an `ArmTiming` for the ones that actually ran. Pure observability — the `Instant` reads never touch what an arm returns or how `rrf_fuse` orders it; `hits` are byte-identical. |
| `ekos_runtime::retrieval_eval` (new) | `EvalQuery`/`EvalMetrics`/`QueryOutcome`/`EvalReport`; pure `recall_at_k` / `reciprocal_rank` / `ndcg_at_k` (unit-tested against textbook cases); `evaluate(runtime, queries, Option<QueryEmbedder>)`; `seed_reference_estate` (Northwind tables + FK edges, 3 code modules + 6 symbols with `ai_overview` prose, 3 doc sections — ~30 objects, hand-built, deterministic); `seed_reference_vectors` (mock-embeds the estate into an RFC 0125 `VectorIndex` and returns a matching query embedder); `reference_queries()` (~30 queries × 5 `QueryType`s); `BASELINE` + `check_regression`. |
| `recovery::embed` | `MockEmbeddingProvider::embed_sync` — a public sync single-text embed for the harness's per-query call site. |
| CI gate | `crates/runtime/tests/retrieval_eval.rs` — `retrieval_quality_meets_baseline` (seed → mock-vectors → evaluate → `check_regression` at tol 0.02 + hard floors) and `lexical_only_stack_is_still_usable` (no embedder → lower bar). Runs in the existing `cargo test --workspace` job. |
| Bench | `benchmark/benches/retrieval_eval.rs` — prints the full `EvalReport` + baseline verdict once, then Criterion-times `understand` (all queries), `retrieve` hybrid, and `retrieve` lexical. New `[[bench]]` entry. |
| `cli::query_log` | `LogEntry.arm_timings: Option<serde_json::Value>` (`skip_serializing_if = "Option::is_none"`). |
| `cli::mcp` | `ekos_search` result JSON gains `arm_timings`; `ekos_retrieve` gains `arms_run` + `arm_timings` (it now also runs the raw retrieval seam for the "show your work" view). `log_call` lifts a non-empty `arm_timings` array out of the result `Value` into the log entry — same mechanism as `estimate_result_count`, no handler signature change. |
| `cli::query` | `ekos query find --explain` prints an `arms: Bm25 3.1ms (12) · ExactName 0.2ms (1) · Vector 8.4ms (10)` line before results. |

### Implementation details worth remembering

- **The overall rank metrics cover the retrieval-shaped query types only** (`Lookup` / `Lexical`
  / `Conceptual`). `Structural` ("what depends on Orders") and `Aggregate` ("how many tables")
  route through REASON / EKL, not SEARCH — scoring their raw `retrieve()` output measures the
  wrong thing. They stay in the set and count toward `intent_accuracy` (all five types), which is
  its own gated metric. `is_retrieval_type` is the filter.
- **`find_objects_scored` is AND-semantics** (`search.rs` builds one `Occur::Must` per term). A
  full-sentence query like "what depends on the Orders table" requires *every* token in one
  document, so it returns nothing for `Structural` questions — which is why those are
  intent-only in the harness, and why real `Structural` answers need the RFC 0123 planner's
  graph op, not raw retrieve.
- **`indexed_content()` only concatenates `excerpt` / `symbols[]` / `ocr_text` / `ai_overview` /
  `ai_usage`** — not arbitrary `properties`. The reference estate's `table()` helper puts column
  names into `symbols` (as a real analyzer would) so column-name lexical queries can match; a
  bare `columns` property alone is invisible to BM25. (tantivy's default tokenizer also doesn't
  split CamelCase, so `TerritoryDescription` is one token — a known soft spot in the Lexical
  slice, baked into the baseline.)
- **`open` vs `open_existing` (from RFC 0125) matters here too:** `seed_reference_vectors` calls
  `embed_objects`, which opens the index against the *provider's* dim/model; the harness's query
  embedder is the *same* `MockEmbeddingProvider` (dim 64) so `FactLedger::retrieve`'s
  `open_existing` + dim check passes and the vector arm fires.
- **`RankedResults` had only 3 real construction sites** + `from_ranked_pairs`. Adding a field
  is cheap; the `#[non_exhaustive]`-style churn worry was unfounded.
- **Baseline regeneration:** `cargo test -p ekos-runtime retrieval_eval::tests::print_current --
  --ignored --nocapture` prints a paste-ready `EvalBaseline { … }`. Observed 2026-09-01: R@10
  0.841 / MRR 0.739 / nDCG 0.745 / intent 0.862; `BASELINE` set a hair under with 0.02 tol on
  top.

### Decisions (alternatives considered, why this choice)

- **Hand-built reference estate, not the real pipeline.** "vs. compiled fixtures" (RFC 0118 §9)
  tempted a `northwind.sql` → full-pipeline seed, but object ids would churn and the harness
  would depend on every analyzer. A ~30-object hand-built estate keyed by name is deterministic,
  fast, reviewable in a diff, and exercises all five arms (mock embeddings included).
- **Gate in `cargo test`, scoreboard in `cargo bench`.** CI runs `cargo test --workspace` (from
  `ekos/`) and `cargo bench` (from `benchmark/`) as separate jobs; `tests/integration` isn't in
  CI at all. Putting the *gate* in a `runtime` integration test means it runs on every PR; the
  bench is the human "show me the numbers" command RFC 0118 §9 Phase 7 named.
- **`arm_timings` populated by `FactLedger` only.** The partitioned / distributed / SQLite paths
  either have no separable arms or already aggregate per-partition; a half-populated field
  everywhere would be worse than an honestly-empty one. `FactLedger` is where the arms are
  separable and where a slow hybrid search actually needs diagnosing.
- **Telemetry lifted through the result `Value`, not a new signature.** `log_call` already pulls
  `result_count` out of the handler's returned JSON; `arm_timings` rides the same path. Zero
  handler-signature changes, and the array is exactly what an agent sees in the tool result.
- **`contextual_score` deferred.** Cosine of LLM embeddings of two objects' text as a same-entity
  identity signal is a real LLM-cost change to identity resolution (a sensitive over-merge area)
  — its own follow-on, now with RFC 0125's `EmbeddingProvider` + `cosine` as the primitives.

---

## Knowledge Captured

- **A retrieval eval harness must measure the surface it gates.** Scoring `retrieve()` output for
  `Structural`/`Aggregate` questions drags the number down for the wrong reason — those aren't
  SEARCH queries. Split: rank metrics over retrieval-shaped types, intent accuracy over all.
- **`find_objects_scored` requires every query token in one doc** (`Occur::Must` per term). Multi-
  word natural-language queries mostly fail it unless the words genuinely co-occur — the reason
  RFC 0121's `understand()` extracts keywords and RFC 0123 plans rather than passing the raw
  question to BM25.
- **`KirObject::indexed_content()` is a fixed whitelist** of property keys (`excerpt`, `symbols`,
  `ocr_text`, `ai_overview`, `ai_usage`) — a new property is *not* searchable unless an analyzer
  also folds it into one of those. Real analyzers put column/symbol names into `symbols`.
- **tantivy's default tokenizer doesn't split CamelCase or snake_case-into-words** beyond the
  separator — `TerritoryDescription` → one token `territorydescription`. `dispatch_signup_notification`
  *does* split (on `_`). Affects what lexical queries can match.
- **Timing an arm is safe observability if you only read `Instant`** around code whose output you
  don't touch. The eval harness's `lexical_only_stack_is_still_usable` test + the byte-identical-
  hits assertion in `retrieve_reports_per_arm_timings` are the guardrails.
- **Mock embeddings are good enough to gate on.** `MockEmbeddingProvider` (sha256 → per-token LCG
  scatter → L2-normalize) makes token-overlapping text land near each other deterministically, so
  the `Conceptual` slice of the harness genuinely exercises the vector arm without a provider.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0126-retrieval-eval-harness.md` | New RFC — Accepted, implemented same day. |
| `ekos/docs/rfcs/0118-compiled-knowledge-query-engine.md` | Status → Accepted (all phases); 0126 row updated. |
| `ekos/crates/ledger/src/retrieval.rs` | `ArmTiming`; `SignalSource: Serialize`; `RankedResults.arm_timings`. |
| `ekos/crates/ledger/src/lib.rs` | Re-export `ArmTiming`. |
| `ekos/crates/ledger/src/fact_ledger.rs` | Per-arm `Instant` brackets in `retrieve`; 1 new test + a timing assertion in an existing one. |
| `ekos/crates/ledger/src/partitioned/mod.rs`, `ekos/crates/distributed/src/gateway.rs` | `arm_timings: Vec::new()` at the two other construction sites. |
| `ekos/crates/runtime/src/retrieval_eval.rs` | New — the harness (types, metrics, `evaluate`, seed fns, query set, baseline). |
| `ekos/crates/runtime/src/lib.rs`, `ekos/crates/runtime/Cargo.toml` | `pub mod retrieval_eval`; `ekos-common` dep. |
| `ekos/crates/runtime/tests/retrieval_eval.rs` | New — the CI gate. |
| `ekos/crates/recovery/src/embed.rs` | `MockEmbeddingProvider::embed_sync`. |
| `ekos/crates/cli/src/commands/query_log.rs` | `LogEntry.arm_timings`. |
| `ekos/crates/cli/src/commands/mcp.rs` | `arm_timings` on `ekos_search` / `ekos_retrieve` results; `log_call` lifts it; test assertion. |
| `ekos/crates/cli/src/commands/query.rs` | `ekos query find --explain` arms line. |
| `benchmark/benches/retrieval_eval.rs`, `benchmark/Cargo.toml` | New scoreboard bench. |
| `TODO.md`, `README.md`, `docs/generated/ekos-self-documentation.html` | 0126 + RFC 0118-complete. |
