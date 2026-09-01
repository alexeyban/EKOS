# RFC 0118 — Compiled-Knowledge Query Engine: SEARCH → QUERY → REASON

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-09-01
**Supersedes (scope):** the deferred "full embedding-based semantic search" non-goal of RFC 0100
**Umbrella for:** RFCs 0119–0126 (per-phase, authored just-in-time)
**Related:** RFC 0009 (AI Runtime), 0013/0115 (MCP), 0014/0016 (content indexing / fact engine),
0018 (impact traversal), 0026 (document semantics), 0029 (identity), 0046/0061 (ekos ask),
0088/0100 (LLM descriptions / ai_overview indexing), 0111 §7 + 0113 B5 (distributed search),
0114 (query usage log)

> **Note:** RFC 0117 is referenced by commit `e8e1ca3`'s message (DbtAnalyzerPass) but was never
> filed. This RFC takes 0118; 0117 should be backfilled by whoever owns the dbt analyzer.

---

## 1. Motivation

Retrieval-augmented generation, as usually built, is:

```
raw documents → chunk → embed → vector DB → query → top-K chunks → LLM → answer
```

The system understands very little about what the documents contain. Every question re-pays the
cost of making sense of raw text.

EKOS already has a **Knowledge Compiler**. `ekos build → recover → resolve → compile → commit`
observes source systems and produces, deterministically and with evidence attached:

- **Entities** (`KirObject`) with canonical identity
- **Relationships** (`KirRelationship`) — a real dependency/call/containment/reference graph
- **Facts** — every object decomposed into EAV triples `(entity, attr, value)` in the
  fact-segment engine (RFC 0016), indexed in three covering sort orders (EAVT/AEVT/AVET)
- **Events** (`KirEvent`) — changes over time, tx-ordered
- **Summaries** — `ai_overview` / `ai_usage` prose (RFC 0088), evidence-grounded
- **Search indexes** — BM25 over name/kind/content (RFC 0014/0016)
- **Provenance** — `KirEvidence { confidence, location, fragment }` on every conclusion

The compiler already did the expensive analysis. What is missing is a **query layer that exploits
that structure** instead of falling back to "retrieve some chunks and ask the LLM what they
mean." This RFC defines that layer.

> **Traditional RAG searches documents. EKOS queries compiled knowledge.**

### 1.1 Three operations, not one

| Operation | Question shape | Mechanism | LLM? |
|---|---|---|---|
| **SEARCH** | "authentication documentation" | BM25 + Vector + Entity resolution → fused ranked objects | no |
| **QUERY** | "What does `authenticate()` return?" · "What depends on `UserService`?" | Direct fact lookup / graph traversal over the compiled structure | **no** |
| **REASON** | "Why is the authentication doc outdated?" | Query Planner → multi-step plan → typed Evidence Set → LLM explains | yes, but only to *explain* structured evidence |

Today EKOS collapses all three into `ekos ask` (retrieve objects → serialize whole state as JSON
→ LLM) and `ekos query find` (BM25 only). This RFC separates them and puts a planner in front.

### 1.2 What EKOS already has vs. what this RFC adds

| Capability | Exists today | This RFC |
|---|---|---|
| Entities / relationships / events / summaries | **Yes** — the compiler's output | reused unchanged |
| **Fact index** (`fact(entity, attr) → values`) | **Yes, internally** — `FactIndexes` EAVT/AEVT/AVET (`crates/ledger/src/index.rs`), tested `scan(ScanPrefix::Entity { entity, attr })` | *expose* it as a query surface + a per-`ObjectKind` **fact schema** the analyzers populate |
| BM25 lexical search | **Yes** — `SearchIndex` (`crates/ledger/src/search.rs`), `find_objects` | wrap in a scored, multi-signal seam; add RFC 0100's deferred `search_aliases` field |
| Graph traversal | **Yes** — `Runtime::load_neighborhood` / `trace_impact` (`crates/runtime/src/lib.rs`), `ekos_impact` (RFC 0018) | expose **named ops** (`dependents / dependencies / neighbors / path / ancestors / descendants`) as a QUERY surface + a **graph retrieval arm** for SEARCH |
| Entity resolution | **Partial** — `identity::similarity` Jaro-Winkler (`crates/identity/src/similarity.rs`), the `name` field | a first-class **query-understanding step**: mention extraction → name/identity match → `ResolvedEntity` |
| Query language | **Yes** — EKL (`crates/ekl`), `MATCH … WHERE … FROM … VIA … RETURN` | add `SEMANTIC 'text'`; keep the flat-clause grammar |
| **NL → executable plan** | **No** — `extract_search_terms` (RFC 0061) is a stopword stripper | the **Query Planner** + **Query Plan IR** — the centrepiece |
| **Typed Evidence Set** | **No** — `ekos ask` dumps whole `ObjectState` JSON | `EvidenceSet { Vec<EvidenceItem> }` with per-item provenance; the LLM prompt becomes "explain this evidence" |
| Vector / semantic search | **No** — `tantivy 0.22` has no vector field; `LlmProvider` has no `embed()`; zero vector deps | `EmbeddingProvider` + `VectorIndex` — **a gated later phase** (§8.6), justified against RFC 0100 |
| Provenance ("why do you believe this?") | **Yes** — evidence primitive | surfaced as an evidence *chain* per reasoned answer |
| Computed staleness | **Partial** — `ai_comment_check` (RFC 0088) | out of scope here → **RFC 0127** (referenced, not designed) |

---

## 2. SEARCH — find relevant knowledge

Input: a phrase. Output: a fused, ranked, evidence-carrying list of knowledge objects.

Three retrieval arms, merged by **Reciprocal Rank Fusion** (RRF, tuning-free — the arms produce
incomparable raw scores: BM25 is unbounded and, in partitioned/distributed mode, shard-local
(RFC 0111 §7); cosine is `[-1, 1]`; graph decay is its own thing):

- **BM25** — exact terminology: class/function/API/error/file names, identifiers, quoted phrases.
  `EKOSConfig` must return `EKOSConfig`, not "something like configuration."
- **Vector** — wording differs from the source ("stop docs from going obsolete" ≈ "detecting
  stale descriptions prevents documentation drift"). *Gated phase — §8.6.*
- **Entity** — the query names a thing the compiler already knows; resolve the mention and seed
  the graph arm / short-circuit to a direct fetch.

The seam is a new trait method (`KnowledgeStore::retrieve`, §8) returning `RankedResults` where
every `Hit` carries its `Signal`s (which arm ranked it, at what rank, raw score) — so
`--explain` and the reasoner can see *why* a result surfaced.

```rust
// crates/ledger/src/retrieval.rs
pub struct Hit {
    pub id: KirId,
    pub name: String,
    pub kind: Option<ObjectKind>,
    pub score: f32,                 // fused
    pub signals: Vec<Signal>,
}
pub struct Signal {
    pub source: SignalSource,
    pub rank: u32,                  // 0-based, within that source's own list
    pub raw_score: f32,             // informational: BM25 unbounded, cosine [-1,1], ExactName 1.0, Graph decay
}
pub enum SignalSource { Bm25, Vector, Graph, ExactName }

/// Reciprocal Rank Fusion (Cormack 2009). `k` default 60. Each list is already
/// rank-ordered (best first) from one source. Ties broken by KirId for determinism.
pub fn rrf_fuse(lists: &[(SignalSource, Vec<ScoredCandidate>)], k: f32, limit: usize) -> Vec<Hit>;
```

`promote_exact_name_matches` (today SQLite-only, `crates/ledger/src/lib.rs:1112`) becomes an
`ExactName` signal that works on the tantivy path too — closing a real existing gap.

---

## 3. QUERY — retrieve structured facts and relationships directly

No BM25 over 10,000 chunks, no LLM, no hallucination. Two surfaces.

### 3.1 Fact lookup

The compiler already knows `authenticate RETURNS AuthToken`. The EAV engine already stores and
indexes it. This RFC exposes:

```rust
// KnowledgeStore / Runtime
fn fact(&self, entity: &KirId, attr: &str) -> Result<Vec<FactValue>, LedgerError>;
fn facts_of(&self, entity: &KirId) -> Result<Vec<(String, FactValue)>, LedgerError>;
fn entities_with(&self, attr: &str, value: Option<&FactValue>) -> Result<Vec<KirId>, LedgerError>;
```

Backed by `FactIndexes::scan` (EAVT for `fact`, AEVT for `entities_with(attr, None)`, AVET for
`entities_with(attr, Some(v))`) via the `AttributeRegistry` name↔`AttrId` map — a prefix ranged
scan, not an object reconstruction. `FactIndexes` and the tested `scan(ScanPrefix::Entity {
entity, attr })` already exist (`crates/ledger/src/index.rs`); this phase is an *exposure*, not
a new index.

**Fact schema.** A per-`ObjectKind` set of well-known attribute paths the analyzers agree to
populate, so `fact(auth, "returns")` is meaningful and not a guess at `properties.*`:

| Kind | Well-known facts (examples) | Populated by |
|---|---|---|
| `Symbol` / `RustSymbol` / `PythonSymbol` | `signature`, `returns`, `raises`, `parameters[*]`, `defined_in`, `visibility`, `deprecated` | `rust_analyzer`, `python_analyzer` |
| `Table` | `columns[*]`, `primary_key`, `foreign_keys[*]`, `schema` | `sql_analyzer`, `dbt_analyzer` |
| `Module` / `File` | `language`, `path`, `size_bytes`, `exports[*]` | build / language analyzers |
| `Document` / `Section` | `documented_signature`, `mentions[*]`, `last_modified` | `local_docs_analyzer`, `document_semantics_analyzer` |

The schema is advisory (a `FactSchema` table in `kir`), not enforced — an object missing a fact
simply returns `[]`, same as `ai_overview` being absent today.

### 3.2 Named graph operations

```
neighbors(entity) · dependencies(entity) · dependents(entity)
path(a, b) · ancestors(entity) · descendants(entity) · impact(entity, direction, kinds, hops)
```

Thin named wrappers over `Runtime::load_neighborhood` / `trace_impact`, filterable by edge kind
and metadata (`deprecated = true`). The Query Planner routes `"what depends on X and is
deprecated"` here — `dependents(X)` then a metadata filter — instead of semantic retrieval.

---

## 4. REASON — combine facts into an answer

### 4.1 The Query Plan IR

The user's question is itself **compiled** into an executable plan (same philosophy: compile
once, execute):

```rust
// crates/runtime/src/retrieval/plan.rs
pub enum PlanNode {
    Resolve   { mention: String },                          // → ResolvedEntity
    Search    { query: String, arms: PlannedArms, limit: usize },
    Fact      { entity: EntityRef, attr: String },
    Graph     { op: GraphOp, seeds: Vec<EntityRef>, kinds: Vec<RelKind>,
                hops: u32, filter: Option<MetaFilter> },
    Compare   { left: Box<PlanNode>, right: Box<PlanNode>, on: CompareKey },  // e.g. dates, signatures
    Compose   { steps: Vec<PlanNode> },                     // sequential; later steps see earlier bindings
}
pub struct QueryPlan {
    pub raw: String,
    pub query_type: QueryType,            // Lookup | Lexical | Conceptual | Structural | Aggregate
    pub root: PlanNode,
    pub confidence: f32,                  // planner's confidence in the routing
}
```

### 4.2 The Query Planner (rules-first)

```
raw question
  → mention extraction (quoted / CamelCase / Module.Path / snake_case tokens)
  → entity resolution (identity::similarity against the name index) → Vec<ResolvedEntity>
  → intent rules → QueryType + PlanNode tree
```

Rules (illustrative, refined in RFC 0121):
- bare id / single exact resolved entity → `Lookup` (fetch state, no retrieval)
- "what does X return / throw / accept", "X's parameters" → `Fact { entity: X, attr }`
- "what depends on X", "callers of X", "what breaks if X changes", "X's dependencies" →
  `Graph { op, seeds: [X] }`
- "how many …", "list all … by …" → `Aggregate` → hand back to EKL `COUNT` / `GROUP BY`
- NL question, no dominant entity → `Search` (Conceptual) + a light `Graph` expansion
- "why is A stale / outdated / wrong vs B" → `Compare { Fact(A, sig), Fact(B, documented_sig), on: … }`
  + `Compare { Event(A, last_change), Event(B, last_update), on: date }`

Optional `[query-planner] planner = "llm"` — an LLM emits the **same `QueryPlan` IR** (schema in
the prompt), rules become the fast-path for obvious cases. The core path stays offline.

### 4.3 The Evidence Set

Replaces `AiRuntime::gather_context`'s whole-`ObjectState`-JSON dump (`crates/runtime/src/ai.rs`).

```rust
pub struct EvidenceItem {
    pub claim: String,            // "authenticate() returns AuthToken"
    pub value: serde_json::Value, // structured, when applicable
    pub source: KirId,            // the KirEvidence it came from
    pub location: String,         // "authentication.py:142"
    pub confidence: f32,
    pub as_of: DateTime<Utc>,     // the tx wall-time this fact was current at
    pub extracted_by: String,     // "python_analyzer" / "document_semantics_analyzer" / …
}
pub struct EvidenceSet { pub items: Vec<EvidenceItem>, pub plan: QueryPlan }
```

The LLM prompt becomes: *"Here is structured evidence assembled to answer the question. Explain
it. Cite each item you use."* — not *"here are some chunks, figure it out."*

### 4.4 Worked example — "Why does the payment doc still mention Stripe API v2?"

```
Query Planner →
  Compose[
    Resolve("payment") → payment module,
    Search("Stripe", arms: bm25) → doc sections mentioning Stripe,
    Fact(entity: <current Stripe integration symbol>, attr: "api_version"),
    Fact(entity: <that doc section>,                   attr: "documented_version"),
    Compare(Event(code, "last_change"), Event(doc, "last_update"), on: date),
  ]
Evidence Set →
  { claim: "code uses Stripe API v3", value: "v3", location: "billing/stripe.ex:19", as_of: 2026-08-14 }
  { claim: "payment.md documents Stripe API v2", value: "v2", location: "docs/payment.md:37", as_of: 2026-06-02 }
  { claim: "billing/stripe.ex last changed 2026-08-14", extracted_by: "git_analyzer" }
  { claim: "docs/payment.md last updated 2026-06-02", extracted_by: "git_analyzer" }
LLM → "The payment documentation still references Stripe API v2 because the integration was
       upgraded to v3 on 2026-08-14 (billing/stripe.ex:19), two months after the doc was last
       updated (2026-06-02)."
```

Every sentence is backed by an `EvidenceItem` with a source:line. "Why do you believe this?" is
answerable directly.

---

## 5. Architecture invariants preserved

- **Append-only ledger** — the query engine is read-only; no new mutable state. Indexes (BM25,
  vector, fact) are *derived and rebuildable* with `last_tx` watermarks, exactly like `SearchIndex`.
- **Evidence-carrying** — every `Hit` and every `EvidenceItem` traces to a `KirEvidence`.
- **Runtime is read-only** — the planner/reasoner compose over `&dyn KnowledgeStore` + `&Runtime`;
  no writes.
- **Deterministic, side-effect-free passes** — embedding / fact-schema population happen at
  commit time (opt-in, cached), not at query time. The *rules-first* planner is deterministic.
- **Offline by default** — SEARCH(BM25) + QUERY(fact + graph) + REASON(rules planner) need zero
  API keys. Vector and the LLM planner tier are opt-in; `RankedResults.arms_run` reports the
  downgrade when a provider is absent.
- **AI consumes knowledge through the Runtime only** — the reasoner's LLM never touches raw
  sources; it sees the Evidence Set.

---

## 6. Non-goals

- **Not a new storage engine.** Everything rides the fact-segment engine (RFC 0016), `SearchIndex`
  (RFC 0014), and `Runtime` traversal. The one new on-disk artifact is `VectorIndex` (§8.6), a
  sibling of `SearchIndex`.
- **Not replacing EKL.** EKL gains a `SEMANTIC` clause; the interpreter can call the retriever as
  a candidate-set strategy. `Aggregate` questions still route to EKL `COUNT` / `GROUP BY`.
- **Not true corpus-global BM25 in distributed mode.** RRF over shard-local ranks is more
  defensible than magnitude comparison of incomparable scores, but a global ranking needs a
  gather-df/rescore protocol that is explicitly out of scope (RFC 0113 B5 already documents this
  limitation).
- **Not computed staleness / drift detection.** `Custom("Drift")` objects, signature diffing,
  hash tracking, the code↔doc re-evaluation model → **RFC 0127**, a downstream consumer of this
  engine.
- **Not async'ing the `KnowledgeStore` trait.** The trait stays sync (evaluated + rejected —
  RFC 0005, `TODO.md`). The one async call — embedding the query — happens once, above the trait,
  in the planner; the result is passed down as a pre-computed `Vec<f32>`.
- **Not an ANN index.** Brute-force cosine over an mmap'd matrix is adequate at EKOS's scale
  (10⁴–10⁵ objects/partition); HNSW is a later, isolated swap behind the same interface.

---

## 7. Provenance

Because every fact carries `KirEvidence`, the engine can answer meta-questions:

```
Fact:   authenticate RETURNS AuthToken
Source: authentication.py:142   ·   Extracted by: python_analyzer   ·   Confidence: 0.99
```

`ekos ask --explain` and the MCP `ekos_retrieve` tool return the `QueryPlan` + `EvidenceSet`
alongside the answer, so retrieval is inspectable, reproducible, and debuggable — a hard
requirement for developer tooling.

---

## 8. Implementation phases

Each phase → its own dated impl RFC, authored just-in-time. Phases 0–4 are **fully offline**.
Crate layout (no new crate): `crates/ledger/src/{retrieval,vector}.rs`,
`crates/recovery/src/embed.rs`, `crates/runtime/src/retrieval/` (the orchestrator + planner +
graph arm). `runtime` gains a dep on `ekos-identity` (pure: `kir` + serde).

The seam itself:

```rust
// added to the KnowledgeStore trait; default impl wraps find_objects (rank-only) so every
// existing implementor compiles unchanged. FactLedger / PartitionedLedger / DistributedLedger
// override with the real scored + fused path.
fn retrieve(&self, req: &RetrievalRequest) -> Result<RankedResults, LedgerError> {
    let pairs = self.find_objects(req.bm25_query())?;
    Ok(RankedResults::rank_only(pairs, &req.raw, req.limit))
}
```

`RetrievalRequest` is built by the `Retriever` orchestrator in `runtime`, never by end consumers:
it carries the raw text, an optional keyword/boolean BM25 form, an optional **pre-computed** query
embedding (the vector arm is skipped unless it is `Some` *and* its length matches the on-disk
`VectorIndex` header dim), the arm set, and the pre- and post-fusion limits.

| RFC | Phase | Deliverable | Offline |
|---|---|---|---|
| **0119** | 0 — the seam | `KnowledgeStore::retrieve(&RetrievalRequest) -> RankedResults` (default impl wraps `find_objects`, rank-only). `find_objects` **byte-identical**. 4 consumers (`ekos query find`, MCP `ekos_search`, EKL `resolve_anchor`, `AiRuntime::search_for_question`) migrated, behaviour-identical. | ✅ |
| **0120** | 1 — fusion + rerank | `rrf_fuse` in `ledger`; `ExactName` signal (fixes tantivy path); `PartitionedLedger` / `DistributedLedger` scored-merge → RRF; `f_kind` → `STORED`; optional LLM cross-encoder rerank. **Flip `find_objects` → shim over `retrieve`** (gated on the eval harness showing Recall@10 / MRR parity). | ✅ |
| **0121** | 2 — query understanding | mention extraction + entity resolution (`ResolvedEntity`); rules-first intent classifier (`QueryType`), seeded from `extract_search_terms`; optional LLM fallback. | ✅ |
| **0122** | 3 — QUERY surface | `fact(entity, attr)` / `facts_of` / `entities_with` over `FactIndexes`; the per-`ObjectKind` **fact schema** + analyzer population; named graph ops (`dependents`, `path`, …); graph retrieval arm for SEARCH. | ✅ |
| **0123** | 4 — REASON | `QueryPlan` IR + rules planner (+ optional LLM planner tier); `EvidenceSet`; `AiRuntime` reworked: plan → execute nodes → assemble Evidence Set → LLM explains + cites; `Compare` / `Compose` multi-step. | ✅ (rules planner) |
| **0124** | 5 — surface | EKL `SEMANTIC 'text' [LIMIT k]`; MCP `ekos_search {limit?, mode?}` + new `ekos_query` (fact / graph, no LLM) + `ekos_retrieve` (plan + signals); `ekos ask` wired to the planner; `ekos query find --explain`. | ✅ (BM25 fallback) |
| **0125** | 6 — SEARCH vector arm **(gated)** | `EmbeddingProvider` (`recovery`: OpenAI `text-embedding-3-small`, Ollama `nomic-embed-text`, Mock, Cached); `VectorIndex` (`ledger`, §8.6); opt-in post-`commit` embed pass (like `[llm-description]`); `publish_aux("vectors")` distributed fan-out. **Gate: build when eval data / real usage shows BM25 + `ai_overview` prose + graph is insufficient** (RFC 0100's stated condition). | ✅ (opt-in, single-node; `ekos query find --mode vector\|hybrid`, `devlog_147`) |
| **0126** | 7 — eval + telemetry *(optional)* | graded eval set `{query, relevant_ids[]}` vs. compiled fixtures + `analytics`; Recall@10 / MRR / nDCG@10 per `QueryType`, CI-gated; per-arm timings into the RFC 0114 usage log; optional `contextual_score` semantic-identity signal. **Pull the eval harness forward as scaffolding during RFC 0120** (needed to prove the `find_objects` flip is safe). | ✅ |

### 8.1 Migration risk (carried into RFCs 0119–0120)

- **`find_objects` semantics blast radius.** `EklInterpreter::resolve_anchor` takes only hit 0;
  `AiRuntime::search_for_question` takes the first `max_matches`; `tests/integration` asserts
  specific objects are findable. Phase 0 keeps `find_objects` byte-identical; the Phase 1 flip is
  gated on the eval harness showing Recall@10 / MRR non-regression, and on `ExactName` +
  `is_simple_term` (RFC 0061) reproducing the `promote_exact_name_matches` cases.
- **`delegate_store!` macro** (`crates/ledger/src/lib.rs:1608`) lists every trait method and
  forwards to an inherent one. A trait *default* method is not auto-forwarded (fine for Phase 0);
  Phase 1 adds one macro arm — `fn retrieve(&self, req) { <$ty>::retrieve(self, req) }` — forcing
  an inherent `retrieve` on `Ledger` (rank-only, the degradation path) and `FactLedger` (real).
- **`PartitionedLedger`'s score-discarding merge** (`partitioned/mod.rs:1224`) becomes a
  per-hot-partition `find_objects_scored` fan + RRF merge; cold-partition skip stays, documented.
- **SQLite `Ledger`** — `retrieve` is a rank-only wrapper + `ExactName` from the existing
  `promote_exact_name_matches`. **No vector arm ever.** Documented as the degradation path.

### 8.6 `VectorIndex` (RFC 0125 preview)

Sibling of `SearchIndex`, at `<partition>/vectors/`. Files: `meta.json`
`{format_version, dim, model, metric:"cosine", count, normalized:true}`, `ids.bin` (count × 16B
KirId), `vectors.f32` (count × dim × f32 LE, **L2-normalized at write** → query-time cosine =
plain dot), `tombstones.bin`, `last_tx` (**separate watermark** — the opt-in embed pass lags BM25
by many commits). Append-only growth + mmap remap; `compact()` on `tombstone_ratio > 0.3` or
rebuild; dim/model mismatch → wipe + rebuild (the RFC 0103 stale-schema pattern). Brute-force
top-k: ~20–40 ms at 100k × 768. Only new dep: `bytemuck` (or hand-rolled `f32::from_le_bytes`).
Distributed: `publish_aux("vectors")` / `fetch_aux("vectors")` alongside the existing `"search"`
calls; a `WorkerRequest::VectorSearch { partition, query_embedding, k }` reusing
`WorkerResponse::ScoredHits`.

---

## 9. Verification (per phase)

Build a real target — `analytics/` (Plausible) or the `northwind` / `ecommerce` fixtures — with
the full pipeline; distributed via `docker-compose.dev.yml` + `ekos compile-worker`; eval via a
new `benchmark/benches/retrieval_eval.rs`.

- **Phase 0:** `retrieve` default returns the same ids + order as `find_objects` for a fixed
  query set (bare name, `order*`, punctuated question, `"literal phrase"`); a diff of
  `ekos query find` output before/after must be empty.
- **Phase 1:** canonical RRF fusion test (Cormack example); `ExactName` wins post-fusion on both
  backends; `ekos query find "README.md"` returns the real README first on a `FactLedger`
  workspace (today only SQLite does); distributed harness pins the new RRF order across 3
  divergent-IDF partitions.
- **Phase 2:** mention-extraction + fuzzy-match table; a ~40-query intent table (rules only);
  `ekos query find --explain "what depends on the orders table"` → `Structural`, `[orders]`.
- **Phase 3:** `fact(auth, "raises")` → `[AuthenticationError]` with source:line, zero LLM;
  `dependents(customers)` on compiled `northwind` returns the FK-dependent tables.
- **Phase 4:** the "why is the payment doc stale" plan executes; the Evidence Set contains the
  two dated facts; `ekos ask --explain` prints the plan + evidence + answer, all cited.
- **Phase 5:** `ekos_query` / `ekos_retrieve` JSON-RPC round-trips; `ekos ask "Who is Niklas
  Hambüchen and what did they contribute?"` retrieves the `Person` object (empty pre-work,
  RFC 0061 live-failure case).
- **Phase 6 (gated):** `[embeddings]` + local Ollama: `ekos commit` creates `vectors/`;
  `ekos query find --mode vector "the thing that sends welcome emails"` finds a function whose
  name/excerpt lack those words (the semantic analogue of RFC 0100's live "greeting" check).
- **Phase 7:** `cargo bench --bench retrieval_eval` prints the metric table; CI-gated vs. a
  checked-in baseline.

Every phase proves its **offline path** (no API key) explicitly.

---

## 10. Prior art in this repo

- **RFC 0100** deferred "full embedding-based semantic search — a new `EmbeddingProvider` trait,
  vector storage, an ANN or brute-force cosine index, and a reciprocal-rank-fusion blend with
  bm25" in favour of indexing the `ai_overview` prose EKOS already generates, stating: *"not
  abandoned, just no longer the first thing attempted — real usage against this cheaper approach
  will show whether … true continuous-similarity search is worth the real new infrastructure it
  needs."* RFC 0118 keeps that discipline: Phases 0–4 (fusion, entity resolution, graph, fact
  surface) are all cheaper approaches that ship first; the vector arm (Phase 6 / RFC 0125) stays
  gated on exactly that usage signal.
- **RFC 0061** solved `ekos ask`'s "retrieve brittleness" with keyword extraction; RFC 0118's
  Query Planner subsumes and generalises it.
- **RFC 0113 B5** already documents the shard-local-IDF limitation RRF improves but does not
  fully solve.
- **RFC 0016** built the EAV fact engine whose `FactIndexes` this RFC's QUERY surface exposes.
