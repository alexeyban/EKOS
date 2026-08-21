# Devlog 51 — RFC 0051: Scenario Definition, and finally something runnable

**Date:** 2026-08-13
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Fifth RFC in the continuation: RFC 0047 (claims/temporal validity), RFC 0048 (World Model), RFC
0049 (Agent Model), RFC 0050 (Decision Engine + Action System + Simulation Engine), and now RFC
0051 — Phase 10 of `EKOS_World_Engine_Development_Plan.md`, Scenario Definition. The user picked
this specifically over deepening the engine further (Phase 9 Conflict Resolution) after being told
directly that four RFCs and a whole new crate in, nothing built so far was runnable by anyone who
wasn't editing this codebase — every scenario in every test was a hand-written Rust fixture. This
RFC closes that gap: YAML scenario/agent files (matching the source document's own §9.1/§15
schemas, reusing every convention RFC 0049/0050 already established) plus a new `ekos simulate`
CLI command. One safety-relevant design decision shaped the whole RFC: simulation runs write to a
dedicated, scenario-scoped ledger by default, never the user's real workspace ledger — because the
ledger has no delete mechanism anywhere in this codebase, and fictional simulation entities have no
business becoming permanent, undeletable neighbors of real, evidence-backed compiled knowledge.

---

## RFC 0051 — Scenario Definition

### Problem / motivation

The gap was concrete and self-inflicted: `devlog_50.md`'s own "Still open" section named "no `ekos
simulate` CLI command" as a repeated, deliberate non-goal across three RFCs running. Asked directly
whether to keep deepening the engine (Phase 9) or close that gap (Phase 10, or a bare CLI wrapper),
the user picked Phase 10 specifically — not the smaller CLI-only option — because scenario
definition addresses the larger problem: a CLI command wrapping RFC 0050's existing types would
still require hand-writing a Rust `SimulationConfig`/`DecisionEngine` map to use it. Scenario files
are what actually let someone who isn't editing this codebase describe a simulation.

### What was built

| Component | Location |
|---|---|
| RFC | `ekos/docs/rfcs/0051-scenario-definition.md` |
| `AgentDefinition`/`ScenarioDefinition`/`AgentRef`/`EventDef` (YAML) | `crates/simulation/src/scenario.rs` |
| `load_scenario`/`load_scenario_from_path` | Same file — the pure core plus a file-IO wrapper |
| `ekos simulate <scenario.yaml>` | `crates/cli/src/commands/simulate.rs`, wired into `bin/ekos.rs` |

**Deliberately not built**, per the RFC's own Non-goals: `world.sources` document ingestion (would
mean wiring the full Observer → ArtifactStore → recovery → KIR → ledger pipeline into scenario
loading — a different-sized RFC); Phase 11 Virtual Social Environment; `--seed`-driven randomness
(parses, produces a visible no-op notice instead of silently discarding stated intent); per-agent
decision-engine selection in YAML (every agent gets `RuleBasedAgent`, the only deterministic
reference engine that exists); scenario linting beyond structural/reference errors; any
ledger-cleanup command.

### The decision that mattered most: never default to the real workspace ledger

Every other CLI command in this codebase opens the workspace's one real ledger via
`commands::store::open_store`. `ekos simulate` deliberately does not call it. The reasoning traces
straight back to `CLAUDE.md`'s own standing invariant, restated from RFC 0043's devlog: "there is
no way to un-commit something already ledgered" — no object-level delete or tombstone exists
anywhere in this codebase. A `SimulatedAgent`/`Proposition`/`Custom("ActionExecuted")` entity is,
at the storage layer, structurally indistinguishable from a real, evidence-backed compiled fact —
both are just a `KirObject`/`KirEvent` with a `Custom()` kind and a `properties` bag. Running a
demo scenario against a real company's real workspace ledger would have permanently commingled
fictional Alice/Bob/Acme data with real compiled knowledge, with no way back. `ekos simulate`
instead opens (or creates) `.ekos/simulations/<scenario-id>/ledger.db` by default — a sibling
location, never the same file — with `--ledger <path>` available for a caller who deliberately
wants otherwise. Verified directly, not just argued: a manual smoke test against a real 3-file
scenario confirmed only `.ekos/simulations/smoke_scenario/ledger.db` was created and
`.ekos/ledger/` never existed.

### The two-pass loader, and why name resolution needed real thought

Scenario files use human-readable string ids everywhere (`agent.id: alice`, `relationships: {bob:
...}`, `knowledge: [alice_saw_theft]`) but KIR's real identity system is UUID-based (`KirId(Uuid)`),
generated fresh on every append. `load_scenario` resolves this with the same "assign names before
resolving references" shape every KIR-producing compiler pass in this codebase already uses: pass
one creates a `KirObject` for every agent and a `KirEvent` for every scenario-authored seed event
(the RFC's own minimal substitute for `world.sources` — real document ingestion was out of scope),
recording each in a `HashMap<String, KirId>` as it goes and erroring on any name collision; pass two
resolves every `relationships:`/`knowledge:`/`beliefs:` entry against that completed registry,
falling back to treating an unresolved name as a literal existing `KirId` in the target ledger
before finally failing — a narrow, real bridge to pre-existing ledger content, short of building
full ingestion.

`relationships:`'s `trust` value maps to a `Custom("Trusts")` relationship with `status:
"confirmed"`, not `"unconfirmed"` — a real decision, not the obvious default. RFC 0049's belief
convention normally marks claims `"unconfirmed"`, but a scenario author asserting a starting trust
value is asserting this simulation's world truth, not a hypothesis pending review; marking it
`"unconfirmed"` would make `build_world` (RFC 0048) silently exclude it from every agent's
observation, breaking `FormAlliance`'s precondition check the moment a scenario tried to use it —
caught by tracing through RFC 0050's own validation logic before picking the status value, not
after a test failed unexplainably.

### A real gotcha found writing the RFC's own test fixture

`RuleBasedAgent`'s goal-string convention (`"support:<name>"`) matches against a `KirObject`'s
`name` field, exactly and case-sensitively — not the scenario's own lowercase reference id used for
`relationships:`/`knowledge:`. The first draft of this RFC's end-to-end CLI smoke test used
`goals: ["support:bob"]` against an agent named `Bob` (capital B in `name:`, lowercase `bob` as the
reference id) and silently got `DoNothing` back — no error, just the wrong decision, exactly the
kind of failure mode that's hardest to debug from the outside. Fixed the test data
(`"support:Bob"`), and — more importantly — fixed `RuleBasedAgent`'s own doc comment to name this
distinction explicitly, since a scenario author hitting this in the wild would have no compiler
error to guide them either.

### Decisions (alternatives considered, why this choice)

- **No `world.sources` document ingestion in this RFC** — rejected; a full pipeline hookup is its
  own RFC-sized problem, and the minimal scenario-authored `events:` section proves out
  `knowledge:` resolution completely without it.
- **Defaulting to the real workspace ledger, matching every other CLI command's convention** —
  rejected outright once RFC 0043's "no delete/tombstone" finding was checked against what this
  specific command would actually write. The scenario-scoped default costs nothing.
- **A YAML/DSL expression language for name resolution** (wildcards, computed references, templating)
  — rejected; plain name-or-existing-id resolution covers every example in the source document's
  own scenario schema, the same call RFC 0050 made for action preconditions.
- **Per-agent decision-engine selection via YAML now** — rejected; with exactly one deterministic
  reference engine in existence, a selection field would have exactly one legal value.

---

## Knowledge Captured

- **A convention that's exact-match/case-sensitive needs to say so in its own doc comment, not just
  in the RFC that introduces it** — `RuleBasedAgent`'s `support:`/`oppose:` goal-string matching
  against `KirObject.name` was documented correctly in RFC 0050, but the distinction from a
  scenario's own separate, differently-cased reference-id field only became a real trap once a
  second, independent naming system (RFC 0051's scenario ids) existed to collide with it. Worth
  remembering: a convention that's safe in isolation can still need a stronger warning once a
  second file format starts feeding it.
- **When a source document's schema uses human-readable ids for something your system generates
  UUIDs for, the resolution layer is the real design work, not the schema parsing.** Parsing
  `AgentDefinition`/`ScenarioDefinition` from YAML was mechanical; deciding when a name lookup
  should fail loudly versus fall back to an existing ledger id, and in what order passes must run so
  references can be resolved at all, was the actual RFC-sized problem.
- **"Which ledger does this write to" is a question worth asking explicitly for every new write
  path, not just inheriting whatever the nearest existing command does.** Four RFCs' worth of
  library code (RFC 0047-0050) never had to answer this, because nothing wrote through the CLI yet.
  The moment a CLI command did, the answer "same as everything else" turned out to be wrong for
  this specific kind of data — worth treating as a standing question for any future write-capable
  command, not just this one.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0051-scenario-definition.md` | New RFC, all Acceptance Criteria checked |
| `ekos/crates/simulation/Cargo.toml` | Add `serde_yaml` |
| `ekos/crates/simulation/src/action.rs` | `ActionKind::all()` |
| `ekos/crates/simulation/src/decision.rs` | `RuleBasedAgent` doc comment clarified re: case-sensitive name matching |
| `ekos/crates/simulation/src/scenario.rs` | New: `AgentDefinition`, `ScenarioDefinition`, `AgentRef`, `EventDef`, `SimulationSettings`, `load_scenario`/`load_scenario_from_path`, `ScenarioError`; 12 unit tests |
| `ekos/crates/simulation/src/lib.rs` | `pub mod scenario;` + re-exports |
| `ekos/crates/simulation/tests/scenario_fixture.rs` | New: multi-file scenario end-to-end, inline agents, unknown-reference error path |
| `ekos/crates/cli/Cargo.toml` | Add `ekos-simulation`, `serde_yaml` |
| `ekos/crates/cli/src/commands/simulate.rs` | New CLI command |
| `ekos/crates/cli/src/commands/mod.rs` | `pub mod simulate;` |
| `ekos/crates/cli/src/bin/ekos.rs` | New `Commands::Simulate` variant + dispatch |

## Still open (tracked, not silently dropped)

- **Whether to continue further** — Phase 9 (Conflict Resolution, still deferred), Phase 11
  (Virtual Social Environment), or `world.sources` document ingestion are all real next forks, each
  needing its own scope confirmation the way this one did.
- **No per-agent decision-engine selection** — every scenario agent gets `RuleBasedAgent`; real,
  deferred, blocked on an LLM-backed `DecisionEngine` actually existing.
- **No scenario cleanup command** — `.ekos/simulations/<id>/` is an ordinary directory a user can
  remove themselves; no ledger-level tombstone exists anywhere in this codebase to build one on.
- **`simulation.seed` still does nothing** — parses, prints a notice, real once Phase 9 lands.
