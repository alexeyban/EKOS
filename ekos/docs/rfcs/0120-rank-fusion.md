# RFC 0120 — Rank fusion + the exact-name signal

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-09-01
**Phase 1 of:** RFC 0118 · **builds on:** RFC 0119

---

## Motivation

RFC 0119 gave `retrieve` a rank-only default. This phase makes it a real fused retrieval:

1. **Reciprocal Rank Fusion** (`rrf_fuse`) — the one merge primitive for combining ranked lists
   from arms whose raw scores are incomparable (BM25 is unbounded and, in partitioned/distributed
   mode, shard-local — RFC 0111 §7; later, cosine is `[-1, 1]`).
2. **The `ExactName` signal** — an exact case-insensitive name match is fused in as its own
   top-ranked list. `promote_exact_name_matches` (`crates/ledger/src/lib.rs`) does this today
   **only on the SQLite path**; a `FactLedger` workspace (the default since 2026-08-21) never
   promoted `README.md` for the query `"README"`. This closes that gap and converges the two
   backends.
3. **The federated merges become RRF** — `PartitionedLedger::retrieve` (today: concat + dedup,
   no re-score) and `DistributedLedger::search` (today: `total_cmp` over incomparable shard-local
   BM25) both switch to RRF over per-shard ranks. This is a defensible improvement, **not** a
   true corpus-global ranking — that needs a gather-df/rescore protocol and stays out of scope
   (RFC 0113 B5 already documents the limitation).

### Behaviour change, stated plainly

At the four migrated call sites (`ekos query find`, MCP `ekos_search`, EKL `resolve_anchor`,
`AiRuntime::search_for_question`), ranking on a **`FactLedger` / partitioned / distributed**
workspace changes: an exact name match is now promoted to the front, matching what the SQLite
backend already did. `find_objects` (trait / `Ledger` / `FactLedger` / `Runtime`) is **still
untouched** — it remains the stable legacy `(id, name)` method. The `find_objects → retrieve`
shim flip is deferred (a later cleanup once `retrieve` has soaked).

---

## Design

### `crates/ledger/src/retrieval.rs`

```rust
/// One arm's candidate before fusion.
#[derive(Debug, Clone)]
pub struct ScoredCandidate {
    pub id: KirId,
    pub name: String,
    pub kind: Option<ObjectKind>,
    pub raw_score: f32,        // arm-native; informational after fusion
}

/// Reciprocal Rank Fusion (Cormack 2009). Each list is best-first from one source. A doc's fused
/// score is `Σ 1/(k + rank)` over the lists it appears in (0-based rank, matching RFC 0119's
/// single-list score). Ties broken by `KirId` for determinism. `limit` caps the output.
pub fn rrf_fuse(
    lists: &[(SignalSource, Vec<ScoredCandidate>)],
    k: f32,
    limit: usize,
) -> Vec<Hit>;

/// The subset of `candidates` whose name matches `query` case-insensitively after trim — the
/// `ExactName` arm's list, in the candidates' original order. Mirrors
/// `promote_exact_name_matches`' comparison exactly.
pub fn exact_name_matches(query: &str, candidates: &[ScoredCandidate]) -> Vec<ScoredCandidate>;
```

### `FactLedger::retrieve` (inherent) + one `delegate_store!` arm

```rust
// delegate_store! gains:
fn retrieve(&self, req: &RetrievalRequest) -> Result<RankedResults, LedgerError> {
    <$ty>::retrieve(self, req)
}
```

- `FactLedger::retrieve`: `find_objects_scored(bm25_query, per_arm_limit)` → BM25 candidates;
  `exact_name_matches(raw, &bm25)` → the ExactName list; `rrf_fuse([(Bm25, …), (ExactName, …)],
  RRF_K, limit)`. `arms_run = { bm25: true, vector: false }`.
- `Ledger::retrieve` (SQLite, degradation path): rank-only wrap of `find_objects` — it already
  promotes exact names internally, so no second ExactName arm.

### Federated overrides

- `PartitionedLedger::retrieve`: for each **hot object partition**, `FactLedger::retrieve(req)`
  → per-partition `Hit`s; treat each partition's list as one ranked list keyed by
  `SignalSource::Bm25`; `rrf_fuse` across partitions (dedup by id inside the fuser); `limit`.
  Cold partitions still skipped, documented. **Plus a cross-partition `ExactName` arm** —
  `exact_name_matches(req.raw, &union_of_all_partition_candidates)` as its own `rrf_fuse` list.
  Without it, a per-partition exact-name promotion only ranks *within* its partition and, once
  merged, merely ties (rank 0) with a strong lexical hit in another partition and loses the
  `KirId` tiebreak — silently reintroducing the exact regression this signal fixes (found by the
  RFC 0111/0118 full-stack test, `devlog_149`).
- `DistributedLedger::retrieve` + `DistributedLedger::search`: fan `find_objects_scored` per
  object partition (unchanged), then `rrf_fuse` the per-partition lists (with the same
  cross-shard `ExactName` arm over the candidate union) instead of the `HashMap` keep-max +
  `total_cmp` sort. `find_objects` on the gateway rides `search` as before.

### `Hit.kind`

Stays `None` in Phase 1 (populating it needs `f_kind` → `STORED`, a tantivy schema change
deferred to when `--explain` consumes it, RFC 0124). `ScoredCandidate.kind` is plumbed through
for later use.

---

## Non-goals

- No `f_kind` schema change; `Hit.kind` stays `None`.
- No `find_objects` shim flip.
- No vector or graph arm.
- No optional LLM cross-encoder rerank (a later, opt-in add).
- Distributed ranking stays an approximation (per-shard IDF).

---

## Verification

- **Unit** (`crates/ledger`): `rrf_fuse` on the canonical Cormack example
  (`A=[d1,d2,d3]`, `B=[d2,d3,d1]`, `k=60` → order `d2, d1, d3`); dedup across lists; `limit`;
  empty lists → empty. `exact_name_matches` case-insensitivity + trim.
- **Unit** (`crates/ledger`): `FactLedger::retrieve` for `"README"` against a store containing
  `README.md`, `readme_generator`, `docs/README-old` → `README.md` (or the exact-name object)
  ranks **first**, and would **not** without the ExactName arm (assert both).
- **Unit** (`crates/distributed`): the existing gateway harness — 3 partitions with divergent
  per-shard IDF, assert the new RRF order and pin it; `search` still returns the full
  de-duplicated set, `k`-bounded.
- **Unit** (`crates/ledger` partitioned): a query whose top BM25 hit lives in a later partition
  now outranks an earlier partition's weaker hit (impossible under the old concat order) — assert
  the order changed.
- **Integration** (`tests/integration`): findability assertions still pass.
- **End-to-end:** build `analytics/`; `ekos query find "README"` returns the real `README.md`
  first on the (fact-engine) workspace — today it does not.
- Full workspace gate + `cargo bench --no-run` from `benchmark/`.
