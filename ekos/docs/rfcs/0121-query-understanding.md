# RFC 0121 — Query understanding: entity resolution + intent

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-09-01
**Phase 2 of:** RFC 0118 · **builds on:** RFC 0119/0120

---

## Motivation

Before a question can be *routed* (SEARCH vs QUERY vs REASON — RFC 0123) it has to be *understood*:
which concrete entities does it name, and what shape of answer does it want? Today
`AiRuntime::search_for_question` (`crates/runtime/src/ai.rs`) does a keyword strip + an AND→OR→raw
BM25 ladder and nothing else — no entity resolution, no intent. This phase adds that
understanding step, **fully offline** (rules + fuzzy string match, no LLM), as the input the
Query Planner will consume.

---

## Design

New module `crates/runtime/src/retrieval.rs` (the orchestrator's home — RFC 0118 §8). `runtime`
gains a dependency on `ekos-identity` (pure: `kir` + serde, no cycle) for
`similarity::{jaro_winkler, normalize}`.

```rust
pub struct QueryUnderstanding {
    pub raw: String,
    pub query_type: QueryType,
    /// Stopword-stripped significant terms (reuses `ai::extract_search_terms`).
    pub keywords: Vec<String>,
    /// Entity mentions in the query resolved to real objects, best match first.
    pub resolved_entities: Vec<ResolvedEntity>,
    /// Set when `query_type == Structural`.
    pub structural_op: Option<StructuralOp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryType {
    Lookup,       // a bare id / a single exact entity name → fetch, don't search
    Lexical,      // keywords → BM25 primary
    Conceptual,   // an NL question, no dominant entity → semantic + BM25
    Structural,   // "what depends on X", "callers of Y", "what breaks if…" → graph
    Aggregate,    // "how many", "list all … by …" → hand to EKL COUNT/GROUP BY
}

#[derive(Debug, Clone)]
pub struct ResolvedEntity {
    pub mention: String,   // the substring the user typed
    pub id: KirId,
    pub name: String,
    pub kind: Option<ObjectKind>,
    pub confidence: f32,   // 1.0 exact (case-insensitive), else jaro_winkler(normalize, normalize)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralOp { Dependents, Dependencies, Callers, Neighborhood, Impact }
```

### `understand(raw, &Runtime) -> Result<QueryUnderstanding, RuntimeError>`

1. **Mention extraction** — hand-rolled, deterministic (no `regex` dep). Candidates, in priority
   order: single- or double-quoted spans, backtick spans, dotted paths (`Foo.Bar`, `a.b.c`),
   CamelCase tokens (`UserService`), snake/kebab identifiers (`user_service`, `order-items`).
   Overlapping/duplicate spans de-duplicated; a mention is dropped if it is a stopword.
2. **Entity resolution** — for each mention: `runtime.retrieve(RetrievalRequest::lexical(mention))`
   (RFC 0119 seam), take the top few hits, score each by
   `jaro_winkler(normalize(mention), normalize(hit.name))`; an exact case-insensitive name match
   scores `1.0`. Keep the best hit per mention above `RESOLVE_THRESHOLD` (0.82). Result sorted by
   confidence.
3. **Intent classification** — rules over the lowercased raw text, first match wins:
   - looks like a UUID, **or** the whole trimmed query is one exact-resolved entity → `Lookup`
   - starts with `how many` / `count ` / `number of` / `list all` → `Aggregate`
   - contains `depends on` / `dependency of` / `dependencies of` / `what uses` / `used by` →
     `Structural` + `Dependents` (or `Dependencies` for the `... of X` shapes)
   - contains `callers of` / `who calls` / `what calls` → `Structural` + `Callers`
   - contains `what breaks if` / `impact of` / `affected by` → `Structural` + `Impact`
   - contains `related to` / `connected to` / `near` → `Structural` + `Neighborhood`
   - ends with `?` or starts with `how`/`why`/`what`/`when`/`where`/`who`/`explain`/`describe`,
     and no dominant single entity → `Conceptual`
   - otherwise → `Lexical`

`ai::extract_search_terms` + `QUESTION_STOPWORDS` are promoted to `pub(crate)` and reused.

### Not wired anywhere yet

Phase 2 ships `understand` + its types + tests. `AiRuntime` / EKL / MCP keep their current
behaviour; RFC 0123 (the planner) and RFC 0124 (the surface) consume this.

---

## Non-goals

- No LLM classifier (a `[query-planner] planner = "llm"` tier is RFC 0123).
- No routing / execution — `understand` only describes the query.
- No change to any consumer's behaviour.
- Mention extraction is heuristic, not NER — good enough to seed resolution; missed mentions just
  fall back to keyword search downstream.

---

## Verification

- **Unit:** mention extraction over a table of inputs (`what does `authenticate()` return` →
  `authenticate`; `"orders" table` → `orders`; `UserService dependencies` → `UserService`;
  `plausible.billing.subscription` → the dotted path); stopword-only input → no mentions.
- **Unit:** entity resolution against a small `Ledger` — an exact name resolves at `1.0`, a
  near-miss (`user_serivce`) resolves via jaro_winkler, garbage resolves to nothing.
- **Unit:** a ~30-row intent table asserting `QueryType` + `StructuralOp` (rules only).
- **Unit:** `understand("what depends on the orders table", rt)` → `Structural` / `Dependents`,
  `resolved_entities == [orders]`.
- Full workspace gate; `runtime` still builds without any new default feature.
