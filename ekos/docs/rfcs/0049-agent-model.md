# RFC 0049 — Agent Model (definitions, beliefs, knowledge, observation)

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-13

---

## Motivation

Third RFC in the same continuation: RFC 0047 (graph layer: claims, temporal validity), RFC 0048
(World Model: a read-only projection over a scoped entity set), and now Agent Model — the next
phase in `EKOS_World_Engine_Development_Plan.md`'s own recommended order (§44), per the user's
standing direction to keep building this one RFC at a time.

Checked before designing, same discipline as the last two RFCs:

- **`ObjectKind::Agent` already exists — and already means something else.** Confirmed by direct
  read (`kir/src/lib.rs:110-111`) and by grep (zero construction sites anywhere in the codebase,
  same "taxonomy variant ahead of any connector that emits it yet" status as `Dataset`/`Pipeline`):
  its doc comment is explicit — "an autonomous AI agent **definition**" — i.e. a *discovered*
  software artifact (a LangChain/AutoGen agent config found in a repository), the same kind of
  thing `ObjectKind::Model`/`Prompt` represent. Reusing it for a *simulated* agent (a World Engine
  participant with goals and beliefs) would silently conflate two unrelated meanings under one
  taxonomy value. This RFC introduces a distinct kind rather than overload the existing one.
- **Zero prior art for goals/beliefs/fears/knowledge-as-a-concept** — confirmed by grep, only
  false-positive hits on unrelated "Non-goals" RFC section headers.
- **RFC 0047 already flagged this exact gap as deferred, not silently dropped**: its own Non-goals
  section says a claim that "isn't naturally an edge between two existing objects (e.g. 'Bob
  believes the sky is falling,' a proposition rather than a typed relationship) doesn't fit this
  shape — that's real future work if the agent/belief layer ever gets built." This RFC is that
  future work, now due.

## Scope

1. **A distinct simulated-agent kind**: `ObjectKind::Custom("SimulatedAgent")` — not the existing
   `ObjectKind::Agent`. Role, goals, and fears as documented `properties` conventions (RFC 0048's
   "resources" pattern extended, not new fields).
2. **Beliefs about existing entities** — reuses RFC 0047's claim shape exactly: a
   `RelationshipKind::Custom("Trusts")`/`Custom("Distrusts")`/etc. relationship from agent to
   target, `status: "unconfirmed"` + `confidence`. No new mechanism.
3. **Free-form propositional beliefs** — RFC 0047's real, named limitation, closed here: a belief
   that isn't about an existing entity gets its content reified as a
   `ObjectKind::Custom("Proposition")` object (e.g. name `"bob_wants_to_replace_me"`), then an
   ordinary claim relationship (`Custom("Believes")`) from the agent to that proposition — same
   confidence/status/evidence machinery, now covering the case RFC 0047 couldn't.
4. **Knowledge** — `RelationshipKind::Custom("Knows")` from agent to whatever object/event it has
   access to. A confirmed fact (no `status` property), not a claim — an agent either knows about
   something or it doesn't; the *content* of what it believes about that thing is a separate claim.
5. **`Runtime::agent_observation`** — `agent.observe(world)` from the source document's §9.3,
   built directly on RFC 0048's `build_world`: an agent's observation is the world scoped to
   exactly what it has a `Knows` edge to, plus itself.

## Non-goals

- **No memory-type taxonomy** (short-term vs. long-term, "incorrect beliefs" tracked as distinct
  from correct ones, memory decay). The source document's §9.2 lists these; they're real, but they
  require reasoning about simulation rounds and belief revision over time, which belongs with the
  Decision Engine/Simulation Engine phases (source document's Phase 5+), not agent *definition*.
  Scoping this RFC to what an agent *is* and *knows right now*, not how its knowledge changes
  through a simulation loop that doesn't exist yet.
  _Tracked as backlog: see `TODO.md` → "Promoted from RFC Non-Goals" → "World Engine". (The
  Decision/Simulation Engine phases this was deferred to now exist, RFC 0050-0055 — the taxonomy
  itself still doesn't.)_
- **No Decision Engine, Action System, or Simulation Engine** — unchanged from RFC 0048's
  Non-goals, still the next fork point after this one, still requiring its own scope confirmation.
- **No `ekos agent create`/`ekos interview` CLI commands** — library capability only, same posture
  RFC 0048 took for `World`.
- **No enforcement that an agent's beliefs are internally consistent** (e.g. simultaneously
  trusting and distrusting the same target). The graph already allows contradictory claims to
  coexist (RFC 0047's whole point — hypotheses, not merged truth) — an agent holding a contradiction
  is itself potentially interesting simulation data, not an error to prevent.

## Design

### `SimulatedAgent` (`ekos/crates/kir/src/lib.rs`, doc-comment convention only — no enum change needed)

`ObjectKind::Custom("SimulatedAgent")` — the existing escape hatch, a new string value. Properties
convention:

```json
{
  "role": "founder",
  "goals": ["retain_control"],
  "fears": ["public_scandal"],
  "resources": { "influence": 0.8, "money": 0.5, "information": 0.9 }
}
```

`resources` is exactly RFC 0048's existing convention, reused verbatim — an agent is a `KirObject`
like any other, so anything already true of objects (evidence links, temporal history via
`object_history`) applies to agents for free. `goals`/`fears` are plain string lists: internal
desires/anxieties, not truth claims, so they don't need RFC 0047's confidence/status machinery —
deliberately simpler than beliefs for that reason, not an oversight.

### Beliefs (`ekos/crates/kir/src/lib.rs`, doc-comment convention only)

Two shapes, both reusing RFC 0047's claim pattern (`status: "unconfirmed"` + `confidence`)
unmodified:

- **About an existing entity**: `agent --Custom("Trusts")--> target`, `properties: {"status":
  "unconfirmed", "confidence": 0.3, "value": -0.7}` (the source document's per-relationship trust
  score rides in `properties["value"]`, alongside the claim machinery, not instead of it).
- **A free-form proposition**: reify the content as `ObjectKind::Custom("Proposition")` (e.g.
  `KirObject::new("bob_wants_to_replace_me", ObjectKind::Custom("Proposition".to_string()))`), then
  `agent --Custom("Believes")--> proposition`, same `status`/`confidence` properties. This closes
  RFC 0047's own named gap: a claim no longer has to be about two entities that already exist for
  independent reasons — the proposition object exists *because* an agent believes it, which is a
  legitimate, evidence-linkable `KirObject` in its own right (an agent's belief can itself carry
  `KirEvidence` — "Alice believes this because she saw event X").

### Knowledge and observation (`ekos/crates/runtime/src/lib.rs`)

`RelationshipKind::Custom("Knows")` from agent to object/event, no `status` property (a confirmed
fact: the agent has this in scope, whether or not it believes any particular claim about it).

```rust
pub fn agent_observation(
    &self,
    agent_id: &KirId,
    at: Option<DateTime<Utc>>,
) -> Result<World, RuntimeError> {
    let rels = match at {
        Some(t) => self.ledger.relationships_at(agent_id, t)?,
        None => self.ledger.relationships_for(agent_id)?,
    };
    let mut scope = vec![*agent_id];
    scope.extend(
        rels.iter()
            .filter(|r| {
                r.from == *agent_id
                    && matches!(&r.kind, RelationshipKind::Custom(k) if k == "Knows")
            })
            .map(|r| r.to),
    );
    self.build_world(format!("{agent_id}-observation"), &scope, at, None)
}
```

Directly reuses `build_world` (RFC 0048) — an observation *is* a world, just one scoped by the
agent's own `Knows` edges instead of a caller-supplied id list. No duplication of `build_world`'s
induced-subgraph logic. The source document's own worked example (§9.3 — Alice sees the theft
directly, Bob only sees "money disappeared," Charlie sees "Alice was near the money") falls
straight out of this: each agent's `Knows` edges differ, so each one's `agent_observation` differs,
without any special-casing per agent.

## Alternatives Considered

- **Storing beliefs directly in `properties["beliefs"]` as plain strings** (matching the source
  document's YAML shape literally) — rejected; loses per-belief evidence links and confidence
  scoring, the exact machinery RFC 0047 built and this RFC's whole point is to extend to
  propositions, not bypass.
- **Reusing `ObjectKind::Agent` for simulated agents** — rejected outright once its actual meaning
  ("a discovered agent definition artifact") was confirmed by reading the doc comment; a different
  kind string costs nothing and avoids a real, silent semantic collision.
- **A dedicated `Memory` struct distinguishing short-term/long-term/incorrect beliefs now** —
  rejected for this RFC; belongs with whatever RFC introduces the simulation loop that actually
  produces the "this round vs. last round" distinction memory tiers require. Building the taxonomy
  before anything populates or reads it risks the same "designed a shape nothing exercises yet"
  mistake this session has repeatedly avoided by checking real constraints before generalizing.

## Testing

- `kir`/`runtime` unit tests: a `Custom("SimulatedAgent")` object with `goals`/`fears`/`resources`
  properties round-trips; a `Trusts` claim relationship and a reified `Proposition` +
  `Believes` claim both work through the existing claim machinery unmodified (no new code path,
  just new convention values — the test is really confirming RFC 0047's machinery is genuinely
  kind-agnostic, not special-cased to the kinds it happened to ship with).
- `runtime` unit tests for `agent_observation`: an agent with `Knows` edges to a subset of objects
  observes exactly that subset (plus itself), not the whole graph; an unconfirmed `Knows` edge (if
  one existed) would be excluded the same way any other claim is — though `Knows` is specified
  as always-confirmed by convention, the exclusion path is exercised for defense in depth.
- Integration: extend RFC 0047/0048's fixture again — give two of the five people `Knows` edges to
  different subsets of the fixture's events, confirm `agent_observation` returns different worlds
  per agent (the source document's Alice/Bob/Charlie asymmetric-observation example, using the
  fixture's own real people instead of inventing new ones), both ledger backends.
- Full workspace: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --check`.

## Acceptance Criteria

- [x] `SimulatedAgent`/`Proposition` conventions documented; no `ObjectKind`/`RelationshipKind` enum
      changes required (both escape hatches already exist) — confirmed no enum edits were needed
      for either kind; only doc-comment conventions.
- [x] `Runtime::agent_observation` implemented, reusing `build_world` directly, no duplicated
      subgraph logic — implemented exactly as designed above, in `runtime/src/lib.rs`.
- [x] Two agents with different `Knows` edges produce different observations from the same
      underlying graph, verified by the fixture extension — `assert_agent_observation_behaves()`
      in `graph_layer_fixture.rs`, both backends.
- [x] No Decision Engine / Action System / Simulation Engine code anywhere in this change —
      confirmed out of scope, not partially started.
- [x] Full workspace `cargo build/test/clippy/fmt` clean — verified after the fixture extension
      landed; zero warnings, zero failures.

A real gap in RFC 0048's `build_world`, found while implementing this RFC's `agent_observation`
(not part of the original plan above): `World`/`build_world` only ever tried `get_object` for
scope ids, silently dropping any id that was actually an event. Since this RFC's own worked
example (agents knowing about events, not just objects) exercises exactly that path, it surfaced
immediately rather than staying latent. Fixed by adding `events: Vec<KirEvent>` to `World` and
extending `build_world` with a `get_event` fallback, filtered by `occurred_at <= at` when
historical. Covered by a new dedicated test (`build_world_includes_events_in_scope`) plus the
`agent_observation` tests, which depend on it.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0049-agent-model.md` | This RFC |
| `ekos/crates/runtime/src/lib.rs` | `World.events` field + `build_world` event fallback (RFC 0048 gap found here); `Runtime::agent_observation`; 5 new tests |
| `ekos/crates/runtime/tests/graph_layer_fixture.rs` | Extended with `Knows` edges + asymmetric-observation test, both backends; 2 new integration tests |
