# RFC 0048 — World Model (a read-only projection, not new storage)

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-13

---

## Motivation

`EKOS_World_Engine_Development_Plan.md`'s own recommended development order (§44) puts "build
world model" immediately after temporal state — which RFC 0047 just shipped (`valid_from`/
`valid_until`, generalized claims, per-entity history). This RFC is the next step in that order,
scoped the same way RFC 0047 was: one phase, additive, built by extending what exists rather than
introducing a parallel storage model. Confirmed with the user: continuing toward the World Engine
vision, one RFC at a time — not the source document's own 46-phase upfront plan.

Direct code research first, same discipline as RFC 0047:

- `KirObject`/`KirRelationship` already model entities and relationships generically (RFC 0047
  made relationships temporally-scoped too). A `KirGraph` is already the container for a bounded
  set of objects/relationships/events/evidence.
- `EventKind` (`kir/src/lib.rs:386-393`) has **no** `Custom(String)` escape hatch — confirmed by
  direct read, unlike `ObjectKind`/`RelationshipKind`, which both do. The source document's example
  events (`ACCUSE`, `POST_MESSAGE`, `SUPPORT`, ...) don't fit any of `Created`/`Modified`/`Deleted`/
  `Migrated`/`Deployed`/`Merged` — a real, small gap, not yet a problem because nothing has tried to
  record a simulation-shaped event before.
- Exhaustive grep for "world clock," "resource pool," "channel" (as a communication venue) across
  every crate: zero hits. Genuinely new territory, as expected — this is the first RFC past the
  graph layer.
- **The key design question this RFC answers**: does "world" need new ledger storage, or is it a
  view over what's already there? EKOS's standing architecture answer is already implicit in how
  everything else works here — the ledger is the one source of truth (append-only), `Runtime` is
  read-only and never duplicates ledger data, and `ObjectState`/`ImpactHop`
  (`runtime/src/lib.rs:24-31`, `43-50`) are exactly this shape already: small, computed, serializable
  read-models built by querying the ledger, not new storage. This RFC's `World` follows the same
  pattern. A world is a **scoped, point-in-time projection** over existing `KirObject`/
  `KirRelationship` data — never a second copy of it.

## Scope

1. **`EventKind::Custom(String)`** — same escape-hatch pattern already proven on `ObjectKind`/
   `RelationshipKind`, so a simulation action can be recorded as a real `KirEvent` instead of
   forcing an ill-fitting existing variant.
2. **A `World` read-model** (`ekos-runtime`, alongside `ObjectState`/`ImpactHop`) — a named,
   time-scoped, entity-scoped view: which objects/relationships are "in" this world as of a given
   time, computed from the ledger, not stored separately.
3. **Resources and channels as conventions, not new primitives** — a documented `properties`
   convention (`KirObject.properties["resources"] = {"influence": 0.8, ...}`) for per-entity
   numeric pools, and `ObjectKind::Custom("Channel")` (already legal today, RFC 0047's own
   generalization of the escape-hatch pattern applies identically here) for a communication venue.
   `World`'s projection reads these conventions out; nothing new to store them.
4. A test fixture reusing RFC 0047's own people/organizations, adding one `Channel` and resource
   properties, proving a `World` can be constructed, scoped, serialized, and reloaded.

## Non-goals

- **No Agent Model, Decision Engine, Action System, or Simulation Engine** — the source document's
  Phase 4 onward. This RFC is world *state*, not agents acting within it. Confirmed as the next,
  separate decision, not started here.
- **No new ledger storage, no new `KnowledgeStore` trait methods.** A `World` is computed entirely
  from existing `Runtime` queries (`load_neighborhood`, `reconstruct_state_at`, `get_object`) —
  if this constraint turns out to be wrong once Agent/Simulation work actually needs to *persist*
  worlds as their own versioned entities (not just point-in-time projections), that's real,
  deferred work for whichever RFC needs it, not assumed here.
- **No `ekos world create`/`ekos simulate` CLI commands.** The source document's §37 CLI design is
  aspirational for the whole initiative; this RFC ships the `World` type and its constructor as a
  library capability other code (a future simulation engine, or direct `ekos_ekl`/MCP use) can
  build on, not a new user-facing command yet.
- **No dedicated `ekos-world` crate**, despite the source document's suggested repository structure
  (§36) naming one. `World` is small enough right now to live in `ekos-runtime` next to
  `ObjectState`/`ImpactHop`, the same kind of read-model struct. A dedicated crate is a natural
  extraction point once Agent Model/Simulation Engine need their own crates anyway — premature to
  split out now for ~150 lines of code.

## Design

### `EventKind::Custom(String)` (`ekos/crates/kir/src/lib.rs`)

```rust
pub enum EventKind {
    Created,
    Modified,
    Deleted,
    Migrated,
    Deployed,
    Merged,
    #[serde(untagged)]
    Custom(String),
}
```

Same shape, same serde attributes, same "safe and low-risk by construction" reasoning already
documented on `ObjectKind` (`kir/src/lib.rs:65-78`) — no code in this workspace exhaustively
matches over `EventKind` today (confirmed by grep), so adding the variant can't silently break an
existing `match`.

### `World` (`ekos/crates/runtime/src/lib.rs`)

```rust
pub struct World {
    pub name: String,
    pub time: DateTime<Utc>,
    pub round: Option<u32>,
    pub objects: Vec<KirObject>,
    pub relationships: Vec<KirRelationship>,
    pub metadata: HashMap<String, serde_json::Value>,
}
```

Constructed via `Runtime::build_world(name, entity_ids: &[KirId], at: Option<DateTime<Utc>>,
round: Option<u32>) -> Result<World, RuntimeError>`: for each id in `entity_ids`, load the object
(current state via `get_object`, or historical via `object_at` when `at` is given — reusing RFC
0047's history machinery, not reimplementing point-in-time logic) and every relationship between
two objects both present in the scope (filtering `relationships_for` results down to edges whose
*other* endpoint is also in `entity_ids` — a world's edges are the subgraph induced by its own
member set, not every edge touching any member). `is_pending_review()` relationships are excluded
by default, same as `load_neighborhood`/`trace_impact` — an unconfirmed claim isn't part of the
world's confirmed state either. `time` defaults to `Utc::now()` when `at` is omitted.

`World` is `Serialize`/`Deserialize` (matching `ObjectState`'s existing derive), so it round-trips
to the source document's own `world.json` shape without new code — the field names above are
already close to §8.1's example; exact renaming to match `time`/`entities`/`relationships`/
`resources` 1:1 is a formatting nicety left to whatever first consumes `World`.

### Resources and channels: conventions, not code

- **Resources**: any `KirObject` may carry `properties["resources"]` as a `{name: number}` map
  (e.g. `{"influence": 0.8, "money": 0.5}`) — read directly off `KirObject.properties` by any
  caller that cares (a future Agent Model consumer), no dedicated accessor needed for this RFC's
  scope. Documented here as the convention so a future RFC doesn't reinvent a different shape.
- **Channels**: `ObjectKind::Custom("Channel")` — a `KirObject` like any other, nameable
  (`"public_forum"`), evidenced, linkable via `RelationshipKind::Custom("PostedTo")` or similar
  between a message-shaped event's subject and the channel object. No new `ObjectKind` variant
  needed; the existing escape hatch already covers this exactly the way it covers `Person`/
  `BusinessConcept` today.

## Alternatives Considered

- **A new `KnowledgeStore`-backed `World` storage table/segment type**, persisting worlds as their
  own versioned entities — rejected for this RFC. Nothing yet needs a world to be more than a
  point-in-time projection; inventing storage for a requirement that doesn't exist yet is the same
  mistake the source document's own "Phase 0: build a generic graph from scratch" made against
  what RFC 0047 found already existed. Revisit if a future Simulation Engine RFC needs to snapshot
  *and later reload* a specific world state as a first-class versioned thing, not just recompute it.
- **A new `ekos-world` crate now** — rejected; ~150 lines doesn't justify a new workspace member
  yet, and `runtime` already hosts comparable read-model structs (`ObjectState`, `ImpactHop`).
- **Storing resources as a dedicated `KirObject` field** (like `properties` is today) instead of a
  documented convention within `properties` — rejected; `properties: HashMap<String, Value>`
  already exists precisely for exactly this kind of extensible, ungoverned data, and adding a
  second, parallel bag (`resources: HashMap<String, f64>`) duplicates it for no real gain at this
  RFC's scope.

## Testing

- `kir` unit tests: `EventKind::Custom` round-trips through JSON; existing named variants unaffected
  (mirrors `object_kind_taxonomy_round_trips`' shape).
- `runtime` unit tests: `build_world` with a small entity scope returns only in-scope
  objects/relationships (an edge to an out-of-scope object is excluded); an unconfirmed claim
  between two in-scope objects is excluded from the world same as `load_neighborhood`; `at` produces
  historical state via the same mechanism `reconstruct_state_at` already uses.
- Integration: extend RFC 0047's own fixture (`runtime/tests/graph_layer_fixture.rs` or a sibling
  file) — build a `World` scoped to a subset of the 5 people + 1 new `Custom("Channel")` object with
  `properties["resources"]` set, confirm it serializes and reloads correctly, in both ledger
  backends.
- Full workspace: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --check`.

## Acceptance Criteria

- [x] `EventKind` gains `Custom(String)`; existing variants and their serialization are unaffected
      (verified: full workspace build has zero exhaustive-match breakage, plus a dedicated round-trip
      test covering every named variant alongside the new one).
- [x] `Runtime::build_world` implemented, returning only the induced subgraph over a given entity
      scope, excluding unconfirmed claims, supporting both current and historical (`at`) state.
- [x] `World` is `Serialize`/`Deserialize` and round-trips through JSON.
- [x] Resources/channels conventions documented (this RFC's Design section); no new storage added
      for either — proven by the fixture's `Custom("Channel")` object with a `properties["resources"]`
      value round-tripping through ordinary object serialization.
- [x] Test fixture proves construction, scoping, and serialize/reload in both ledger backends
      (`world_works_on_sqlite_backend` / `world_works_on_fact_ledger_backend`, extending RFC 0047's
      own fixture rather than duplicating it).
- [x] No Agent Model / Decision Engine / Action System / Simulation Engine code anywhere in this
      change — confirmed out of scope, not partially started.
- [x] Full workspace `cargo build/test/clippy/fmt` clean.

## Files Changed (planned)

| File | Change |
|---|---|
| `ekos/docs/rfcs/0048-world-model.md` | This RFC |
| `ekos/crates/kir/src/lib.rs` | `EventKind` gains `Custom(String)`; test |
| `ekos/crates/runtime/src/lib.rs` | New `World` struct + `Runtime::build_world`; tests |
| `ekos/crates/runtime/tests/` | World-scoped extension to (or sibling of) RFC 0047's fixture |
