# Devlog 50 — RFC 0050: the first genuinely new subsystem, Decision Engine + Action System + Simulation Engine

**Date:** 2026-08-13
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Fourth and largest RFC in this continuation: RFC 0047 (claims/temporal validity), RFC 0048 (World
Model), RFC 0049 (Agent Model), and now RFC 0050 — Phases 5-7 of `EKOS_World_Engine_Development_
Plan.md` (Agent Decision Engine, Action System, Simulation Engine) in one RFC, the user's explicit
choice after being offered smaller slices (Action-System-only, Decision-Engine-only, or stopping at
RFC 0049) and told directly this was the first fork in the continuation with zero prior art
anywhere in EKOS to extend. Unlike the prior three RFCs, this one earned a brand-new crate
(`ekos-simulation`) rather than another extension to `runtime` — the point RFC 0048's own devlog
had already flagged as the natural extraction moment. The result: a finite 12-action vocabulary, a
provider-independent `DecisionEngine` trait with two deterministic reference implementations, and a
round-based simulation loop that reuses RFC 0048/0049's `build_world`/`agent_observation` for every
read and writes through `KnowledgeStore` directly — no new ledger storage, no new trait methods, no
LLM call anywhere in the crate.

---

## RFC 0050 — Decision Engine, Action System, Simulation Engine

### Problem / motivation

Checked before designing, same discipline as the last three RFCs, but this time the answer was
different: RFC 0047-0049 each found their target concept 80% already present (claims, temporal
validity, world projection, agent observation all had close analogs — `properties` bags, `Custom()`
escape hatches, `ObjectState`/`ImpactHop`-shaped read models). This RFC's target — "an agent chooses
an action, actions have effects, rounds advance a simulation" — has no analog anywhere. The one
constraint that mattered most, confirmed by direct read before writing a line of design: `Runtime`
(`runtime/src/lib.rs`'s own module doc) is read-only end to end, "no `&mut self` methods that affect
the ledger (RFC 0005)." A simulation engine has to write every round. Rather than carve an exception
into `Runtime`, the new crate writes through `&dyn KnowledgeStore` directly — the same access level
`commit.rs` (an existing compiler-pass write path) already has — and keeps using `Runtime` for reads
only. The read-only invariant survives this RFC completely intact.

### What was built

| Component | Location |
|---|---|
| RFC | `ekos/docs/rfcs/0050-decision-action-simulation-engine.md` |
| New crate `ekos-simulation` | `crates/simulation/` — first new workspace member since `crates/demo-server` |
| `ActionKind` (12 variants, no escape hatch) + `Action` + `validate_action` | `crates/simulation/src/action.rs` |
| `DecisionContext`/`Decision`/`DecisionEngine` trait + `AlwaysDoNothing`/`RuleBasedAgent` | `crates/simulation/src/decision.rs` |
| `SimulationConfig`/`Simulation`/`RoundResult`/`run_round`/`run` | `crates/simulation/src/simulation.rs` |
| Integration test | `crates/simulation/tests/simulation_fixture.rs` — self-contained, both backends |

**Deliberately not built**, matching the RFC's own Non-goals and the user's confirmed scope: no
Phase 8 Parallel Agent Execution (the observe-then-decide-then-execute ordering already produces
the same order-independent semantics Phase 8 wants, without needing real concurrency); no Phase 9
Conflict Resolution (priority rules, resource constraints, `--seed` — nothing here is stochastic, so
a seed would have nothing to seed); no LLM-backed `DecisionEngine`; no per-kind action effects
beyond the one worked `FormAlliance` example; no Phase 10+ work (scenarios, replay, metrics, turning
points, reports, Monte Carlo, counterfactuals, web UI, video); no `ekos simulate` CLI command; no
new `KnowledgeStore` trait methods.

### Why a new crate, and why now

RFC 0048's own devlog named this explicitly, a session ago: *"~150 lines doesn't justify a new
workspace member yet... natural extraction point once Agent Model/Simulation Engine need their own
crates anyway."* This RFC is where that prediction paid off. `runtime` is documented and enforced
read-only; a write-capable engine with its own closed vocabulary, its own decision contract, and
its own multi-step loop is categorically different from another `ObjectState`/`World`-shaped
read-model struct. `ekos-simulation` depends on `ekos-kir`, `ekos-ledger` (writes), and
`ekos-runtime` (reads) — no dependency added beyond what RFC 0047-0049 already established as the
graph layer's own dependency surface.

### The lifecycle, and where the source document's own warning actually bites

The source document's Phase 7 (§12) states its lifecycle abstractly and adds one concrete warning:
*"Do not immediately mutate the world after Agent A acts before Agent B has observed the same
round."* `Simulation::run_round` implements this literally, not just in spirit: **every** agent's
`agent_observation` call happens in a first pass (step 1, one shared `occurred_at` cut), **every**
agent's `DecisionEngine::decide` call happens in a second pass over those already-captured
observations (step 2), and only after both passes complete does execution (step 5) begin. This
means an agent's decision is structurally incapable of seeing another agent's round-N action — not
because of a convention the caller has to remember, but because the data it could see (its own
`observations[agent_id]` entry) was captured before any round-N write happened. "Update world" (the
lifecycle's own step 7) needed no code at all: `World` (RFC 0048) is a projection, so the next
round's `agent_observation` calls already see everything the previous round wrote, for free — the
same "don't duplicate the ledger's own state" discipline every prior RFC in this continuation has
held to.

### The one real precondition, and why not eleven more

The source document's own worked example (§11) is `FORM_ALLIANCE: preconditions: trust > 0.4`. This
RFC implements exactly that one precondition and exactly one effect (`Trusts` relationship value
bumped `+0.2`, clamped to `1.0`, same relationship id — a new *version*, matching how RFC 0047's
`relationship_history` already understands "an updated fact"). Every other action kind — `ChangeGoal`
actually mutating an agent's own `goals` property, `Oppose` producing a `Custom("Opposes")`
relationship symmetric to `FormAlliance`'s `Trusts` handling, resource costs, etc. — produces only
the uniform `Custom("ActionExecuted")` audit event and gets no bespoke effect in this RFC. Named
explicitly in Non-goals as deferred, not overlooked: designing effect semantics for eleven action
kinds with no concrete scenario driving their shape yet is exactly the "designed but not exercised"
mistake this session has now avoided four RFCs running (RFC 0048's `Channel`/`resources`
convention, RFC 0049's `goals`/`fears` staying plain strings, and now this).

### A real constraint found mid-implementation: the RFC's own testing plan didn't survive contact with Rust's crate boundaries

The RFC's original Testing section said the integration test would "extend RFC 0047-0049's shared
fixture" and have `crates/simulation`'s test "reuse the fixture loader" from
`runtime/tests/graph_layer_fixture.rs`. That plan doesn't compile: a crate's `tests/` directory is
integration-test code, not part of its public library API, and `ekos-simulation` depends on
`ekos-runtime` (not the reverse) — there is no direction in which one crate's test binary can import
another crate's test-only fixture function. Caught while writing the test, not after a build
failure surprised anyone: `crates/simulation/tests/simulation_fixture.rs` ships its own small,
self-contained fixture instead (Alice/Bob/Charlie, matching the naming convention, a documented
rationale comment explaining exactly this constraint), and both the RFC's Testing section and its
Acceptance Criteria were corrected to describe what was actually built rather than the original,
infeasible plan — the same "the RFC gets corrected to match reality, not shipped wrong" pattern RFC
0047's `is_pending_review()` fix and RFC 0049's `World.events` fix both already established.

### Decisions (alternatives considered, why this choice)

- **No pluggable action registry** (closures/trait objects per `ActionKind`) — rejected; the
  vocabulary is closed and named explicitly in the source document, not something callers extend. A
  `match` in `validate_action`/`execute_action` is the honest shape for a fixed 12-item set — the
  same call RFC 0049 made keeping `goals`/`fears` as plain string lists instead of inventing a typed
  schema for a shape that hasn't varied yet.
- **No YAML/DSL precondition language** (matching the source document's own illustrative YAML block
  literally) — rejected for this RFC; one real Rust-native precondition proves the validation
  contract works without inventing and parsing a rule language nothing else in EKOS has. Real future
  work only if Phase 10 (Scenario Definition) ever needs non-recompiling precondition authoring.
- **No `&mut Runtime`-style write handle carved out for this one caller** — rejected; `Runtime`'s
  read-only invariant (RFC 0005) is load-bearing elsewhere in the codebase and blurring it for one
  new caller reopens a settled question for no real gain, since writing through `&dyn KnowledgeStore`
  directly costs nothing and matches `commit.rs`'s existing access level.
- **No `--seed`/randomness** — rejected for this RFC; nothing in the current loop is stochastic
  (fixed processing order is already fully deterministic), so a seed would configure nothing real.
  Becomes real once Phase 9's actual conflict/priority rules land and need reproducible tie-breaking.

---

## Knowledge Captured

- **When a source document's testing plan says one crate's tests will "reuse" another crate's test
  fixture, check whether that's even expressible in the language before designing the RFC's Testing
  section around it.** Integration tests (`tests/*.rs`) are not part of a crate's public API surface
  in Rust — they can only see what the crate exports as a library, not its own or another crate's
  test-only helper functions. This is a fully general Rust constraint, not project-specific, and
  worth checking early (ideally before writing the RFC, not mid-implementation) whenever a plan
  calls for cross-crate test-code sharing.
- **A read-only invariant (RFC 0005's `Runtime`) survives adding a write-heavy subsystem cleanly if
  the new subsystem writes through the same lower-level interface (`KnowledgeStore`) an existing
  write path already uses, rather than requesting a carve-out on the read-only type itself.** No
  code in `runtime/src/lib.rs` changed for this RFC at all — confirmed by the diff, not assumed.
- **Four RFCs running now, "does the existing escape hatch/convention already cover this" has held
  as the right first question every time** — `Custom()` on `EventKind`/`RelationshipKind` covered
  `Custom("ActionExecuted")` and `Custom("Trusts")` with zero enum changes; `relationship_history`'s
  existing "same id = new version" convention (RFC 0047) covered the `FormAlliance` trust-bump
  effect with zero new "update" mechanism. The genuinely new work this RFC needed was real but
  small: the vocabulary, the decision contract, and the loop's own ordering discipline — not new
  storage primitives.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0050-decision-action-simulation-engine.md` | New RFC, all Acceptance Criteria checked, Testing/Files-Changed sections corrected to match what was actually built |
| `ekos/Cargo.toml` | New workspace member `crates/simulation`, `ekos-simulation` workspace dependency |
| `ekos/crates/simulation/Cargo.toml` | New crate manifest |
| `ekos/crates/simulation/src/lib.rs` | Crate root, re-exports, 11 unit tests |
| `ekos/crates/simulation/src/action.rs` | `ActionKind`, `Action`, `validate_action` |
| `ekos/crates/simulation/src/decision.rs` | `DecisionContext`, `Decision`, `DecisionEngine`, `AlwaysDoNothing`, `RuleBasedAgent` |
| `ekos/crates/simulation/src/simulation.rs` | `SimulationConfig`, `Simulation`, `RoundResult`, `run_round`/`run` |
| `ekos/crates/simulation/tests/simulation_fixture.rs` | New self-contained integration test: 3-agent round, `FormAlliance` precondition/effect, public `Knows` fanout, determinism check — both ledger backends |

## Still open (tracked, not silently dropped)

- **Whether to continue further** — Phase 8 (Parallel Agent Execution, likely a non-event given
  this RFC's ordering already satisfies its stated goal), Phase 9 (real Conflict Resolution with
  priority/resource rules and a `--seed`), or Phase 10+ (Scenario Definition and everything
  downstream) are all real next forks, each needing its own scope confirmation the way this one did.
- **No LLM-backed `DecisionEngine`** — the trait is provider-independent by design, but wiring an
  actual model means real prompt design work, deliberately out of scope here.
- **Per-kind action effects beyond `FormAlliance`** — real, named, deferred; `ChangeGoal` doesn't
  yet mutate anything, `Oppose`/`Support`/`BreakAlliance` don't yet produce relationship effects.
- **No `ekos simulate` CLI command** — library capability only, same posture as `World`/
  `agent_observation` before it.
