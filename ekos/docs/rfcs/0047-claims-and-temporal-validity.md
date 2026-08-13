# RFC 0047 — Confidence-Scored Claims & Temporal Validity (Graph Layer)

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-13

---

## Motivation

`EKOS_World_Engine_Development_Plan.md` (new, untracked, brought in for evaluation this week)
proposes pivoting EKOS toward a general-purpose knowledge graph + multi-agent simulation platform.
That pivot is explicitly **not** what this RFC is — see the written analysis this session produced
(recorded in the session's plan file) for the full strategic assessment, including two open
questions (product-identity fork, process fit) that are the user's call, not resolved here. What
*was* resolved: if any part of that document's foundation gets built, scope it to the graph layer
only, and do it by extending what EKOS already has rather than the document's own "Phase 0: build
a generic graph from scratch" framing — because most of that foundation already exists.

Direct code research (not assumption) established exactly what's there and what's missing:

**Already real and working:**
- `KirObject`/`KirRelationship` (`kir/src/lib.rs:190-199`, `291-301`) are already generic —
  `properties: HashMap<String, Value>` plus a `Custom(String)` escape hatch on both `ObjectKind`
  and `RelationshipKind` (`:113-114`, `:139-140`), explicitly documented as safe to extend and
  already used for non-engineering kinds (`Person`, `BusinessConcept`, `Model`, `Agent`).
- `KirEvidence` (`:264-270`) already carries `location`, `fragment`, `confidence: f32`.
- Point-in-time reconstruction already exists and is genuinely append-only/multi-version:
  `FactLedger::object_at(id, at)`/`relationships_at(id, at)` (`ledger/src/fact_ledger.rs:356-395`),
  `Runtime::reconstruct_state_at` (`runtime/src/lib.rs:249-273`).
- A real "hypothesis vs. confirmed fact" distinction already exists, narrowly: RFC 0029's
  `KirRelationship::is_pending_review()` (`kir/src/lib.rs:326-329`) returns true only for a
  `Custom("SameAs")` relationship whose `properties["status"] != "confirmed"`, and both graph
  traversal call sites (`Runtime::load_neighborhood` at `runtime/src/lib.rs:101-105`,
  `Runtime::trace_impact` at `:166-168`) already exclude anything `is_pending_review()` flags —
  an unconfirmed candidate is structurally never walked as if it were an observed fact.

**Confirmed missing, by direct grep (zero hits anywhere in `ekos/crates/`):**
- `valid_from`/`valid_until` — no temporal-validity window exists on any KIR type. `object_at`/
  `relationships_at` answer *"what did the ledger know as of T"* (an observation-time cut over
  append history); they do not answer *"was this fact true during T"* (a domain-time validity
  window) — genuinely different questions, both useful, easy to conflate by accident.
- A general-purpose claim concept. `is_pending_review()`'s hypothesis/fact distinction is real but
  hardcoded to one specific relationship kind (`SameAs`) from one specific pass (RFC 0029's
  cross-system identity resolution) — there's no way today to mark an arbitrary
  `Custom("Opposes")`/`Custom("Believes")` relationship as an unconfirmed claim the same way.
- A per-entity full-history query. The ledger's versioned entry log already stores every version
  of every entity (confirmed via `fact_ledger.rs`'s `entity_entries`/`reconstruct_at`), but nothing
  exposes "give me every version of this one entity, oldest to newest" — only a single point-in-time
  cut (`object_at`) or a windowed diff (`diff_ledger`, `ledger/src/lib.rs:1251-1271`).

This RFC closes exactly those three gaps, by extension of existing types, not by adding a new
top-level primitive.

## Scope

1. **`valid_from`/`valid_until` on `KirRelationship`** — additive, optional, backward-compatible
   fields.
2. **Generalize `is_pending_review()`** beyond `Custom("SameAs")` to any relationship kind carrying
   `properties["status"] == "unconfirmed"` — RFC 0029's own behavior is unchanged (a regression
   test pins this), it simply stops being the only caller of the pattern it already established.
3. **`object_history`/`relationship_history`** on `KnowledgeStore` (both backends) and `Runtime` —
   built entirely on the ledger's existing versioned storage, no new storage format.
4. A test fixture matching the source document's own §45 suggestion — 5 people, 3 organizations,
   10 events, 15 relationships, 5 claims — loaded, queried, serialized, and reloaded in both ledger
   backends, proving the extension works end to end without touching anything world/agent/
   simulation-shaped.

## Non-goals

- **No World Model, Agent Model, Decision Engine, Simulation Engine, or Action System** — the
  source document's Phase 3 onward. Confirmed zero prior art for any of it in this codebase
  (exhaustive grep for belief/simulation/multi-agent/world-model returns nothing real); building
  toward it is an explicit, separate decision the user has not made yet.
- **No new top-level `KirClaim` struct.** A "claim" in this RFC is an ordinary `KirObject` or
  `KirRelationship` carrying a `status` property (`"unconfirmed"`/`"confirmed"`/`"disputed"`) plus
  a confidence value — the same shape RFC 0029 already proved out for identity candidates, just no
  longer hardcoded to one kind. A genuine limitation this leaves unsolved, noted honestly: a claim
  that isn't naturally an edge between two existing objects (e.g. "Bob believes the sky is falling,"
  a proposition rather than a typed relationship) doesn't fit this shape — that's real future work
  if the agent/belief layer ever gets built, not solved here.
- **No claim-review MCP tool** (no `ekos_claim_review` mirroring RFC 0029's `ekos_identity_review`).
  Confirming or disputing a claim is already possible today via the existing
  `append_relationship`/`append_object` write path (the ledger already supports appending an
  updated version of an existing entity id) — no new API is required to prove the concept. A
  dedicated review tool is a natural, explicitly-deferred follow-up once this is used for real.
- **No `valid_from`/`valid_until` on `KirObject`.** The source document's own example only shows
  temporal validity on a relationship; adding it to objects too is a natural but unrequested
  extension, deferred to keep this RFC's surface area minimal.

## Design

### `valid_from`/`valid_until` (`ekos/crates/kir/src/lib.rs`)

```rust
pub struct KirRelationship {
    // ...existing fields unchanged...
    #[serde(default)]
    pub valid_from: Option<DateTime<Utc>>,
    #[serde(default)]
    pub valid_until: Option<DateTime<Utc>>,
}
```

`#[serde(default)]` on both fields means every relationship already persisted in a live ledger
deserializes unchanged (`None`/`None` = always valid, today's implicit behavior, made explicit).
A small builder helper (`with_validity(from, until)`), matching the existing `with_property`/
`with_evidence` pattern already on `KirObject`. No query surface changes in this RFC — `valid_from`/
`valid_until` are stored and readable via `properties`-style access today; a `WHERE valid_at <ts>`-shaped
EKL/MCP filter is real follow-up work, deliberately not bundled into this RFC's already-multi-part
scope.

### Generalized claim/hypothesis distinction (`ekos/crates/kir/src/lib.rs`)

`is_pending_review()`'s condition broadens from `matches!(&self.kind, RelationshipKind::Custom(k) if k == "SameAs")`
to any kind at all — the check becomes purely about the `status` property, not the specific kind
that happens to be RFC 0029's:

```rust
pub fn is_pending_review(&self) -> bool {
    match self.properties.get("status").and_then(|v| v.as_str()) {
        None => false,
        Some(status) => status != "confirmed",
    }
}
```

Note this is `!= "confirmed"`, not `== "unconfirmed"` — `ekos_identity_review`'s `decision` is
`"confirmed"` **or** `"rejected"` (`mcp.rs:475`), and a rejected candidate must stay excluded from
traversal exactly like an unconfirmed one (it's still not an observed fact); narrowing the check to
only `"unconfirmed"` would have silently let rejected identity matches leak back into
`load_neighborhood`/`trace_impact` as if they were real edges — caught before implementation by
checking `ekos_identity_review`'s actual valid `decision` values, not assumed. This is a strict
generalization, not a behavior change for existing data: RFC 0029 already always sets
`properties["status"]` on `SameAs` candidates (never absent — `identity.rs:60` always sets
`"unconfirmed"` at creation), so every existing call site (`load_neighborhood`, `trace_impact`)
keeps excluding exactly what it excludes today; a relationship that has never touched this
property (confirmed by grep: nothing except the identity-review path does) falls into the `None`
branch and is unaffected. The doc comment updates to describe the generalized contract (currently
scoped to "cross-system identity candidate," per line 316-321) rather than narrowing readers to
think this is RFC-0029-only forever.

A "claim" going forward is simply: any `KirRelationship` (or `KirObject`, for a standalone
assertion) constructed with `properties["status"] = "unconfirmed"` and a confidence value —
reusing `KirEvidence.confidence` where the claim is evidence-backed, or a `properties["confidence"]`
value directly on the claiming object/relationship where it isn't. No new type, no new storage.

### `object_history`/`relationship_history` (`ekos/crates/ledger/src/lib.rs`, `fact_ledger.rs`, `runtime/src/lib.rs`)

New `KnowledgeStore` trait methods (`ledger/src/lib.rs:1350-1377`'s existing trait, alongside
`object_at`/`relationships_at`):

```rust
fn object_history(&self, id: &KirId) -> Result<Vec<KirObject>, LedgerError>;
fn relationship_history(&self, id: &KirId) -> Result<Vec<KirRelationship>, LedgerError>;
```

`FactLedger`'s implementation is additive over what's already there: `object_at`/`relationships_at`
already call `inner.entity_entries(id.0)` internally to find versions and pick one via a time cut
(`fact_ledger.rs:365`, `385`) — `*_history` is the same lookup without the cut, mapping every entry
to its deserialized object/relationship, oldest to newest. The legacy `Ledger` (SQLite v1/v2)
backend needs the equivalent query against its own version-row storage (confirmed append-only per
RFC 0044's "every version row keeps its rowid," `ledger/src/lib.rs:1014`) — same shape, backend-
specific query. `Runtime` gets thin wrappers (`object_history`/`relationship_history`), mirroring
`reconstruct_state_at`'s existing pattern (`runtime/src/lib.rs:249-273`).

### Test fixture

A new fixture (5 people, 3 organizations, 10 events, 15 relationships, 5 claims — the source
document's own §45 suggestion, reused verbatim since it's a reasonable minimal shape) exercising:
load → query (confirm unconfirmed claims are excluded from `load_neighborhood`/`trace_impact`
regardless of relationship kind, confirm `valid_from`/`valid_until` round-trip) → serialize →
reload, in both ledger backends. Proves the extension without constructing anything
world/agent/simulation-shaped — entities are plain `ObjectKind::Person`/`Custom("Organization")`
objects, events are plain `KirEvent`s, nothing here is a belief, a goal, or a simulation round.

## Alternatives Considered

- **A new top-level `KirClaim` primitive**, a fifth `KirGraph` member alongside
  `objects`/`relationships`/`events`/`evidence` — rejected for this RFC. Bigger surface area
  (new storage column/table in both backends, new `KnowledgeStore` trait methods, new MCP
  tools, new EKL support) than proving the concept requires, and the existing `Custom()` +
  `properties` pattern already generalizes cleanly (RFC 0029 is the proof). Revisit only if a
  future agent/belief layer needs claims that aren't naturally object-to-object edges — noted
  honestly as a real gap this RFC doesn't close, not silently dropped.
- **A dedicated `ekos_claim_review` MCP tool** mirroring RFC 0029's `ekos_identity_review` —
  rejected; the existing append-a-new-version write path already lets a caller flip
  `status: "unconfirmed"` → `"confirmed"` with no new API. A dedicated review UX is real,
  deferred follow-up, not required to prove this RFC's scope works.
- **Storing `valid_from`/`valid_until` in `properties` instead of first-class fields** — rejected;
  the source document explicitly wants temporal validity as a queryable concept, and typed struct
  fields match how `confidence`/`created_at` are already modeled on `KirEvidence` rather than
  living in the untyped properties bag.

## Testing

- `kir` unit tests: `is_pending_review()` still returns `true` for an unconfirmed `Custom("SameAs")`
  relationship (RFC 0029 regression) and now also for an unconfirmed `Custom("Opposes")` (or any
  other kind) carrying the same `status` convention; `valid_from`/`valid_until` serialize/deserialize
  correctly, and a relationship JSON blob predating this RFC (missing both fields) still deserializes
  via `#[serde(default)]`.
- `ledger`/`fact_ledger` unit tests: `object_history`/`relationship_history` return every version of
  an entity across multiple appended updates, oldest to newest, in both backends.
- `runtime` unit tests: `load_neighborhood`/`trace_impact` exclude an unconfirmed claim of an
  arbitrary (non-`SameAs`) kind, matching the existing RFC 0029 exclusion tests' shape.
- Integration: the 5-person/3-org/10-event/15-relationship/5-claim fixture, full
  load → query → serialize → reload cycle, both ledger backends.
- Full workspace: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --check`.

## Acceptance Criteria

- [x] `KirRelationship` gains `valid_from`/`valid_until`; existing serialized ledger data
      deserializes unchanged (verified by
      `relationship_json_predating_rfc_0047_still_deserializes`).
- [x] `is_pending_review()` generalizes beyond `Custom("SameAs")`; RFC 0029's own behavior is
      pinned by a regression test, not just assumed unchanged
      (`is_pending_review_regression_same_as_unconfirmed_and_confirmed`) — and a real bug in the
      first draft of this generalization was caught before merge: narrowing the check to
      `== "unconfirmed"` would have silently let a *rejected* identity candidate leak back into
      traversal, since `ekos_identity_review`'s only two `decision` values are `"confirmed"`/
      `"rejected"`, not just `"unconfirmed"`/`"confirmed"`. Fixed to `!= "confirmed"`, matching
      the original's actual (safer) semantics; covered by
      `is_pending_review_excludes_rejected_not_just_unconfirmed`.
- [x] `object_history`/`relationship_history` implemented on both `KnowledgeStore` backends,
      exposed through `Runtime`. The `FactLedger` (v3) implementation required real design work
      beyond the SQLite backend's simple table scan: facts are stored per-attribute, not as
      whole-object snapshots, so "history" is defined as the object's reconstructed state at every
      distinct `tx` where anything about it changed (`Inner::entity_history`).
- [x] The 5-person/3-org/10-event/15-relationship/5-claim test fixture loads, queries, serializes,
      and reloads correctly in both backends (`crates/runtime/tests/graph_layer_fixture.rs`,
      `fixture_works_on_sqlite_backend` / `fixture_works_on_fact_ledger_backend`).
- [x] No World Model / Agent Model / Simulation Engine code anywhere in this change — confirmed
      out of scope, not partially started.
- [x] Full workspace `cargo build/test/clippy/fmt` clean.

## Files Changed (planned)

| File | Change |
|---|---|
| `ekos/docs/rfcs/0047-claims-and-temporal-validity.md` | This RFC |
| `ekos/crates/kir/src/lib.rs` | `KirRelationship` gains `valid_from`/`valid_until` + `with_validity` builder; `is_pending_review()` generalized beyond `SameAs`, doc comment updated |
| `ekos/crates/ledger/src/lib.rs` | `KnowledgeStore` trait gains `object_history`/`relationship_history`; `Ledger` (SQLite) implementation |
| `ekos/crates/ledger/src/fact_ledger.rs` | `FactLedger` (v3) implementation, built on existing `entity_entries` |
| `ekos/crates/runtime/src/lib.rs` | `Runtime::object_history`/`relationship_history` thin wrappers |
| `ekos/crates/kir/tests/` or `ekos/crates/ledger/tests/` (new) | 5-person/3-org/10-event/15-relationship/5-claim fixture + load/query/serialize/reload test |
