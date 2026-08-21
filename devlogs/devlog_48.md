# Devlog 48 — RFC 0048: World Model, staying a projection instead of new storage

**Date:** 2026-08-13
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Direct continuation of RFC 0047 (devlog_47): with the user confirming intent to keep building
toward `EKOS_World_Engine_Development_Plan.md`'s vision — one RFC at a time, not the source
document's own 46-phase upfront plan — this session took the next step in that document's own
recommended development order (§44): World Model, immediately after temporal state. The central
design question was whether "world" needs new ledger storage or is a view over what already
exists; the answer followed directly from EKOS's own standing architecture (append-only ledger as
the one source of truth, `Runtime` as a read-only query layer that never duplicates data) rather
than being decided fresh. `World` ships as a computed projection, the same shape `ObjectState`/
`ImpactHop` already are — no new storage, no new `KnowledgeStore` trait methods.

---

## RFC 0048 — World Model

### Problem / motivation

Same discipline as RFC 0047: check what's real before designing around what's assumed missing.
`EventKind` (`kir/src/lib.rs`) turned out to have no `Custom(String)` escape hatch, unlike
`ObjectKind`/`RelationshipKind` — confirmed by direct read, not assumed from the pattern holding
elsewhere. This matters concretely: none of the source document's example simulation actions
(`ACCUSE`, `POST_MESSAGE`, `SUPPORT`) fit `Created`/`Modified`/`Deleted`/`Migrated`/`Deployed`/
`Merged`. Exhaustive grep for "world clock," "resource pool," "channel" across every crate: zero
hits, as expected for the first RFC past the graph layer.

### What was built

| Component | Location |
|---|---|
| RFC | `ekos/docs/rfcs/0048-world-model.md` |
| `EventKind::Custom(String)` | `kir/src/lib.rs` — same escape-hatch shape as `ObjectKind`/`RelationshipKind` |
| `World` struct | `runtime/src/lib.rs` — alongside `ObjectState`/`ImpactHop`, not a new crate |
| `Runtime::build_world` | `runtime/src/lib.rs` — the induced subgraph over a scoped entity set, current or historical |
| Test fixture | `runtime/tests/graph_layer_fixture.rs` — extended (not duplicated) with a `Custom("Channel")` object and two World-scoped tests, both backends |

**Deliberately not built**, matching the RFC's Non-goals and the user's confirmed scope: no Agent
Model, Decision Engine, Action System, or Simulation Engine (source document's Phase 4 onward); no
new `KnowledgeStore` trait methods or ledger storage for `World` itself; no `ekos world create`/
`ekos simulate` CLI commands; no dedicated `ekos-world` crate.

### The design decision that shaped everything else: World is a view, not a store

The source document's §8 describes a world as "a graph plus state" with its own `entities`/
`relationships`/`resources`/`channels`/`time` fields, which reads naturally as a new stored
structure. Checked against what EKOS already does before accepting that framing: `ObjectState`
(`runtime/src/lib.rs:24-31`) and `ImpactHop` (`:43-50`) are already exactly this shape — small,
`Serialize`-only structs computed on demand by querying the ledger, never persisted as their own
entities. `World` follows the identical pattern: `Runtime::build_world` takes an entity-id scope
and an optional point-in-time cut, and returns the *induced subgraph* — every requested object
that exists, plus every relationship whose *other* endpoint is also in scope (not every edge
touching any member, which would leak the rest of the graph in through the back door). This keeps
one invariant intact throughout the whole session's graph-layer work: the ledger is the one source
of truth; nothing downstream of it maintains a second copy.

Resources and channels followed the same instinct once framed this way: a "resource" is just
`KirObject.properties["resources"]`, a documented convention read by whoever cares, not a new
field. A "channel" is just `ObjectKind::Custom("Channel")` — RFC 0047's own generalization work
last session already made the identity/claims escape hatch generic; this is the same escape hatch,
a different kind string. Neither needed a single line of new storage code — proven directly by the
fixture test, which builds a `Channel` object with resource properties and round-trips it through
nothing but the object serialization every other `KirObject` already goes through.

### Decisions (alternatives considered, why this choice)

- **No `World`-as-versioned-ledger-entity** — rejected for this RFC; nothing yet needs a world to
  be more than a recomputed projection. Inventing storage for a requirement that doesn't exist yet
  is the exact mistake the source document's own "Phase 0: build a generic graph from scratch"
  made against what RFC 0047 found already existed one session ago — not repeating it here.
  Explicitly left open as real, deferred work if a future Simulation Engine RFC needs to snapshot
  *and later reload* a specific world state as a first-class thing, not just recompute it on demand.
- **No dedicated `ekos-world` crate** — ~150 lines doesn't justify a new workspace member yet;
  `runtime` already hosts the same kind of read-model struct. Natural extraction point once
  Agent Model/Simulation Engine need their own crates anyway.
- **Resources as a `properties` convention, not a dedicated `HashMap<String, f64>` field** — a
  second, parallel bag would duplicate what `properties` already exists for, with no real gain at
  this RFC's scope.

---

## Knowledge Captured

- **When a source document's own data model reads naturally as "new storage," check whether the
  target codebase already has a read-model layer that provides the same shape as a computed view
  first** — `ObjectState`/`ImpactHop` were sitting right next to where `World` needed to go, and
  matching their pattern (rather than the source document's literal schema) kept the whole
  extension additive with zero storage risk. The source document's schema was still useful — it's
  what the field names and JSON shape are modeled after — just not a mandate to build new storage
  to match it structurally.
- **"Both variants of a two-option-looking design"** (from RFC 0047, still the operative lesson
  this session): before adding an escape hatch or generalizing a pattern, grep for every real
  caller's actual constraints, not the two most visible states. This session's `EventKind::Custom`
  addition was checked against "does anything exhaustively match over `EventKind`" before landing,
  the same way RFC 0047's `is_pending_review()` generalization was checked against
  `ekos_identity_review`'s actual valid `decision` values (three states, not two) before that RFC's
  design was finalized.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0048-world-model.md` | New RFC, all Acceptance Criteria checked |
| `ekos/crates/kir/src/lib.rs` | `EventKind` gains `Custom(String)`; 1 new test |
| `ekos/crates/runtime/src/lib.rs` | `World` struct + `Runtime::build_world`; 4 new tests |
| `ekos/crates/runtime/tests/graph_layer_fixture.rs` | Extended with a `Channel` object + 2 new World-scoped tests, both backends |

## Still open (tracked, not silently dropped)

- **Whether to continue further into Agent Model** — the next item in the source document's own
  development order, and the next point at which "keep building toward the World Engine vision"
  will need re-confirming scope, the same way this session did before starting.
- **`World` has no persistence path** — by design, for now (see Alternatives Considered). Real,
  deferred work the moment something needs to reload a specific historical world rather than
  recompute it.
- **No query surface for `valid_from`/`valid_until`** — carried over from RFC 0047, still
  unaddressed.
