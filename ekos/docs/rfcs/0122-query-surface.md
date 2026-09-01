# RFC 0122 — The QUERY surface: fact lookup + named graph operations

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-09-01
**Phase 3 of:** RFC 0118 · **builds on:** RFC 0119/0121

---

## Motivation

RFC 0118's **QUERY** operation retrieves structured facts and relationships *directly* — no BM25
over chunks, no LLM. Two surfaces, both over data the compiler already produced:

1. **Fact lookup** — `fact(entity, attr)`. "What does `authenticate()` return?" is a property
   read, not a search. Today the only way to get one property is `get_object` + hand-walk
   `properties`.
2. **Named graph operations** — `dependents` / `dependencies` / `callers` / `related`. The
   traversal engine (`Runtime::trace_impact` / `load_neighborhood`, RFC 0018) exists; this phase
   gives it named entry points with sensible default edge-kind sets, so the planner (RFC 0123)
   can route `"what depends on X"` to `dependents(X)` instead of a search.

---

## Design

### Fact lookup — `KnowledgeStore` + `Runtime`

```rust
// KnowledgeStore trait, default impl provided
fn fact(&self, entity: &KirId, attr: &str) -> Result<Option<serde_json::Value>, LedgerError>;
```

Default impl: `get_object(entity)` then resolve `attr`:
- `"name"` → the object name; `"kind"` → the kind's display string;
- anything else → a **dotted path** into `properties` (`"foreign_keys.0.column"` walks
  object → array index → object key).

`Runtime::fact` is a thin wrapper. `Runtime::facts_of(entity) -> Vec<(String, Value)>` returns
the top-level `properties` entries plus `name`/`kind` — the "everything the compiler knows about
this entity" view.

A `FactLedger` fast-path over `FactIndexes` (EAVT prefix scan, no object reconstruction) is a
follow-on optimization — **not** in this phase; correctness first, and the default is already
O(1) object fetch.

### Named graph operations — `Runtime`

```rust
pub fn dependencies(&self, id: &KirId, hops: u32) -> Result<Vec<KirObject>, RuntimeError>;
pub fn dependents(&self,   id: &KirId, hops: u32) -> Result<Vec<KirObject>, RuntimeError>;
pub fn callers(&self,      id: &KirId, hops: u32) -> Result<Vec<KirObject>, RuntimeError>;
pub fn related(&self,      id: &KirId, depth: u32) -> Result<Vec<KirObject>, RuntimeError>;
pub fn graph_op(&self, op: StructuralOp, id: &KirId, hops: u32) -> Result<Vec<KirObject>, RuntimeError>;
```

| op | direction | edge kinds |
|---|---|---|
| `dependencies` | outward (`from == current`) | `DependsOn`, `Calls`, `ForeignKey`, `References`, `Contains` |
| `dependents` / `Impact` | inward (`to == current`) | `DependsOn`, `Calls`, `ForeignKey`, `References` |
| `callers` | inward | `Calls` |
| `related` / `Neighborhood` | both | all (`load_neighborhood`) |

Each returns the reached `KirObject`s, root excluded, first-reached-wins (the `trace_impact`
contract). `graph_op` dispatches `StructuralOp` (RFC 0121) → the right method — the planner's
single entry point.

### Not wired anywhere yet

Phase 3 ships the methods + tests. EKL / MCP surface additions are RFC 0124; the planner that
routes to `fact` / `graph_op` is RFC 0123. A per-`ObjectKind` **fact schema** the analyzers
populate (so `fact(sym, "returns")` is reliably present) is real follow-on work, named here,
deferred — the surface is useful now against whatever `properties` the analyzers already write.

---

## Non-goals

- No `FactIndexes` fast-path (a `FactLedger` optimization for later).
- No `entities_with(attr, value)` (AVET scan) — a later add.
- No analyzer changes / fact schema — advisory, deferred.
- No numeric-range fact queries (a documented `FactIndexes` non-goal, RFC 0016).
- No EKL / MCP / CLI surface (RFC 0124).

---

## Verification

- **Unit:** `fact(id, "name")` / `"kind"` / a scalar property / a nested `"a.b"` path / a missing
  attr → `None`; against both `Ledger` and `FactLedger`.
- **Unit:** `dependencies` / `dependents` / `callers` on a hand-built graph (A calls B, B depends
  on C) — assert direction + edge filtering + hop bound + root exclusion + `pending_review`
  SameAs excluded (inherited from `trace_impact`).
- **Unit:** `graph_op(StructuralOp::Dependents, …)` == `dependents(…)`.
- Full workspace gate.
