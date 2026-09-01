# RFC 0119 — The retrieval seam: `KnowledgeStore::retrieve`

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-09-01
**Phase 0 of:** RFC 0118 (Compiled-Knowledge Query Engine)

---

## Motivation

RFC 0118 sequences the query engine into phases. Phase 0 is the seam every later phase hangs off:
a single scored, multi-signal retrieval method on `KnowledgeStore`, with the current lexical
behaviour preserved exactly.

Today the only search method on the trait is
`find_objects(&str) -> Vec<(KirId, String)>` — it drops scores, hard-caps at 50, and has no place
to attach *why* a result surfaced. The scored variants that do exist
(`SearchIndex::query_scored`, `FactLedger::find_objects_scored`, `DistributedLedger::search`) are
inherent-only and inconsistent. This RFC introduces `retrieve` + a `RankedResults` type carrying
per-hit `Signal`s, gives it a default impl that wraps `find_objects` so every implementor
compiles unchanged, and routes the four search consumers through it — **with byte-identical
output**.

No fusion, no new arms, no ranking change. That is Phase 1 (RFC 0120).

---

## Design

### `crates/ledger/src/retrieval.rs` (new module, `pub mod retrieval;`)

```rust
/// Which store-local retrieval arms to run. Graph is not store-local (it needs the Runtime),
/// so it is not represented here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmSet { pub bm25: bool, pub vector: bool }

/// Built by the Runtime-level orchestrator (RFC 0121+), never by end consumers. Handed to the
/// sync `KnowledgeStore::retrieve`.
#[derive(Debug, Clone)]
pub struct RetrievalRequest {
    /// Raw user text, verbatim. Drives exact-name logic (RFC 0120) and is the BM25 query when
    /// `keywords` is unset.
    pub raw: String,
    /// Processed keyword / boolean form for BM25 ("a AND b" / "a OR b"). Wins over `raw`.
    pub keywords: Option<String>,
    /// Pre-computed, L2-normalizable query embedding. Vector arm is skipped unless this is `Some`
    /// AND its length matches the on-disk `VectorIndex` header dim (RFC 0125). Unused in Phase 0.
    pub query_embedding: Option<Vec<f32>>,
    pub arms: ArmSet,
    /// Candidates pulled from each arm before fusion.
    pub per_arm_limit: usize,
    /// Final cap after fusion.
    pub limit: usize,
}

impl RetrievalRequest {
    /// BM25-only, `limit` 50 — the drop-in for a `find_objects` call.
    pub fn lexical(raw: impl Into<String>) -> Self;
    pub fn bm25_query(&self) -> &str; // keywords.as_deref().unwrap_or(&self.raw)
}

#[derive(Debug, Clone)]
pub struct RankedResults {
    pub hits: Vec<Hit>,
    /// Arms that actually ran — `vector` drops to `false` when no index is on disk / no embedding
    /// was supplied. Callers and `--explain` see the downgrade.
    pub arms_run: ArmSet,
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub id: KirId,
    pub name: String,
    /// `None` until `f_kind` is `STORED` (RFC 0120) or the orchestrator hydrates the final top-N.
    pub kind: Option<ObjectKind>,
    /// Fused score. Rank-only (a single arm, no raw scores) → `1 / (RRF_K + rank)`.
    pub score: f32,
    pub signals: Vec<Signal>,
}

#[derive(Debug, Clone)]
pub struct Signal {
    pub source: SignalSource,
    pub rank: u32,       // 0-based, within that source's own list
    pub raw_score: f32,  // 0.0 when unknown (find_objects gives no score)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalSource { Bm25, Vector, Graph, ExactName }

/// RRF constant, shared with the real fuser in RFC 0120.
pub const RRF_K: f32 = 60.0;
```

### The trait method (default impl)

```rust
// added to `pub trait KnowledgeStore`
fn retrieve(&self, req: &RetrievalRequest) -> Result<RankedResults, LedgerError> {
    let pairs = self.find_objects(req.bm25_query())?;
    Ok(RankedResults::from_ranked_pairs(pairs, SignalSource::Bm25, req.limit))
}
```

`from_ranked_pairs` truncates to `limit`, assigns `score = 1.0 / (RRF_K + rank)`, one
`Signal { Bm25, rank, raw_score: 0.0 }` per hit, `kind: None`, `arms_run: ArmSet { bm25: true,
vector: false }`.

- `delegate_store!` (`Ledger`, `FactLedger`) is **not** touched — a default trait method is not
  auto-forwarded, so both silently get the default. That is exactly the Phase 0 intent. (RFC 0120
  adds the one macro arm that gives `FactLedger` the real scored path.)
- `PartitionedLedger` / `DistributedLedger` manual `impl KnowledgeStore` blocks also inherit the
  default (no override added in Phase 0).

### `Runtime::retrieve` (passthrough)

```rust
pub fn retrieve(&self, req: &RetrievalRequest) -> Result<RankedResults, RuntimeError> {
    Ok(self.ledger.retrieve(req)?)
}
```

`Runtime::find_objects` stays a byte-identical passthrough (flipped to a shim over `retrieve` in
RFC 0120). `RetrievalRequest`, `RankedResults`, `Hit`, `Signal`, `SignalSource` re-exported from
`ekos_runtime`.

### The four consumers — migrated, behaviour-identical

| Consumer | Was | Now |
|---|---|---|
| `ekos query find` (`cli/commands/query.rs`) | `rt.find_objects(query)` | `rt.retrieve(&RetrievalRequest::lexical(query))?.hits` → `(id, name)` |
| MCP `ekos_search` (`cli/commands/mcp.rs`) | `runtime.find_objects(query)` | `runtime.retrieve(&RetrievalRequest::lexical(query))?.hits` → `{id, name}` |
| EKL `resolve_anchor` (`ekl/interpreter.rs`) | `find_objects(name).next()` | `retrieve(&RetrievalRequest::lexical(name))?.hits.into_iter().next()` |
| `AiRuntime::search_for_question` (`runtime/ai.rs`) | 3× `runtime.find_objects(x)` (AND→OR→raw) | 3× `runtime.retrieve(&RetrievalRequest::lexical(x))?.hits` → `(id, name)`; ladder unchanged |

Each site keeps its exact selection logic (anchor = hit 0; ask = `.take(max_matches)`; find =
print all). Because the default `retrieve` *is* `find_objects` wrapped, and RRF is not yet
applied, the id order out of each site is unchanged.

---

## Non-goals

- No fusion, no `ExactName` signal, no vector arm, no graph arm, no ranking change — RFC 0120+.
- `find_objects` (trait, `Ledger`, `FactLedger`, `Runtime`) is **untouched**.
- `RetrievalRequest.query_embedding` / `arms.vector` exist but are inert.
- No MCP tool schema change (`ekos_search` still returns `{matches: [{id, name}]}`).

---

## Verification

- **Unit** (`crates/ledger`): default `retrieve` returns the same `Vec<KirId>` in the same order
  as `find_objects` for: a bare name (`orders`), a prefix (`order*`), a punctuated question, a
  `"literal phrase"`, and the empty result. `Hit.score` is strictly decreasing;
  `arms_run == { bm25: true, vector: false }`.
- **Unit** (`crates/runtime`): `Runtime::retrieve` == `Runtime::find_objects` (ids).
- **Integration** (`tests/integration`): the existing findability assertions still pass through
  the migrated `ekos_search` path.
- **End-to-end:** build `analytics/` (or the `ecommerce` fixture); capture
  `ekos query find <q>` for a fixed query set before and after; the diff must be **empty**.
- Full workspace gate: `cargo fmt --check`, `build --workspace`, `clippy --workspace -D warnings`,
  `test --workspace`; `cargo bench --no-run` from `benchmark/`.
