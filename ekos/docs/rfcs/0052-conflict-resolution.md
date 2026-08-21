# RFC 0052 — Conflict Resolution

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-13

---

## Motivation

Sixth RFC in the continuation: RFC 0047 (claims/temporal validity), RFC 0048 (World Model), RFC
0049 (Agent Model), RFC 0050 (Decision/Action/Simulation Engine), RFC 0051 (Scenario Definition),
and now Phase 9 of `EKOS_World_Engine_Development_Plan.md` (§14) — Conflict Resolution, the user's
choice over Phase 11 (Virtual Social Environment) or `world.sources` document ingestion, closing a
gap RFC 0050 named twice already: `SimulationConfig` has no way to express priority, resource
scarcity, or a reproducible seed, and `Simulation::run_round`'s "resolve conflicts" step has been a
documented no-op ("nothing here is stochastic, so a seed would have nothing to seed") since RFC
0050 shipped.

Checked before designing, same discipline as the prior five RFCs — and this time the check changed
the scope meaningfully:

- **The source document's own worked example — `Alice → SUPPORT Bob`, `Charlie → OPPOSE Bob` in
  the same round — has nothing to actually resolve in the engine as it exists today.** Traced
  through `execute_action` (RFC 0050): `Support`/`Oppose` produce only an audit
  `Custom("ActionExecuted")` event, no relationship or numeric effect on Bob at all. Two actions
  with no shared mutation target don't conflict; they're just two independent facts, and the
  append-only ledger already has no trouble recording both (RFC 0050's own devlog said this
  explicitly). Building priority/ordering/a seed around an example that doesn't structurally
  produce a collision would be conflict-resolution machinery with nothing real to resolve — the
  exact "designed but not exercised" mistake this session has avoided five RFCs running.
- **A genuine collision requires a *shared*, *scarce* mutation target — not per-actor state.**
  `FormAlliance`'s `Trusts` effect (RFC 0050) is keyed by `(actor, target)`, unique per actor —
  two different actors acting toward the same target never touch the same relationship id, so no
  contention exists there either. RFC 0048's own `Custom("Channel")` convention (a `resources`
  property with a numeric pool, e.g. `capacity`) is the one existing shape in this codebase that's
  genuinely shared across actors — this RFC builds the one real conflict this engine can currently
  produce on top of it, rather than inventing a second one with no grounding.
- **Confidence-based priority already has a field to reuse, not invent.** `Decision.confidence:
  f32` (RFC 0050) exists but has been purely an audit annotation until now — nothing read it. Using
  it to drive execution order costs no new field, matching this continuation's repeated preference
  for reusing what already exists over adding new structure.

## Scope

1. **`SimulationConfig.seed: Option<u64>`** — new field; `None` behaves exactly as RFC 0050 always
   did (fully deterministic, no RNG involved unless a tie needs breaking — see below).
   `load_scenario` (RFC 0051) finally wires `LoadedScenario`'s already-parsed
   `scenario.simulation.seed` into it — the "parses but does nothing" gap named in RFC 0051 closes
   here. `ekos simulate --seed <N>` overrides a scenario's own value.
2. **Priority + ordering, real this time**: `run_round`'s "resolve conflicts" step sorts decisions
   by `Decision.confidence` descending before execution; equal-confidence ties (the *common* case,
   not an edge case — the two reference `DecisionEngine`s each return one of a small handful of
   fixed confidence constants) are broken by a seeded shuffle
   (`StdRng::seed_from_u64(seed.unwrap_or(0).wrapping_add(round as u64))`), reproducible for a given
   `(seed, round)` pair, varying round-to-round even with a fixed seed.
3. **Resource constraints**: every non-`DoNothing` action costs a fixed, uniform amount of an
   `"energy"` resource — deliberately uniform, not per-kind-differentiated (see Alternatives).
   Opt-in: an agent with no `properties["resources"]["energy"]` is unconstrained, so every RFC
   0050/0051 fixture and test keeps working unmodified. Consumption happens at *execution* time
   (not the earlier pre-round validation pass), appending an updated agent object (same id, new
   version — the same "same id = new version" convention `bump_trust` already established).
4. **The one real, shared-resource conflict**: `PostMessage` targeting a `Custom("Channel")` object
   consumes 1 unit of that channel's `properties["resources"]["capacity"]`, also at execution time.
   Two agents `PostMessage`-ing the same channel in the same round both pass pre-round validation
   (neither has seen the other's same-round action, per RFC 0050's own invariant) — whichever
   executes first (by the new priority order) succeeds; whichever executes second, finding capacity
   already exhausted *this round*, fails with a new, distinct failure class
   (`RoundResult.conflict_failures`, parallel to but different from `validation_failures`: it
   passed validation and still didn't happen, because of same-round contention, not a bad decision).
5. **Visibility, "already done"**: `ActionKind::is_public`/`is_private`/`is_self_only` (RFC 0050)
   already covers the source document's "visibility" bullet — reconfirmed, not rebuilt.

## Non-goals

- **No per-action-kind differentiated resource costs.** One uniform `ACTION_ENERGY_COST` for every
  non-`DoNothing` action. Assigning different costs to `PostMessage` vs. `FormAlliance` with no
  concrete scenario driving the specific numbers would be fabricating precision this RFC has no
  basis for — real, deferred work once an actual scenario needs it.
- **No YAML-authorable resource costs** (a scenario defining its own cost table) — would extend RFC
  0051's schema; the engine constant is fixed for this RFC.
- **No richer domain-level conflict rules** beyond the one worked shared-capacity example (e.g.
  "only one alliance per round," mutually exclusive action pairs, priority overrides beyond
  confidence). Nothing in the current action/effect set produces a second kind of real collision to
  resolve yet.
- **No Phase 8 Parallel Agent Execution** — unchanged from RFC 0050's finding; this RFC's
  confidence+seed ordering is still a single-threaded, deterministic sequence, not concurrency.

_Per-action-kind resource costs, YAML-authorable costs, and richer domain conflict rules are
tracked as backlog: see `TODO.md` → "Promoted from RFC Non-Goals" → "World Engine" (Parallel
Agent Execution is the same tracked item as RFC 0050's)._
- **No true randomness anywhere in decision-making itself** — the seed governs only tie-break
  ordering among already-decided actions, never what an agent decides to do. `DecisionEngine`
  implementations stay fully deterministic given their inputs, matching the source document's own
  Design Principle §4.4.

## Design

### `SimulationConfig` and seeded ordering (`crates/simulation/src/simulation.rs`)

```rust
pub struct SimulationConfig {
    pub agents: Vec<KirId>,
    pub available_actions: Vec<ActionKind>,
    pub seed: Option<u64>,
}
```

`run_round`'s step 4 becomes real: decisions are sorted by `confidence` descending (a stable sort,
so a tied group's relative order starts as the original `config.agents` order), then each
contiguous equal-confidence run is shuffled with a `StdRng` seeded from
`seed.unwrap_or(0).wrapping_add(round as u64)` — the same `(seed, round)` pair always produces the
same order; a different seed can (not must, since untied entries are unaffected) produce a
different one. `RoundResult.decisions` reflects this execution order, not the original collection
order — a caller inspecting "what happened, in what order" now sees the real order, not an
implementation artifact.

### Resource constraints and the one real conflict (`crates/simulation/src/simulation.rs`)

```rust
const ACTION_ENERGY_COST: f64 = 0.1;

enum ConsumeResult { Consumed, Insufficient { available: f64 }, NoSuchResource }

fn try_consume_resource(
    store: &dyn KnowledgeStore, entity_id: &KirId, key: &str, amount: f64,
) -> Result<ConsumeResult, SimulationError> { /* opt-in: missing resources[key] => NoSuchResource,
    i.e. unconstrained — never blocks an agent/channel that never declared the resource */ }
```

Reused for two checks inside `execute_action`, both at execution time (after priority ordering, not
during the earlier pre-round validation pass — this is the whole point: validation happens against
a shared pre-round snapshot identical for every agent, so it structurally cannot see same-round
contention; only a check performed *as each action actually executes, in priority order,* can):

1. The actor's own `resources.energy`, for every non-`DoNothing` action.
2. A `PostMessage` target's `resources.capacity`, only when the target is a `Custom("Channel")`
   object.

Either check failing returns a new `ExecutionOutcome::Conflict(ConflictError)` instead of
`ExecutionOutcome::Executed(KirEvent)` — no event, no `Knows` fanout, no ledger mutation beyond
whatever the *successful* consumer of a shared resource already committed. `RoundResult` gains
`conflict_failures: Vec<(KirId, ConflictError)>`, kept distinct from `validation_failures` (a
decision that never should have been attempted, per the pre-round snapshot) because a
conflict-failed decision was entirely reasonable when it was made — it lost a race that didn't
exist yet at validation time.

## Alternatives Considered

- **Per-action-kind resource costs now** — rejected; no concrete scenario in this codebase
  distinguishes "PostMessage should cost less than FormAlliance," and inventing specific numbers
  with no grounding is exactly the premature-precision mistake this session has repeatedly avoided.
- **A true priority *field* on `SimulatedAgent` instead of reusing `Decision.confidence`** —
  rejected; `confidence` already exists, is already per-decision (finer-grained than a static
  per-agent priority would be), and using it costs zero new schema. A dedicated priority field is
  real, deferred work only if confidence turns out to be the wrong signal in practice.
- **Resolving the source document's literal `Alice SUPPORT Bob` / `Charlie OPPOSE Bob` example
  directly** (e.g. giving `Bob` a numeric "standing" both actions adjust) — rejected; a
  commutative numeric adjustment (`+0.1` then `-0.1` vs. `-0.1` then `+0.1`) produces the *same
  final value* regardless of order, so it wouldn't actually exercise order-dependent conflict
  resolution at all — it would look like it did without being a real test of anything. The
  channel-capacity example is smaller than the source document's own illustration but is a genuine,
  order-sensitive collision; the literal example is flagged honestly as *not* actually needing
  conflict resolution given today's effect model, rather than building a fake resolution for it.
- **Marking a conflict-failed decision as a validation failure instead of a new category** —
  rejected; conflating "this was never going to be a reasonable action" with "this was reasonable
  and lost a same-round race" would erase real information a scenario author or analyst would want
  — RFC 0050's own `validation_failures` design already drew this same distinction between
  structural/precondition failures and everything else, and a conflict failure is a third,
  genuinely different case.

## Testing

- `simulation` unit tests: confidence-descending ordering with no ties is exact and seed-independent;
  a tied group is shuffled deterministically for a fixed `(seed, round)` and can differ across two
  different seeds; `try_consume_resource` returns `NoSuchResource` (unconstrained) when the key is
  absent, `Consumed` plus a persisted deduction when sufficient, `Insufficient` (no mutation) when
  not.
- `simulation` integration test: two agents both `PostMessage` a shared `Channel` (capacity 1) in
  one round with distinct confidences (so ordering is unambiguous) — the higher-confidence agent's
  post succeeds (an `ActionExecuted` event exists, `Knows` fanout happened), the lower-confidence
  agent's appears in `conflict_failures`, not `events`; re-running the identical round from a fresh,
  identically-loaded starting ledger with the same seed produces the same winner (determinism
  preserved, extending RFC 0050's own determinism test to cover this new path); every RFC 0050/0051
  fixture/test continues to pass unmodified (resource/capacity checks are opt-in and untouched by
  fixtures that never set an `energy`/`capacity` property).
- Full workspace: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --check`.

## Acceptance Criteria

- [x] `SimulationConfig.seed` implemented; `load_scenario` wires `scenario.simulation.seed` into
      it; `ekos simulate --seed` overrides it — the "parses, does nothing" gap named in RFC 0051 is
      closed, not just documented differently. Verified live: `ekos simulate scenario.yaml --seed
      99` prints `Seed:     99` and runs normally.
- [x] Decision ordering is confidence-descending, with a seeded, reproducible tie-break — verified
      by `order_by_priority_tie_break_is_deterministic_for_same_seed_and_round` (same order for the
      same `(seed, round)`), `order_by_priority_tie_break_can_differ_across_seeds` (different seeds
      can differ), and `seeded_rng_differs_by_round_under_a_fixed_seed`.
- [x] Resource constraints are opt-in (`NoSuchResource` when absent) and every pre-existing RFC
      0050/0051 test continues to pass unmodified — confirmed: all 25 pre-RFC-0052 tests still pass
      with zero changes to their own fixtures (only the two `SimulationConfig` struct literals
      needed a new `seed: None`/`seed` field, a mechanical, not behavioral, change).
- [x] The channel-capacity conflict is real: two same-round `PostMessage`s at one another's
      expense produce exactly one `Executed` and one `Conflict` outcome, not two of either —
      `exactly_one_agent_wins_a_shared_channel_capacity_race`.
- [x] `RoundResult.conflict_failures` is a distinct field from `validation_failures`, not a reused
      or overloaded one.
- [x] No richer conflict-rule machinery, no per-kind cost table, no Phase 8 concurrency anywhere in
      this change — confirmed out of scope, not partially started.
- [x] Full workspace `cargo build/test/clippy/fmt` clean.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0052-conflict-resolution.md` | This RFC, all Acceptance Criteria checked |
| `ekos/crates/simulation/Cargo.toml` | Add `rand` dependency |
| `ekos/crates/simulation/src/simulation.rs` | `SimulationConfig.seed`; seeded priority ordering (`seeded_rng`/`order_by_priority`); `try_consume_resource`/`ConsumeResult`; `ExecutionOutcome`; `ConflictError`; `RoundResult.conflict_failures`; 10 new tests |
| `ekos/crates/simulation/src/scenario.rs` | `load_scenario` wires `scenario.simulation.seed` into `SimulationConfig.seed` |
| `ekos/crates/simulation/src/lib.rs` | `ConflictError` re-exported |
| `ekos/crates/simulation/tests/simulation_fixture.rs` | `SimulationConfig` construction updated for the new `seed` field (mechanical only) |
| `ekos/crates/simulation/tests/conflict_fixture.rs` | New: channel-capacity-race integration test, 3 tests |
| `ekos/crates/cli/src/commands/simulate.rs` | `--seed` override wired through; seed-is-a-no-op notice replaced with an always-shown `Seed:` line; `conflict_failures` printed per round |
| `ekos/crates/cli/src/bin/ekos.rs` | `Commands::Simulate` gains a `seed: Option<u64>` field |
