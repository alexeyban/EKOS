# Devlog 52 — RFC 0052: Conflict Resolution, and finding the one real collision this engine has

**Date:** 2026-08-13
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Sixth RFC in the continuation: RFC 0047 (claims/temporal validity), RFC 0048 (World Model), RFC
0049 (Agent Model), RFC 0050 (Decision/Action/Simulation Engine), RFC 0051 (Scenario Definition),
and now RFC 0052 — Phase 9 of `EKOS_World_Engine_Development_Plan.md`, Conflict Resolution. The
user picked this over Phase 11 (Virtual Social Environment) or `world.sources` document ingestion,
closing a gap RFC 0050 had named twice: `SimulationConfig` had no seed, and "resolve conflicts" had
been a documented no-op since it shipped. Tracing through the source document's own worked example
before designing anything — `Alice → SUPPORT Bob`, `Charlie → OPPOSE Bob` in one round — turned up
a real finding: as the engine exists today, that example doesn't structurally produce a collision
at all. Building priority/ordering/a seed around it would have been conflict-resolution machinery
with nothing real to resolve. The RFC instead builds the one genuine same-round collision this
engine can currently produce (two agents racing for a shared, scarce `Channel` capacity), and reuses
`Decision.confidence` — already present, previously just an audit annotation — to drive execution
priority.

---

## RFC 0052 — Conflict Resolution

### Problem / motivation

Checked before designing, same discipline as the prior five RFCs, and this time the check reshaped
the scope:

- Traced `Support`/`Oppose` through RFC 0050's `execute_action`: both produce only an audit
  `Custom("ActionExecuted")` event, no relationship or numeric effect on the target at all. Two
  actions with no shared mutation target don't conflict — they're independent facts, and the
  append-only ledger already records both without issue (RFC 0050's own devlog said this
  explicitly). The source document's own example has nothing to resolve, as this engine's effect
  model currently stands.
- `FormAlliance`'s one existing relationship effect (RFC 0050) is keyed by `(actor, target)` —
  unique per actor, so two different actors never touch the same relationship id. No contention
  there either.
- RFC 0048's `Custom("Channel")` convention (a `resources` property with a numeric pool like
  `capacity`) is the one existing shape in this codebase that's genuinely *shared* across actors —
  the RFC builds its one real conflict on top of that, rather than inventing a second grounding
  with nothing behind it.
- `Decision.confidence: f32` (RFC 0050) already existed but nothing read it until now — reused for
  priority ordering instead of adding a new field.

### What was built

| Component | Location |
|---|---|
| RFC | `ekos/docs/rfcs/0052-conflict-resolution.md` |
| `SimulationConfig.seed` | `crates/simulation/src/simulation.rs` |
| Seeded priority ordering (`seeded_rng`/`order_by_priority`) | Same file |
| `try_consume_resource`/`ConsumeResult`, `ExecutionOutcome`, `ConflictError` | Same file |
| `RoundResult.conflict_failures` | Same file |
| `ekos simulate --seed` | `crates/cli/src/commands/simulate.rs`, `bin/ekos.rs` |

**Deliberately not built**, per the RFC's own Non-goals: per-action-kind differentiated resource
costs (one uniform `ACTION_ENERGY_COST` for every non-`DoNothing` action, not eleven speculative
numbers); YAML-authorable costs; richer domain-level conflict rules beyond the one worked
shared-capacity example; Phase 8 Parallel Agent Execution (unchanged finding from RFC 0050: this
RFC's ordering is still single-threaded and deterministic, not concurrency); any randomness in
`DecisionEngine` behavior itself — the seed governs only tie-break *ordering*, never what an agent
decides to do.

### Why the source document's own example doesn't need conflict resolution, as built today

This is the finding that shaped the whole RFC. `Alice → SUPPORT Bob` / `Charlie → OPPOSE Bob` reads
naturally as "these two actions conflict, so something needs resolving" — but nothing in RFC 0050's
effect model makes that true. Considered and rejected: giving `Bob` a numeric "standing" property
both actions adjust (`+0.1`/`-0.1`). A commutative adjustment produces the *same final value*
regardless of execution order — `0.1 + (-0.1)` equals `-0.1 + 0.1`. Building that would have looked
like it exercised order-dependent conflict resolution without actually being a real test of
anything, the exact "designed but not exercised" trap this session has caught and avoided five
times running (RFC 0047's `is_pending_review` bug, RFC 0048's missing event path, RFC 0049's untested
`Proposition`/`Believes` gap noted honestly rather than hidden, RFC 0050's single-worked-precondition
discipline, RFC 0051's case-sensitivity gotcha). The channel-capacity race is a smaller example than
the source document's own illustration, but it's a genuine, order-sensitive collision — named
honestly in the RFC as smaller in scope than the literal example, not a disguised substitute.

### The two-phase check that makes a real collision possible: validate against a shared snapshot, resolve at execution

RFC 0050's own invariant — every agent's decision is made from the same pre-round observation,
before any round-N action executes — means *validation* structurally cannot see same-round
contention: both Alice and Bob's `PostMessage` decisions pass validation against a channel that
still shows full capacity, because neither has executed yet. The actual race can only be observed
at *execution* time, and only if execution happens in a defined order. That's what priority ordering
(confidence descending, seeded tie-break for the common case of equal confidence) is actually for in
this RFC — not cosmetic sequencing, but the mechanism that makes "whoever goes first wins" a
coherent, reproducible question at all. `try_consume_resource`'s re-check inside `execute_action`
is the moment the second agent's identical, equally-reasonable decision discovers the resource is
already gone.

### Decisions (alternatives considered, why this choice)

- **Per-action-kind resource costs now** — rejected; no concrete scenario in this codebase
  distinguishes a `PostMessage`'s cost from a `FormAlliance`'s, and picking specific numbers with no
  grounding would be fabricated precision.
- **A dedicated priority field on `SimulatedAgent`** instead of reusing `Decision.confidence` —
  rejected; confidence already exists, is finer-grained (per-decision, not static per-agent), and
  costs zero new schema.
- **Building conflict resolution around the source document's literal example** — rejected once
  traced through and found not to be a real collision under the current effect model; the RFC says
  so directly rather than quietly building something that looks like it addresses the example.
- **Folding conflict failures into `validation_failures`** — rejected; conflating "never reasonable"
  with "reasonable but lost a same-round race" erases real information. `RoundResult` keeps them
  distinct, mirroring the distinction RFC 0050 already drew between structural rejection and
  everything else.

---

## Knowledge Captured

- **Before building "conflict resolution" for a source document's own worked example, trace the
  example through the actual current effect model to confirm it produces a real collision at all.**
  It didn't. A commutative numeric adjustment is a specific, checkable reason an example *won't*
  exercise order-dependent behavior — worth watching for whenever "two things happen to the same
  target" is proposed as a conflict without checking whether the effect is order-sensitive.
- **A field that exists but nothing reads (`Decision.confidence`, sitting unused since RFC 0050) is
  worth checking before adding a new one for a similar purpose.** Priority ordering needed exactly
  what confidence already represents — no schema change, no new concept for a scenario author to
  learn.
- **Making a same-round conflict observable requires a defined execution order, not just a shared
  resource.** Validation happening against one static pre-round snapshot (RFC 0050's own
  determinism guarantee) means contention can only ever surface at execution time, in whatever
  order execution actually happens — which made priority ordering a load-bearing prerequisite for
  the conflict mechanism, not a separate feature bolted on alongside it.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0052-conflict-resolution.md` | New RFC, all Acceptance Criteria checked |
| `ekos/crates/simulation/Cargo.toml` | Add `rand` |
| `ekos/crates/simulation/src/simulation.rs` | `SimulationConfig.seed`; `seeded_rng`/`order_by_priority`; `try_consume_resource`/`ConsumeResult`; `ExecutionOutcome`; `ConflictError`; `RoundResult.conflict_failures`; 10 new tests |
| `ekos/crates/simulation/src/scenario.rs` | `load_scenario` wires `scenario.simulation.seed` through |
| `ekos/crates/simulation/src/lib.rs` | `ConflictError` re-exported |
| `ekos/crates/simulation/tests/simulation_fixture.rs` | Mechanical: `SimulationConfig` construction updated for `seed` |
| `ekos/crates/simulation/tests/conflict_fixture.rs` | New: channel-capacity-race integration test, 3 tests |
| `ekos/crates/cli/src/commands/simulate.rs` | `--seed` override; always-shown `Seed:` line; `conflict_failures` printed per round |
| `ekos/crates/cli/src/bin/ekos.rs` | `Commands::Simulate` gains `seed: Option<u64>` |

## Still open (tracked, not silently dropped)

- **Whether to continue further** — Phase 11 (Virtual Social Environment) or `world.sources`
  document ingestion are still the two real next forks, each needing its own scope confirmation.
- **No per-action-kind resource costs** — one uniform constant for now; real, deferred once a
  concrete scenario needs differentiated weights.
- **No richer conflict rules** beyond the one worked shared-capacity example — nothing else in the
  current action/effect set produces a second kind of real collision yet.
- **`Support`/`Oppose` still have no relationship effect** — carried over from RFC 0050, unchanged;
  this RFC confirmed (rather than fixed) that the source document's own worked example doesn't need
  one under the current model.
