# Devlog 49 — RFC 0049: Agent Model, and a gap RFC 0048 left behind

**Date:** 2026-08-13
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Third and (for now) final step in the graph-layer continuation of `EKOS_World_Engine_Development_
Plan.md`: RFC 0047 (claims/temporal validity), RFC 0048 (World Model), and now RFC 0049 (Agent
Model) — the next phase in the source document's own recommended order (§44), taken one RFC at a
time per the user's standing direction ("keep building toward the World Engine vision"). The
central design question was whether a simulated agent needs new storage or new primitives at all;
the answer, again, was no — `SimulatedAgent`/`Proposition` are conventions on the existing
`Custom()` escape hatch, beliefs reuse RFC 0047's claim machinery unmodified, and knowledge is a
single new relationship-kind convention (`Custom("Knows")`) feeding directly into RFC 0048's
`build_world`. The one genuinely new code path, `Runtime::agent_observation`, is eleven lines
because everything it needs already existed two RFCs ago. Implementing it also surfaced a real,
previously-latent gap in RFC 0048's own `build_world`: it silently dropped any scope id that
resolved to an event rather than an object, which this RFC's own worked example (agents that
`Know` about events, not just objects) exercised immediately.

---

## RFC 0049 — Agent Model (definitions, beliefs, knowledge, observation)

### Problem / motivation

Same discipline as the prior two RFCs — check what's real before designing around what's assumed
missing:

- `ObjectKind::Agent` already exists in `kir/src/lib.rs`, confirmed by direct read and by grep
  (zero construction sites anywhere) — but its doc comment means something specific and different:
  a *discovered* AI agent definition/config artifact (a LangChain/AutoGen config found in a repo),
  the same category as `ObjectKind::Model`/`Prompt`. Reusing it for a *simulated* agent (a World
  Engine participant with goals and beliefs) would silently conflate two unrelated meanings under
  one taxonomy value. Confirmed before designing, not assumed available.
- Zero prior art anywhere for goals/beliefs/fears/knowledge-as-a-concept — confirmed by grep, only
  false-positive hits on unrelated "Non-goals" RFC section headers.
- RFC 0047 had already named this exact gap as deferred, not silently dropped: its own Non-goals
  section flagged that a claim which isn't naturally an edge between two existing objects (a
  standalone proposition, not a relationship) doesn't fit the claim shape it built — real future
  work if an agent/belief layer ever got built. This RFC is that future work, now due.

### What was built

| Component | Location |
|---|---|
| RFC | `ekos/docs/rfcs/0049-agent-model.md` |
| `SimulatedAgent`/`Proposition` conventions | Doc-comment only — `ObjectKind::Custom("SimulatedAgent"/"Proposition")`, no enum changes |
| Beliefs | Doc-comment only — reuse RFC 0047's claim pattern (`status`/`confidence` properties) unmodified, via `Custom("Trusts")`/`Custom("Believes")` etc. relationship kinds |
| Knowledge | Convention: `RelationshipKind::Custom("Knows")`, agent → object/event, no `status` property (always-confirmed fact, not a claim) |
| `World.events` + `build_world` fix | `runtime/src/lib.rs` — closes a gap in RFC 0048's implementation, found while building this RFC |
| `Runtime::agent_observation` | `runtime/src/lib.rs` — built directly on `build_world`, no duplicated subgraph logic |
| Test fixture | `runtime/tests/graph_layer_fixture.rs` — extended with `Knows` edges from two fixture people to different event subsets, both backends |

**Deliberately not built**, matching the RFC's Non-goals and unchanged scope confirmation: no
memory-type taxonomy (short-term/long-term, belief revision over simulation rounds — needs a
simulation loop that doesn't exist yet); no Decision Engine, Action System, or Simulation Engine;
no `ekos agent create`/`ekos interview` CLI commands; no enforcement of internally-consistent
beliefs (the graph already allows contradictory claims to coexist by design, per RFC 0047).

### The gap RFC 0048 left behind, found by using it for something new

`World`/`build_world` (RFC 0048) only ever called `get_object`/`object_at` for each id in its
scope. Every RFC 0048 test happened to scope worlds to objects only, so this never surfaced. RFC
0049's own worked example — agents that `Know` about *events* they witnessed, not just objects,
matching the source document's §9.3 (Alice sees the theft event directly) — put an event id
straight into a `build_world` scope for the first time, and it silently vanished from the result
instead of erroring or including it.

Fixed by adding `events: Vec<KirEvent>` to `World` and extending `build_world`'s per-id loop: try
`get_object`/`object_at` first (unchanged), and if that resolves to nothing, fall back to
`get_event`, filtered by `occurred_at <= at` when a historical cut was requested (mirroring how the
object path already respects `at`). Covered by a new dedicated test
(`build_world_includes_events_in_scope`) run against the same fixture RFC 0048 used, confirming no
regression to any of the four pre-existing `World`/`build_world` tests. `agent_observation`'s own
tests depend on this fix directly — an agent whose only `Knows` edge points at an event would
otherwise observe an empty world.

This is the same shape of bug RFC 0047 caught in its own first draft (`is_pending_review()`
checking `== "unconfirmed"` instead of `!= "confirmed"`): a piece of code that looked complete
against the tests that existed for it, but had an untested branch that the *next* RFC's own use
case happened to exercise. Caught here by working through RFC 0049's test plan carefully before
writing `agent_observation`, not after a test failed.

### `Runtime::agent_observation`

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

An observation *is* a world — one scoped by the agent's own `Knows` edges instead of a
caller-supplied id list. Zero new subgraph logic; `build_world` (RFC 0048) already does induced-
subgraph computation, unconfirmed-claim exclusion, and now (after the fix above) event resolution.
The source document's own worked example — Alice sees the theft directly, Bob only sees "money
disappeared," Charlie sees "Alice was near the money" — falls straight out of this: each agent's
`Knows` edges differ, so each one's `agent_observation` differs, with no per-agent special-casing
anywhere.

### Decisions (alternatives considered, why this choice)

- **Storing beliefs directly as plain strings in `properties["beliefs"]`** (matching the source
  document's YAML shape literally) — rejected; loses per-belief evidence links and confidence
  scoring, which is the exact machinery RFC 0047 built and this RFC exists to extend to
  propositions, not bypass.
- **Reusing `ObjectKind::Agent` for simulated agents** — rejected outright once its actual meaning
  ("a discovered agent definition artifact") was confirmed by reading the doc comment; a different
  kind string costs nothing and avoids a real, silent semantic collision between "an AI agent found
  in a customer's repo" and "a World Engine simulation participant."
- **A dedicated `Memory` struct now, distinguishing short-term/long-term/incorrect beliefs** —
  rejected for this RFC; belongs with whatever future RFC introduces the simulation loop that
  actually produces the "this round vs. last round" distinction memory tiers require. Building the
  taxonomy before anything populates or reads it repeats the exact mistake this session has
  avoided three RFCs running: designing a shape nothing exercises yet.

---

## Knowledge Captured

- **A fix that closes a gap in RFC N often only becomes necessary — and gets caught — while
  implementing RFC N+1, if N+1's own worked example happens to exercise the untested branch.**
  `World`/`build_world`'s missing event-fallback path existed, silently, through all of RFC 0048's
  own tests; it took RFC 0049's specific need (agents knowing about *events*) to surface it. Worth
  remembering as a reason to keep building one RFC at a time on a shared, extended fixture rather
  than designing several phases upfront — later phases are a real testing mechanism for earlier
  ones, not just consumers of them.
- **When a new relationship convention (`Knows`) is "always confirmed by design, no `status`
  property," still write the excluded-by-unconfirmed-claim test anyway.** `agent_observation`'s
  test suite includes `agent_observation_excludes_unconfirmed_knows_edge` even though the RFC
  itself specifies `Knows` edges never carry a `status` property in normal use — defense in depth
  against a future caller constructing one incorrectly, and free to write since `build_world`
  already has this exclusion logic from RFC 0047/0048.
- **Three RFCs running now, the same architectural discipline has held**: no new `KnowledgeStore`
  trait methods, no new ledger storage, no new top-level KIR primitive types — `Custom()` escape
  hatches plus `properties` conventions plus existing read-model patterns (`ObjectState`/
  `ImpactHop`/`World`) have been sufficient for claims, temporal validity, world projection, and
  now agent definition/belief/knowledge/observation. The genuinely new engineering, if this
  continues, starts at the next fork point: Decision Engine / Action System / Simulation Engine —
  the first phase in this whole continuation with no existing analog anywhere in EKOS to extend.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0049-agent-model.md` | New RFC, all Acceptance Criteria checked, Files Changed table updated to include the `World.events` fix |
| `ekos/crates/runtime/src/lib.rs` | `World` gains `events: Vec<KirEvent>`; `build_world` gains a `get_event` fallback (closes an RFC 0048 gap); `Runtime::agent_observation`; 5 new tests |
| `ekos/crates/runtime/tests/graph_layer_fixture.rs` | Extended with `Knows` edges from two fixture people to different event subsets + asymmetric-observation assertions, both backends; 2 new integration tests |

## Still open (tracked, not silently dropped)

- **Whether to continue further into Decision Engine / Action System / Simulation Engine** — the
  next item in the source document's own development order, and the point at which "keep building
  toward the World Engine vision" will need re-confirming scope again, same as before each of the
  last two RFCs.
- **No dedicated test exercising `Trusts`/`Believes`/`Proposition` conventions end-to-end** — the
  RFC's Design section argues (correctly, per RFC 0047's own generalization) that these need no new
  code since they're existing claim machinery with new convention strings, but no test in this
  session's diff explicitly constructs a `Proposition` + `Believes` claim to prove it beyond
  argument. Worth adding before any future RFC starts depending on the propositional-belief shape
  specifically, rather than assuming untested code paths behave as designed.
- **`World` still has no persistence path** — carried over from RFC 0048, unchanged by this RFC.
- **No query surface for `valid_from`/`valid_until`** — carried over from RFC 0047, unchanged.
