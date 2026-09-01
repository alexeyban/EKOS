# RFC 0123 — REASON: the Query Plan IR, rules planner, and typed Evidence Set

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-09-01
**Phase 4 of:** RFC 0118 · **builds on:** RFC 0121 (understanding) + RFC 0122 (QUERY surface)

---

## Motivation

`AiRuntime::gather_context` today does one thing: BM25 → take top-N → dump each object's whole
`ObjectState` as JSON → "here are some objects, figure it out." The LLM re-derives structure the
compiler already knows, and the answer's provenance is whatever evidence ids happened to ride
along in the JSON.

REASON replaces the guesswork with a **compiled plan**. The question is itself compiled — the
same philosophy as the rest of EKOS: a rules planner turns a [`QueryUnderstanding`](RFC 0121)
into a typed **`QueryPlan`**, the plan executes against the QUERY surface (RFC 0122) + the
retrieval seam (RFC 0119), and the result is a typed **`EvidenceSet`** — a flat list of atomic
claims, each with its own source, location, confidence, and extractor. The LLM's job shrinks to
*"explain this evidence, cite each item you use."*

---

## Design — `crates/runtime/src/reason.rs`

### The IR

```rust
pub enum EntityRef {
    Resolved(KirId),        // the planner already bound it (RFC 0121 resolution)
    Mention(String),        // bind at execution from a Resolve step
}

pub enum PlanNode {
    Resolve { mention: String },
    Search  { query: String, limit: usize },
    Fact    { entity: EntityRef, attr: String },
    Graph   { op: StructuralOp, seed: EntityRef, hops: u32 },
    Compose { steps: Vec<PlanNode> },   // executed in order; every step contributes evidence
}

pub struct QueryPlan {
    pub raw: String,
    pub query_type: QueryType,
    pub root: PlanNode,
    pub confidence: f32,   // the planner's own confidence in the routing (0..=1)
}
```

`Compare` (dated-fact diffing for "why is X stale") is named in RFC 0118 §4 but **deferred** —
it needs event-time facts the analyzers don't populate yet (that's RFC 0127 territory). The enum
stays open for it.

### The rules planner — `plan(&QueryUnderstanding) -> QueryPlan`

Deterministic, offline. Rules are tried in this order; the first match wins:

| # | `QueryUnderstanding` shape | `PlanNode` | confidence |
|---|---|---|---|
| 1 | a fact-attribute question ("what does X return/raise/accept", "X's columns/params") + primary entity — checked **first**, ahead of the intent class, because RFC 0121 classifies "what does …" as `Structural` | `Compose[ Fact { primary, <mapped attr> }, Fact { primary, "*" } ]` | `primary.confidence` |
| 2 | `Lookup` + a primary entity | `Fact { entity: Resolved(primary), attr: "*" }` (→ `facts_of`) | `primary.confidence` |
| 3 | `Structural` + `structural_op` + primary entity | `Compose[ Graph { op, seed: primary, hops: 2 }, Fact { primary, "*" } ]` | `primary.confidence` |
| 4 | `Aggregate` | `Search { raw, limit: 50 }` + a diagnostic that EKL `COUNT`/`GROUP BY` is the real surface | 0.3 |
| 5 | `Conceptual` / `Lexical` / anything else | `Search { keywords-or-raw, limit: max_matches*… }`; if a primary entity resolved, `Compose` a `Graph { Neighborhood, primary, 1 }` after it | 0.5–0.8 |

`Lookup`/`Structural`/`Aggregate` without a primary entity fall through to a low-confidence (0.4) `Search`.

The fact-attribute keyword map is small and explicit: `returns`/`return` → `"returns"`,
`raises`/`throws`/`exceptions` → `"raises"`, `parameters`/`params`/`arguments`/`accepts` →
`"parameters"`, `signature` → `"signature"`, `columns`/`fields` → `"columns"`,
`type` → `"type"`. A miss just falls through to the `Search` rule.

Optional `[query-planner] planner = "llm"` (RFC 0118 §4.2) — an LLM emits the **same
`QueryPlan`** shape; rules stay the fast path. **Not in this phase** — the seam ships as a
`PlannerTier { Rules, Llm }` enum + `plan_with(&QueryUnderstanding, PlannerTier) -> QueryPlan`,
with `Llm` falling back to `Rules` so RFC 0124+ can add the real tier without a signature change.

### Execution — `execute(&QueryPlan, &Runtime) -> Result<EvidenceSet, RuntimeError>`

```rust
pub struct EvidenceItem {
    pub claim: String,              // "orders.schema = \"public\""  /  "order_items depends on orders"
    pub value: serde_json::Value,   // structured form when there is one, else Value::Null
    pub source: Option<KirId>,      // a KirEvidence id when the object carries one
    pub location: String,           // "schema.sql:12"  /  "" when unknown
    pub confidence: f32,
    pub extracted_by: String,       // analyzer / source kind: properties["source_kind"|"analyzer"|"language"], else ""
    pub entity: Option<KirId>,      // the object this claim is about
}
pub struct EvidenceSet { pub items: Vec<EvidenceItem>, pub plan: QueryPlan, pub diagnostics: Vec<Diagnostic> }
```

- `Resolve` → binds a `KirId` into the execution environment (retrieve → best hit); contributes
  no item on its own.
- `Fact { entity, "*" }` → one item per `facts_of` entry.
- `Fact { entity, attr }` → one item (or none if the path is absent).
- `Graph { op, seed, hops }` → `runtime.graph_op(op, seed, hops)`; one item per reached object,
  `claim` = `"<name> — <op> of <seed name>"`, `value` = the object id.
- `Compose` → each step in order; a later `Fact`/`Graph` with a `Mention` ref reads a binding an
  earlier `Resolve` made.
- Every item that names an entity pulls that object's **first** evidence fragment (via
  `reconstruct_state`) for `source` / `location` / `confidence`, and the object's own provenance
  property for `extracted_by` (`KirEvidence` carries no extractor field today) — so each claim is
  traceable, the RFC 0118 §7 requirement.

An item cap (`EvidenceSet::truncate_to`, default 60) keeps a hub entity from flooding the set;
truncation emits a diagnostic, same shape as `gather_context`'s `AI003`.

### `AiRuntime` — the new REASON entrypoint

```rust
impl AiRuntime<'_> {
    pub fn plan(&self, question: &str) -> Result<QueryPlan, AiError>;          // understand + plan
    pub fn gather_evidence(&self, question: &str) -> Result<EvidenceSet, AiError>;
    pub async fn reason(&self, question: &str) -> Result<AiAnswer, AiError>;   // gather_evidence → LLM explains + cites
}
```

`reason` builds the prompt from the `EvidenceSet` (a numbered list of `claim` + `location`, not
raw `ObjectState` JSON) and reuses the existing `extract_citations` machinery — a cited id is now
an `EvidenceItem.source`. `ask` is **unchanged in this phase** — wiring `ekos ask` / MCP / EKL
onto `reason` is RFC 0124, so the well-tested `gather_context` path keeps working untouched while
`reason` is proven alongside it.

---

## Non-goals

- No `Compare` node / dated-fact staleness (RFC 0127).
- No LLM planner tier (stub only).
- No change to `ask` / `ask_stream` / MCP / EKL (RFC 0124).
- No new analyzer facts — `execute` works against whatever `properties`/evidence exist today.
- No multi-hop plan optimization / caching — plans are cheap and rebuilt per question.

---

## Verification

- **Planner unit table:** ~12 `(question → QueryType, PlanNode shape, confidence band)` rows
  against a small seeded ledger — lookup, each structural op, each fact-attribute keyword,
  aggregate, conceptual-with-entity, conceptual-without; plus `plan_with(_, Llm)` == the rules plan.
- **Executor units:** `Fact "*"` yields one item per property + name/kind, each carrying the
  seeded `location` and `extracted_by`; `Graph Dependents` on `a<-b<-c` yields `b`,`c`;
  `Compose[Resolve, Graph{Mention}]` binds correctly; the item cap truncates + emits `RSN001`;
  an `Aggregate` plan emits the `RSN005` "use EKL" diagnostic.
- **`reason` e2e (MockLlm):** "what depends on the orders table" → an `EvidenceSet` whose items
  name the FK-dependent tables; the mock's citation of an `EvidenceItem.source` survives
  `extract_citations`.
- Full workspace gate (fmt, clippy `-D warnings`, `cargo test --workspace`).
