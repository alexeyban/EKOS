# Devlog 47 — RFC 0047: claims and temporal validity, the graph-layer slice of a much bigger proposal

**Date:** 2026-08-13
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

The user brought in `EKOS_World_Engine_Development_Plan.md` — a 46-phase proposal to pivot EKOS
from an engineering knowledge compiler into a general-purpose knowledge graph + multi-agent
simulation platform. Rather than plan the whole thing, the session split cleanly into two parts.
First, a written strategic analysis (no code): direct code research showed the document's own
"Phase 0" — build a generic, provenance-aware, temporal knowledge graph — mostly already exists in
EKOS's KIR/ledger/runtime, while the World Model/Agent Model/Simulation Engine layers (Phase 3
onward) have zero prior art anywhere in the codebase. Two open questions (product-identity fork,
process fit against this repo's own just-in-time RFC discipline) were flagged as the user's call,
not resolved unilaterally. The user's answer: graph-layer extensions only, nothing world/agent/
simulation-shaped. RFC 0047 scoped and implemented exactly that — three additive extensions to
existing types, no new primitives — and caught a real correctness bug in its own first draft before
it shipped.

---

## RFC 0047 — Claims and Temporal Validity (Graph Layer)

### Problem / motivation

Direct code research (not assumption) against `kir`/`ledger`/`runtime`/`identity` established
precisely what existed and what didn't, before writing a line of RFC text:

- `KirObject`/`KirRelationship` already have a `Custom(String)` escape hatch on both `ObjectKind`
  and `RelationshipKind`, plus a `properties: HashMap<String, Value>` bag — already generic.
- `object_at`/`relationships_at`/`reconstruct_state_at` already give point-in-time reconstruction,
  backed by a genuinely append-only, multi-version ledger.
- RFC 0029's `KirRelationship::is_pending_review()` already proved out a real "hypothesis vs.
  confirmed fact" distinction — but hardcoded to exactly one relationship kind
  (`Custom("SameAs")`), from exactly one pass (cross-system identity resolution).
- Missing, confirmed by grep returning zero hits anywhere in `ekos/crates/`: `valid_from`/
  `valid_until` temporal-validity fields, a general-purpose claim concept usable by any kind (not
  just `SameAs`), and a per-entity full-version-history query (only a single point-in-time cut or a
  windowed diff existed, not "every version of this one entity").

### What was built

| Component | Location |
|---|---|
| RFC | `ekos/docs/rfcs/0047-claims-and-temporal-validity.md` |
| `valid_from`/`valid_until` | `kir/src/lib.rs` — additive `Option<DateTime<Utc>>` fields on `KirRelationship`, `#[serde(default)]`, `with_validity` builder |
| Generalized claim/hypothesis distinction | `kir/src/lib.rs` — `is_pending_review()` broadened from one hardcoded kind to any kind carrying the same `status` convention |
| `object_history`/`relationship_history` | `ledger/src/lib.rs` (trait + SQLite `Ledger`), `ledger/src/fact_ledger.rs` (`FactLedger`/v3), `runtime/src/lib.rs` (thin `Runtime` wrappers) |
| Test fixture | `runtime/tests/graph_layer_fixture.rs` — 5 people, 3 orgs, 10 events, 15 relationships, 5 claims, both ledger backends |

**Deliberately not built**, per the RFC's own Non-goals (matching what the user confirmed): no new
`KirClaim` top-level type — a claim is just an existing `KirObject`/`KirRelationship` with a
`status` property, the same shape RFC 0029 already proved out. No claim-review MCP tool (the
existing append-a-new-version write path already covers confirm/dispute). No `valid_from`/
`valid_until` on `KirObject`. No World Model, Agent Model, Decision Engine, Simulation Engine, or
Action System — the source document's Phase 3 onward, confirmed to have zero prior art and left
that way.

### A real bug caught before merge, not after

The first draft of the generalized `is_pending_review()` was:

```rust
self.properties.get("status").and_then(|v| v.as_str()) == Some("unconfirmed")
```

This looked right against RFC 0029's *initial* state (`identity.rs:60` always sets `status =
"unconfirmed"` when a candidate is created) but was checked against the *review* write path
(`mcp.rs`'s `ekos_identity_review` handler) before implementation, not after: `decision` there is
constrained to `"confirmed"` **or** `"rejected"` — never re-checked against `"unconfirmed"`. A
rejected identity candidate is still not an observed fact and must stay excluded from
`load_neighborhood`/`trace_impact` exactly like an unconfirmed one — but `== "unconfirmed"` would
have silently let it back in, since `"rejected" != "unconfirmed"`. The correct generalization keeps
the original's actual (safer) semantics: `status != "confirmed"`. A dedicated test
(`is_pending_review_excludes_rejected_not_just_unconfirmed`) pins this specifically, and the RFC's
own Design section was corrected to match before any code was written to the wrong spec — caught by
reading the actual valid-value constraints at the real call site, not by assuming the two-state
name ("unconfirmed"/"confirmed") implied there were only two states.

### The `FactLedger` history query needed real design, not just plumbing

The SQLite `Ledger` backend's `object_history`/`relationship_history` is a straightforward
unfiltered scan of the append-only `entries` table by id — every version is already its own row,
so history is "just don't filter by timestamp." `FactLedger` (RFC 0016's fact-segment engine)
stores decomposed per-attribute facts, not whole-object snapshots, so "every version" has no direct
storage analog. The correct definition, worked out from first principles: an entity's full history
is one reconstructed snapshot per **distinct `tx`** at which anything about it changed — the same
`fold_state`/`reconstruct` machinery `object_at` already uses for one point-in-time cut, walked
across every cut where something actually changed rather than just the one the caller asked for.
`Inner::entity_history` implements exactly this, reusing `entity_entries`/`fold_state`/`reconstruct`
unmodified. Noted honestly in the RFC as O(versions × entries) — fine for this RFC's scope (a small
fixture), not optimized for entities with very long histories.

### Decisions (alternatives considered, why this choice)

- **No new `KirClaim` primitive** — rejected; bigger surface area (new storage, new trait methods,
  new MCP tools, new EKL support) than proving the concept requires, and RFC 0029 already proved
  the `Custom()` + `properties["status"]` pattern generalizes cleanly. A genuine limitation left
  unsolved, noted honestly: a claim that isn't naturally an edge between two existing objects (a
  standalone proposition, not a relationship) doesn't fit this shape — real future work only if an
  agent/belief layer ever gets built.
- **No claim-review MCP tool** — rejected; confirming/disputing a claim already works via the
  existing `append_relationship`/`append_object` write path (appending an updated version of an
  existing id). A dedicated review UX is real, deferred follow-up, not required to prove the RFC's
  scope.
- **Direct `entries` table scan for `relationship_history`, bypassing `current_relationships`** —
  the SQLite backend's `relationships_at` has a documented RFC 0011 limitation (filters *current*
  version by timestamp, not true multi-version history, because relationships don't have a per-id
  historical index the way objects do via `object_at`'s direct table scan). `relationship_history`
  sidesteps this entirely by querying `entries` directly by the relationship's own id — the same
  shape `object_at` already uses — so it gets true multi-version history despite RFC 0011's
  standing limitation, for this one id.

---

## Knowledge Captured

- **When generalizing a two-state-looking check (`"unconfirmed"`/`"confirmed"`) into shared code,
  verify every real write path's actual valid values before assuming there are only two states** —
  `ekos_identity_review`'s `decision` parameter has always accepted `"rejected"` too; the original
  `is_pending_review()` handled this correctly (by checking `!= "confirmed"`, not `==
  "unconfirmed"`), and a naive read of the two most-visible states would have silently regressed
  it. Caught by reading `mcp.rs`'s actual validation logic before writing the generalized version,
  not by trusting the field name's apparent semantics.
- **Fact-based (per-attribute) ledger backends and whole-object-snapshot backends need genuinely
  different history-query implementations, not just "the same read, different SQL."** `FactLedger`
  has no stored concept of "a version" the way SQLite's `entries` table does — it has to be derived
  from the set of distinct transaction ids at which any fact about an entity changed, then
  reconstructed per cut. Worth remembering next time a new query needs to cross both backends: the
  two backends' storage models are different enough that "port the SQL" is rarely the right
  translation.
- **A large, ambitious external planning document is often best mined for its one smallest
  defensible next step, not executed phase-by-phase** — the World Engine document's own "Phase
  0"/§45 recommendation ("build a generic graph from scratch") would have meant re-implementing
  things that already existed; the actual gap, once measured against real code, was three small
  additive extensions, not a new subsystem.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0047-claims-and-temporal-validity.md` | New RFC, all Acceptance Criteria checked |
| `ekos/crates/kir/src/lib.rs` | `KirRelationship` gains `valid_from`/`valid_until` + `with_validity`; `is_pending_review()` generalized (corrected to `!= "confirmed"`); 6 new tests |
| `ekos/crates/ledger/src/lib.rs` | `KnowledgeStore` trait + SQLite `Ledger` gain `object_history`/`relationship_history`; `delegate_store!` macro updated; 4 new tests |
| `ekos/crates/ledger/src/fact_ledger.rs` | `Inner::entity_history` + `FactLedger::object_history`/`relationship_history`; 4 new tests |
| `ekos/crates/runtime/src/lib.rs` | `Runtime::object_history`/`relationship_history` thin wrappers; 2 new tests |
| `ekos/crates/runtime/tests/graph_layer_fixture.rs` | New integration test: 5/3/10/15/5 fixture, both backends |
| `ekos/crates/cli/src/commands/commit.rs` | `ckm_rel_to_kir`'s struct-literal `KirRelationship` construction updated for the two new fields |
| `EKOS_World_Engine_Development_Plan.md` | Read and analyzed, not implemented beyond the graph-layer slice — the World Model/Agent Model/Simulation Engine phases remain an open strategic decision, not started |

## Still open (tracked, not silently dropped)

- **The product-identity fork and process-fit questions from this session's earlier analysis** —
  still the user's call, not resolved by implementing the graph layer. RFC 0047 proves the
  foundation is easy to extend; it does not answer whether extending it further (toward World
  Model/Agent Model/Simulation) is a direction worth taking.
- **`valid_from`/`valid_until` has no query surface yet** — stored and round-trippable, but no
  `WHERE valid_at <timestamp>`-shaped EKL/MCP filter. Deliberately deferred, noted in the RFC.
- **A dedicated claim-review tool** (mirroring `ekos_identity_review`) — real, deferred follow-up,
  not needed to prove this RFC's scope.
