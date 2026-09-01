# RFC 0126 — Phase 7 of RFC 0118: retrieval eval harness + per-arm telemetry

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-09-01
**Implemented:** 2026-09-01 (`devlog_148`)
**Phase 7 of:** RFC 0118 · **builds on:** every prior phase (0119–0125) — this is the scoreboard for all of them
**Defers:** the optional `contextual_score` semantic-identity signal → a future follow-on

---

## Motivation

Phases 0–6 built a five-arm retrieval stack (BM25, `ExactName`, graph, a rules planner, a vector
arm) and flipped `find_objects` onto it — each phase gated on "the eval harness showing Recall@10
/ MRR non-regression" (RFC 0118 §8.1). **That harness was scaffolded ad-hoc per phase and never
checked in.** There is no single command that answers "did retrieval quality just regress?", and
no per-arm latency data — the RFC 0114 usage log records one `duration_ms` for the whole call,
so nobody can see that the vector arm is the slow half of a hybrid search.

This RFC ships both, permanently:

1. **A graded query set + metrics + a checked-in baseline**, gated in CI. A change that drops
   Recall@10 / MRR / nDCG@10 below the baseline (minus a tolerance) fails the build.
2. **Per-arm timings** on `RankedResults`, surfaced into the RFC 0114 usage log and
   `ekos query find --explain`.

Non-negotiable constraints from the existing architecture:

- **The metrics harness is offline and LLM-free.** It seeds a `FactLedger` with a fixed
  reference estate and runs `understand` + `retrieve` — no API key, no network, deterministic.
- **Timings never change an answer.** `arm_timings` is pure observability; `rrf_fuse` input and
  output are byte-identical with and without it. A logging/timing failure never fails a query.
- **`RankedResults` stays cheap to construct.** Three real construction sites
  (`FactLedger`/`PartitionedLedger`/`gateway`) + `from_ranked_pairs`; the new field defaults to
  empty and only `FactLedger::retrieve` populates it.

---

## Design

### 1. `ekos_runtime::retrieval_eval` — the harness

New module in `runtime` (it already owns `Runtime`, `retrieve`, and `understand`).

```rust
/// One graded query: the text, the shape we expect it classified as, and the object *names*
/// (not ids — ids are unstable) that a good retrieval must surface in the top-k.
pub struct EvalQuery {
    pub query: &'static str,
    pub expect_type: QueryType,
    pub relevant: &'static [&'static str],
}

/// Recall@k / MRR / nDCG@k over one run, plus the same three sliced by `QueryType`.
pub struct EvalMetrics {
    pub recall_at_10: f64,
    pub mrr: f64,
    pub ndcg_at_10: f64,
    pub n: usize,
}
pub struct EvalReport {
    pub overall: EvalMetrics,
    pub by_type: Vec<(QueryType, EvalMetrics)>,
    pub per_query: Vec<QueryOutcome>,      // for --verbose / regenerating the baseline
    pub intent_accuracy: f64,              // fraction where understand() matched expect_type
}
```

- **`seed_reference_estate(store: &dyn KnowledgeStore)`** — ~55 objects, hand-built, modelling a
  small realistic estate: the Northwind tables (columns as `properties`, real `ForeignKey`
  relationships), 3 code `Module`s + 6 `Symbol`s carrying evidence-shaped `ai_overview` prose
  ("dispatches the welcome email to newly-registered customers"), 4 doc `Section`s. Checked in
  as code, not a fixture file — type-checked, reviewable in a diff, no path wrangling.
- **`reference_queries() -> &'static [EvalQuery]`** — ~40 queries across all five `QueryType`s:
  bare names (`Lookup`), keyword (`Lexical`), token-disjoint concept queries the vector arm
  exists for (`Conceptual` — "the thing that emails new customers"), `"what depends on Orders"`
  (`Structural`), `"how many tables"` (`Aggregate`).
- **`evaluate(runtime, queries) -> EvalReport`** — for each query: `understand()` for the intent
  check, `retrieve()` for the ranked ids, resolve `relevant` names → ids via `find_objects`
  exact match, compute the rank metrics. Pure functions `recall_at_k` / `reciprocal_rank` /
  `ndcg_at_k` live here and are unit-tested directly against textbook examples.
- **`BASELINE: EvalBaseline`** — a `const` of the three overall numbers + per-type Recall@10,
  captured from a real run. **`check_regression(report, baseline, tol) -> Result<(), String>`**
  fails when any metric drops more than `tol` (default `0.02`) below its baseline.

Regenerating the baseline: `cargo test -p ekos-runtime retrieval_eval::print_current -- --ignored
--nocapture` prints a paste-ready `EvalBaseline { … }` block.

### 2. The CI gate — `ekos-runtime` test

`crates/runtime/tests/retrieval_eval.rs` (a normal integration test, so it runs in the existing
`cargo test --workspace` CI job — *this* is the gate):

```rust
#[test]
fn retrieval_quality_meets_baseline() {
    let dir = tempdir().unwrap();
    let fl = FactLedger::open(&dir.path().join("fl")).unwrap();
    retrieval_eval::seed_reference_estate(&fl);
    let rt = Runtime::over(&fl);
    let report = retrieval_eval::evaluate(&rt, retrieval_eval::reference_queries());
    retrieval_eval::check_regression(&report, &retrieval_eval::BASELINE, 0.02)
        .unwrap_or_else(|e| panic!("retrieval quality regressed:\n{e}\n{report:#}"));
    assert!(report.intent_accuracy >= 0.80, "intent classifier: {}", report.intent_accuracy);
}
```

### 3. The bench — `benchmark/benches/retrieval_eval.rs`

Criterion bench (`harness = false`, `criterion_main!` like every sibling). It:

- times `understand` + `retrieve` over the reference query set on the seeded `FactLedger`
  (`retrieval_eval_understand`, `retrieval_eval_retrieve` groups), and
- once, before the timed loop, prints the full `EvalReport` metric table to stdout — so
  `cargo bench --bench retrieval_eval` is the human "show me the scoreboard" command named in
  RFC 0118 §9 Phase 7, and the benchmark CI job surfaces it in its log.

`benchmark/Cargo.toml` gains the `[[bench]]` entry; deps are already present (`ekos-runtime`,
`ekos-ledger`, `tokio`, `criterion`).

### 4. Per-arm timings — `RankedResults.arm_timings`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ArmTiming {
    pub source: SignalSource,   // Bm25 | ExactName | Vector | Graph
    pub elapsed_ms: f64,
    pub candidates: usize,      // how many rows this arm contributed pre-fusion
}
```

- `SignalSource` gains `Serialize` (currently `Debug/Clone/Copy/PartialEq/Eq/Hash`).
- `RankedResults` gains `pub arm_timings: Vec<ArmTiming>`. `from_ranked_pairs`, the SQLite
  `retrieve`, `PartitionedLedger::retrieve` and `gateway::search_ranked` set it to `Vec::new()`
  (their per-arm structure is either trivial or already aggregated); only
  **`FactLedger::retrieve`** populates it — one `Instant::now()` bracket around each of the
  BM25, `ExactName`, and vector arms.
- **RFC 0114 usage log:** `query_log::LogEntry` gains
  `#[serde(skip_serializing_if = "Option::is_none")] pub arm_timings: Option<serde_json::Value>`.
  The `ekos_search` MCP handler already returns `arms_run` in its result JSON — it also returns
  `arm_timings`, and `mcp.rs::log_call` lifts that array out of the result `Value` into the log
  entry (exactly how it already lifts `result_count` via `estimate_result_count`). No handler
  signature changes.
- **`ekos query find --explain`** prints an `arms:` line — `bm25 3.1ms (12) · exact 0.2ms (1) ·
  vector 8.4ms (10)` — after the existing plan render.

### 5. What is *not* here

- **`contextual_score`** (TODO.md line 899: cosine of LLM embeddings of two objects' text as a
  same-entity identity signal) — a real LLM-cost identity-resolution change, its own follow-on.
  RFC 0125's `EmbeddingProvider` + `cosine` are the primitives it will use.
- **nDCG with graded (non-binary) relevance** — the query set is binary-relevant for now
  (`relevant` is a flat name list). The `ndcg_at_k` fn takes a gain function, so upgrading later
  is additive.
- **Telemetry for `ekos_ekl SEMANTIC` / `ekos ask`** — those route through `retrieve` too, but
  their log entries are dominated by interpretation/LLM time; per-arm retrieval timing there is
  noise. `ekos_search` / `ekos_retrieve` / `ekos query find` are where it's actionable.

---

## Non-goals

- No new eval *framework* — no `insta`, no golden-file diffing. Three floats and a tolerance.
- No online/production metric collection — the query set is curated and checked in.
- No per-arm timing on the SQLite / partitioned / distributed paths — `FactLedger` is where the
  arms are separable and where it matters.
- No change to fusion, ranking, or any answer. Pure addition.

---

## Verification

- **Metric math units** (`retrieval_eval` module tests): `recall_at_k` / `reciprocal_rank` /
  `ndcg_at_k` against hand-computed textbook cases (perfect ranking → 1.0; known imperfect
  ranking → the known fraction; empty relevant set handled).
- **`seed_reference_estate` + `evaluate`**: the reference run's overall Recall@10 ≥ 0.85,
  MRR ≥ 0.75, intent accuracy ≥ 0.80 (the seeded estate is designed to be answerable).
- **The CI gate test** fails loudly when a metric is > 2% under baseline (proved by a temporary
  local edit that nerfs `rrf_fuse`).
- **`arm_timings`**: a `FactLedger::retrieve` with a query embedding returns three `ArmTiming`s
  with `source` ∈ {Bm25, ExactName, Vector}, `elapsed_ms > 0.0` for the arms that ran,
  `candidates` matching the pre-fusion list lengths; a lexical-only retrieve returns two.
  Byte-identical `hits` with the field present vs. a build with timing stubbed out.
- **Usage log**: an `ekos_search { mode: "hybrid" }` MCP call writes a `query-log.jsonl` line
  whose `arm_timings` array has the three arms.
- **`cargo bench --bench retrieval_eval`** prints the metric table and completes; the
  `benchmark` CI job artifact contains it.
- Full workspace gate + `tests/integration` + `cargo bench`.
