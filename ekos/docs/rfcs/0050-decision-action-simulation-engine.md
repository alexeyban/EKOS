# RFC 0050 — Decision Engine, Action System, Simulation Engine

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-13

---

## Motivation

Fourth RFC in the same continuation: RFC 0047 (claims/temporal validity), RFC 0048 (World Model),
RFC 0049 (Agent Model), and now the next fork in `EKOS_World_Engine_Development_Plan.md`'s own
recommended order (§44) — Phases 5-7: Agent Decision Engine, Action System, Simulation Engine. The
user picked this explicitly (over a smaller Action-System-only slice, a Decision-Engine-only slice,
or stopping at RFC 0049) after being told directly that this is qualitatively different from the
last three RFCs: RFC 0047-0049 were each small, additive extensions to things that already existed
(`Custom()` escape hatches, `properties` conventions, existing read-model patterns). This RFC is the
first with **zero prior art anywhere in EKOS** — there is no existing "an agent chooses an action"
or "advance a round" concept to extend. Both `devlog_48.md` and `devlog_49.md` flagged this
specific fork as needing its own scope confirmation before starting; this RFC is that confirmation
having happened.

Checked before designing, same discipline as the last three RFCs:

- `Runtime` is documented and enforced as read-only (`runtime/src/lib.rs`'s own module doc: "The
  Runtime has no `&mut self` methods that affect the ledger (RFC 0005)"). A simulation round
  **writes** — new events every round, occasionally new/updated relationships. This RFC's engine
  therefore does not go through `Runtime` for writes, only for reads (`agent_observation`,
  `build_world`); writes go through `&dyn KnowledgeStore` directly, the same access level
  `commit.rs` (a compiler-pass write path) already has. `Runtime`'s read-only invariant is
  unchanged by this RFC — the simulation engine is a new *kind* of write path (like `commit.rs`),
  not an exception carved into `Runtime`.
- `KirEvent.subject: KirId` is a single id, `payload: serde_json::Value` is free-form — confirmed
  by direct read (`kir/src/lib.rs:374-382`) before assuming an event could carry an actor/target
  pair structurally; it can't, so actor/target/content live in `payload`, matching how every other
  event producer in the codebase already uses `payload` for structured, non-schema-fixed data.
- `EventKind::Custom(String)` (RFC 0048) and `RelationshipKind::Custom(String)` (pre-existing) are
  reused for `Custom("ActionExecuted")` and `Custom("Trusts")` respectively — no enum changes
  needed, continuing the pattern all three prior RFCs established.
- `KirRelationship::new`/`append_relationship` with the **same relationship id** is how an existing
  relationship gets a new version (confirmed by reading `Ledger::append_relationship`,
  `ledger/src/lib.rs:545-567` — `written_at` is the ledger's own append timestamp; `rel.id` is what
  ties versions together, exactly what RFC 0047's `relationship_history` already walks). No new
  "update" mechanism needed — appending with the same id already means "new version of this fact."

## Scope

1. **A finite action vocabulary** (source document §11, Phase 6) — `ActionKind`, one of the 12
   named in the source document (`PostMessage`, `SendMessage`, `Support`, `Oppose`,
   `ShareInformation`, `WithholdInformation`, `FormAlliance`, `BreakAlliance`,
   `RequestInformation`, `ChangeGoal`, `ChangeBelief`, `DoNothing`), plus a target/content payload
   and a fixed public/private visibility per kind.
2. **Validation** — structural (does this action kind need a target, and is the target actually in
   the acting agent's own observation) plus exactly one real precondition, the source document's
   own worked example (§11: `FORM_ALLIANCE` requires `trust > 0.4`). Richer per-kind preconditions
   are named, not built (see Non-goals).
3. **A provider-independent decision contract** (source document §10, Phase 5) — a `DecisionEngine`
   trait taking an agent's own observation (`World`, from RFC 0049's `agent_observation`) plus its
   available actions, returning a `Decision` (action + concise, auditable `reasoning_summary` +
   `confidence` — explicitly not hidden chain-of-thought, matching the source document's own
   instruction). Two deterministic reference implementations ship (`AlwaysDoNothing`,
   `RuleBasedAgent`); no LLM-backed implementation in this RFC (see Non-goals).
4. **A round-based simulation loop** (source document §12, Phase 7) — observe (every agent, from
   the same pre-round cut) → decide (every agent, from that same cut) → validate → execute (append
   events, and for `FormAlliance` only, an updated `Trusts` relationship) → update agent memories
   (`Knows` edges to newly created events, fanned out per the acting action's visibility). No world
   mutation step of its own — `World` (RFC 0048) is a projection, so "update world" is free: the
   next round's `agent_observation` re-queries the ledger and sees everything the previous round
   wrote.
5. **Minimal, deterministic conflict handling** — a fixed, caller-supplied agent processing order
   (not simultaneous, not randomized) is the only conflict-resolution mechanism. Sufficient for
   reproducibility without a `--seed` (see Non-goals for why real Phase 9 conflict rules are
   deferred).

## Non-goals

- **No Phase 9 Conflict Resolution** (priority rules, resource constraints, `--seed`-based
  randomness). This RFC's rounds are already fully deterministic from fixed processing order alone
  — two agents acting on the same relationship in one round simply produce two ledger versions in
  a well-defined order, both preserved (the ledger is append-only; nothing is lost, `relationship_
  history` already shows both). Real arbitration rules (e.g., "higher-trust action wins," resource
  costs that can run out) are deferred until a scenario actually needs them — building that
  machinery before anything exercises it repeats the mistake this session has avoided three times
  running.
- **No Phase 8 Parallel Agent Execution.** The `observe-then-decide-then-execute` ordering already
  matches the source document's own stated goal for Phase 8 ("avoids order-dependent behavior" by
  collecting all decisions from one shared pre-round cut before executing any of them) — true
  concurrent/parallel execution is a performance optimization over the same semantics, not a
  behavior change, and isn't needed to prove the loop.
- **No LLM-backed `DecisionEngine`.** The trait is deliberately provider-independent (mirroring
  `LlmProvider`'s shape from RFC 0021/0046), but wiring an actual `LocalLLMAgent`/`CloudLLMAgent`
  means real prompt design (what does "reasoning_summary" look like from a live model, how is
  `available_actions` presented, how is the result parsed back into a `Decision`) — a separate,
  focused RFC once the deterministic loop itself is proven, not bundled into the RFC that proves
  the loop.
- **Per-kind action effects beyond the one worked `FormAlliance` example.** Every executed action
  produces a `Custom("ActionExecuted")` event (audit trail, uniform across all 12 kinds) and fans
  out `Knows` edges per visibility — that's the real, load-bearing effect this RFC needs to prove
  the loop end-to-end. Whether `ChangeGoal` should mutate the agent's own `properties["goals"]`,
  whether `Oppose` should produce a `Custom("Opposes")` relationship symmetric to `FormAlliance`'s
  `Trusts` handling, etc. — real, named, deferred work; building bespoke effect semantics for 11
  action kinds with no concrete scenario driving their shape yet risks the same
  "designed-not-exercised" mistake RFC 0048/0049's own Alternatives Considered sections already
  called out and avoided.
- **No Phase 10+ work** (Scenario Definition, Virtual Social Environment, Event Store as a distinct
  concept, Replay, Metrics, Turning Point Detection, Report Generation, Monte Carlo, Counterfactuals,
  Web UI, Video Generation) — all downstream of a working simulation loop, all their own scope
  decisions later.
- **No `ekos simulate` CLI command.** Library capability only, same posture RFC 0048/0049 took.
- **No new workspace-level `KnowledgeStore` trait methods.** Every write this RFC needs
  (`append_event`, `append_relationship`) already exists.

## Design

### Why a new crate, not another extension to `runtime`

RFC 0048's own devlog named this explicitly: *"~150 lines doesn't justify a new workspace member
yet... natural extraction point once Agent Model/Simulation Engine need their own crates anyway."*
This is that point — `runtime` is documented as read-only end to end; a write-capable engine with
its own vocabulary (actions), its own contract (decisions), and its own loop (rounds) is a
different kind of thing, not another read-model struct alongside `ObjectState`/`World`. New crate:
`ekos-simulation` (`crates/simulation`), depending on `ekos-kir`, `ekos-ledger` (for
`KnowledgeStore`, writes), and `ekos-runtime` (for `Runtime::agent_observation`/`build_world`,
reads) — no new dependency on anything outside what RFC 0047-0049 already established.

### Action vocabulary (`crates/simulation/src/action.rs`)

```rust
pub enum ActionKind {
    PostMessage, SendMessage, Support, Oppose, ShareInformation, WithholdInformation,
    FormAlliance, BreakAlliance, RequestInformation, ChangeGoal, ChangeBelief, DoNothing,
}

pub struct Action {
    pub kind: ActionKind,
    pub target: Option<KirId>,
    pub content: Option<String>,
}
```

`ActionKind::requires_target()` — false only for `DoNothing`. `ActionKind::is_public()` — a fixed
table, documented per variant: broadcast/observable-by-every-participant (`PostMessage`, `Support`,
`Oppose`, `FormAlliance`, `BreakAlliance`) vs. targeted/private (`SendMessage`,
`ShareInformation`, `WithholdInformation`, `RequestInformation`) vs. self-only
(`ChangeGoal`, `ChangeBelief`, `DoNothing` — these affect no one but the actor, so "visibility"
doesn't fan out `Knows` edges to anyone else).

### Validation (`crates/simulation/src/action.rs`)

```rust
pub fn validate_action(
    actor_id: &KirId,
    action: &Action,
    observation: &World,
) -> Result<(), ValidationError>
```

Structural: a target-requiring action with no target, or a target not present in the acting
agent's own `observation` (objects or events), is rejected — an agent cannot act on what it hasn't
observed. Precondition: `FormAlliance` requires an existing `Custom("Trusts")` relationship from
actor to target in `observation.relationships` with `properties["value"] > 0.4` (the source
document's own worked example, §11). A rejected action is not silently dropped — the simulation
loop records the validation failure and substitutes `DoNothing`, both facts (attempted action,
rejection reason) captured in the round result for audit.

### Decision contract (`crates/simulation/src/decision.rs`)

```rust
pub struct DecisionContext<'a> {
    pub agent_id: KirId,
    pub observation: &'a World,
    pub available_actions: &'a [ActionKind],
}

pub struct Decision {
    pub action: Action,
    pub reasoning_summary: String,
    pub confidence: f32,
}

pub trait DecisionEngine {
    fn decide(&self, ctx: &DecisionContext) -> Decision;
}
```

`reasoning_summary` is a plain string, never a hidden reasoning trace — matches the source
document's own instruction (§10) not to store chain-of-thought, and this project's own RFC 0019/
0021 pattern of treating LLM output as auditable text, never opaque internal state. Two reference
implementations ship, both deterministic (no network/model call), proving the contract without
requiring an LLM:

- `AlwaysDoNothing` — the trivial baseline; always returns `DoNothing`, confidence `1.0`.
- `RuleBasedAgent` — reads the acting agent's own `goals` property (RFC 0049's convention) off its
  object in `observation`; understands one additional, documented sub-convention this reference
  engine alone interprets (not a `SimulatedAgent` schema requirement): a goal string of the shape
  `"support:<name>"`/`"oppose:<name>"` names another object in the observation by its `name` field,
  and produces the matching `Support`/`Oppose` decision targeting it; falls back to `DoNothing` if
  no goal matches anything currently observed.

### Simulation loop (`crates/simulation/src/simulation.rs`)

```rust
pub struct SimulationConfig {
    pub agents: Vec<KirId>,               // processing order — deterministic by construction
    pub available_actions: Vec<ActionKind>,
}

pub struct Simulation<'a> {
    store: &'a dyn KnowledgeStore,
    config: SimulationConfig,
    engines: HashMap<KirId, Box<dyn DecisionEngine>>,
}

impl<'a> Simulation<'a> {
    pub fn run_round(&self, round: u32) -> Result<RoundResult, SimulationError> { /* ... */ }
    pub fn run(&self, rounds: u32) -> Result<Vec<RoundResult>, SimulationError> { /* ... */ }
}
```

`run_round`, matching the source document's own recommended execution model (§12) and its explicit
warning ("do not immediately mutate the world after Agent A acts before Agent B has observed the
same round"):

1. **Observe** — one timestamp captured at round start; every configured agent's
   `Runtime::agent_observation(agent_id, Some(round_started_at))` computed from that single cut,
   before any round-N write happens.
2. **Decide** — every agent's `DecisionEngine::decide` called against its own step-1 observation.
   All decisions collected before any execution starts — no agent's decision can see another
   agent's round-N action, satisfying the source document's warning directly.
3. **Validate** — each decision's action checked against the same step-1 observation; failures
   substitute `DoNothing` and are recorded, not discarded.
4. **Resolve conflicts** — no-op beyond the fixed `config.agents` order (see Non-goals).
5. **Execute** — for each agent, in order: append a `Custom("ActionExecuted")` event (`subject` =
   actor, `payload` = `{actor, kind, target, content, round, reasoning_summary, confidence}`); for
   `FormAlliance` specifically, additionally re-append the validated `Trusts` relationship (same
   `id`, `value` incremented by `0.2` clamped to `1.0` — the source document's own worked effect).
6. **Persist** — implicit in step 5; every `append_event`/`append_relationship` call writes
   immediately, no batching.
7. **Update world** — nothing to do; `World` is a projection (RFC 0048), so the next round's
   `agent_observation` calls already see everything step 5 wrote.
8. **Update agent memories** — for each event created in step 5, fan out `Custom("Knows")`
   relationships from observers to that event: every configured agent for a public action; actor +
   target only for a private one; actor only for a self-only one.

## Alternatives Considered

- **A pluggable action registry (closures/trait objects per `ActionKind`)** instead of a `match` in
  `validate_action`/`execute_action` — rejected; the vocabulary is a fixed, closed set of 12 named
  in the source document itself, not something callers extend. A registry would be premature
  abstraction over a shape that doesn't vary yet, the same call RFC 0049 made for
  `goals`/`fears` staying plain string lists instead of a typed schema.
- **A YAML/DSL precondition and effect language** (matching the source document's own illustrative
  YAML block in §11 literally) — rejected for this RFC; one real Rust-native precondition
  (`FormAlliance`'s trust check) proves the validation contract works without inventing and parsing
  a rule language nothing else in EKOS has. Real future work if Phase 10 (Scenario Definition) ever
  needs scenario authors to define new preconditions without recompiling.
- **Giving the Simulation Engine its own `&mut Runtime`-style write handle** — rejected; `Runtime`
  is documented read-only end to end (RFC 0005), and blurring that for one caller reopens a
  question this codebase settled on purpose. Writing through `&dyn KnowledgeStore` directly (same
  access level `commit.rs` already has) keeps the invariant intact and costs nothing — the engine
  already needs `Runtime` for reads (`agent_observation`) and can hold both references side by side.
- **A random seed for round-to-round variation, per the source document's Phase 9 `--seed`
  mention** — rejected for this RFC; nothing here is stochastic (fixed processing order is fully
  deterministic on its own), so a seed with nothing to seed would be dead configuration. Real when
  Phase 9's actual conflict/priority rules land and need reproducible tie-breaking.

## Testing

- `simulation` unit tests: `ActionKind::requires_target`/`is_public` cover all 12 variants;
  `validate_action` rejects a missing target, a target outside the observation, and a `FormAlliance`
  with insufficient trust, and accepts a `FormAlliance` with trust `> 0.4`; `AlwaysDoNothing` and
  `RuleBasedAgent` each produce the expected `Decision` from a hand-built `DecisionContext`.
- `simulation` integration test (`crates/simulation/tests/simulation_fixture.rs` — a new, compact,
  self-contained fixture, not a `graph_layer_fixture.rs` extension; see the Acceptance Criteria
  section for why): Alice trusts Bob at `0.5` (clearing `FormAlliance`'s precondition) and always
  proposes `FormAlliance`; Bob has an `"oppose:Acme"` goal; Charlie takes no action. One round:
  assert all three decisions match expectations, the round produced the expected `ActionExecuted`
  events, Alice's `Trusts` relationship toward Bob increased from `0.5` to `0.7`, and Charlie (who
  acted on nothing) still gained `Knows` edges to all three of the round's events via public
  fanout — both ledger backends.
- Determinism check: running the same round twice from the same starting ledger state (two
  freshly-loaded fixture copies) produces the same sequence of decisions and the same executed
  action kinds/targets — proving the loop has no hidden nondeterminism (clock-based `occurred_at`
  values are allowed to differ; decision/action content must not).
- Full workspace: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --check`.

## Acceptance Criteria

- [x] `ekos-simulation` crate builds as a new workspace member; no changes required to `kir`'s or
      `runtime`'s existing public API beyond what RFC 0047-0049 already added — confirmed: zero
      diffs to `kir`/`runtime`/`ledger` source in this RFC, only a new crate consuming their
      existing public surface.
- [x] `DecisionEngine` trait implemented by two deterministic reference engines (`AlwaysDoNothing`,
      `RuleBasedAgent`); no LLM call anywhere in this crate — confirmed by construction (no network
      or model-provider dependency in `crates/simulation/Cargo.toml`).
- [x] `Simulation::run_round` implements the full 8-step lifecycle from the Design section, reusing
      `Runtime::agent_observation`/`build_world` for reads and `KnowledgeStore::append_event`/
      `append_relationship` for writes — no new `KnowledgeStore` trait methods.
- [x] All agents' decisions for round N are computed from the same pre-round observation — the
      implementation collects every agent's observation (step 1) before calling any
      `DecisionEngine::decide` (step 2), and every decision before any `execute_action` (step 5) is
      called, so no agent's decision can structurally see another agent's round-N action;
      `same_round_from_same_starting_state_is_deterministic` in
      `crates/simulation/tests/simulation_fixture.rs` verifies the resulting output is unaffected
      by the collection order.
- [x] Two runs of the same round from the same starting state produce identical decisions/actions
      (determinism check) — `same_round_from_same_starting_state_is_deterministic`, both backends
      exercised elsewhere in the same file's other two tests.
- [x] No Phase 8 (Parallel Execution)/Phase 9 (Conflict Resolution)/Phase 10+ code anywhere in this
      change — confirmed out of scope, not partially started; conflict handling is exactly the
      fixed `SimulationConfig.agents` processing order, nothing stochastic, no `--seed`.
- [x] Full workspace `cargo build/test/clippy/fmt` clean.

A scope difference from the plan below, decided during implementation for a concrete technical
reason: the Testing section originally said the integration test would extend
`runtime/tests/graph_layer_fixture.rs` and have `crates/simulation`'s own test "reuse the fixture
loader." That isn't possible in Rust as stated — a crate's `tests/` integration-test binaries are
not part of its public library API, so `ekos-simulation` (which depends on `ekos-runtime`, not the
other way around) cannot import `ekos-runtime`'s test-only fixture code. `graph_layer_fixture.rs`
was therefore left untouched, and `crates/simulation/tests/simulation_fixture.rs` ships its own
compact, self-contained fixture instead, following the same naming/testing conventions (Alice/Bob/
Charlie, both ledger backends, a documented rationale comment) without literal code sharing.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0050-decision-action-simulation-engine.md` | This RFC, all Acceptance Criteria checked |
| `ekos/Cargo.toml` | New workspace member `crates/simulation`, `ekos-simulation` workspace dependency |
| `ekos/crates/simulation/Cargo.toml` | New crate manifest |
| `ekos/crates/simulation/src/lib.rs` | Crate root, re-exports, 11 unit tests |
| `ekos/crates/simulation/src/action.rs` | `ActionKind`, `Action`, `validate_action` |
| `ekos/crates/simulation/src/decision.rs` | `DecisionContext`, `Decision`, `DecisionEngine`, `AlwaysDoNothing`, `RuleBasedAgent` |
| `ekos/crates/simulation/src/simulation.rs` | `SimulationConfig`, `Simulation`, `RoundResult`, `run_round`/`run` |
| `ekos/crates/simulation/tests/simulation_fixture.rs` | New, self-contained integration test (not a `graph_layer_fixture.rs` extension — see the note above): a 3-agent scenario proving one full round, the `FormAlliance` precondition/effect, public-action `Knows` fanout, and round-to-round determinism, both ledger backends |
