# RFC 0051 — Scenario Definition

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-13

---

## Motivation

Fifth RFC in the continuation: RFC 0047 (claims/temporal validity), RFC 0048 (World Model), RFC
0049 (Agent Model), RFC 0050 (Decision Engine + Action System + Simulation Engine), and now Phase
10 of `EKOS_World_Engine_Development_Plan.md` (§15) — Scenario Definition, the user's choice after
being offered three alternatives (Phase 9 Conflict Resolution, a bare CLI wrapper around what
already exists, or stopping) and told directly why it mattered: after four RFCs and a whole new
crate, **nothing built so far is runnable by anyone who isn't editing this codebase** — every
scenario in every test (RFC 0050's `simulation_fixture.rs` included) is a hand-written Rust
fixture. The user picked the option that closes that gap, not the option that deepens the engine
further while the gap stays open.

Checked before designing, same discipline as the prior four RFCs:

- The source document's own scenario/agent YAML shapes (§9.1, §15) map almost directly onto
  conventions RFC 0049/0050 already established: `role`/`goals`/`fears`/`resources` are exactly
  `SimulatedAgent`'s existing `properties` convention; `relationships: {bob: {trust: -0.7}}` is
  exactly the `Custom("Trusts")` + `properties["value"]` shape RFC 0050's `bump_trust` already
  writes; `beliefs: [...]` is exactly RFC 0049's `Proposition` + `Believes` claim convention —
  named as designed-but-untested in `devlog_49.md`'s own "Still open" section, closed here by real
  parsing code instead of hand-built test fixtures.
- The source document's `world.sources: [reports/report_01.md, ...]` (ingesting real documents into
  world state) requires the full Observer → ArtifactStore → recovery-pass → KIR → ledger pipeline —
  confirmed by re-reading `CLAUDE.md`'s own architecture table, not assumed. Wiring that is a
  disproportionate scope increase for this RFC and is **not** attempted (see Non-goals) — agents'
  `knowledge:` lists instead reference either another named entity within the same scenario file
  set, or (new, minimal) a handful of scenario-authored seed events, or an existing object/event
  already in the ledger by its real id.
- `ekos_common::redaction`'s "no delete/tombstone exists anywhere in the codebase" finding (RFC
  0043's devlog, restated in `CLAUDE.md`'s own invariants list) matters concretely here: a
  simulation scenario's fictional `SimulatedAgent`/`Proposition`/`Custom("ActionExecuted")` entities
  would become **permanent, undeletable** entries in whatever ledger they're appended to. Appending
  them straight into a user's real workspace ledger (`.ekos/ledger/ledger.db`, the same store `ekos
  build`/`commit` populate with real, evidence-backed enterprise knowledge) would permanently
  commingle fictional simulation data with real compiled facts — this RFC does not do that (see
  Design's storage-location decision below).

## Scope

1. **`AgentDefinition`** (YAML, source document §9.1's schema, minus `knowledge`'s literal
   `event_001`-style ids, which need scenario-local resolution — see below): `id`, `name`, `role`,
   `goals`, `fears`, `beliefs`, `knowledge`, `relationships`, `resources`.
2. **`ScenarioDefinition`** (YAML, source document §15's schema, trimmed to what this RFC actually
   implements): `id`, `name`, `agents` (a list mixing file-path references and inline agent
   definitions), a new, minimal `events` section (scenario-authored seed events, substituting for
   `world.sources` document ingestion — named as a deliberate deviation, not a silent one),
   `simulation.rounds`, `simulation.seed` (parsed for forward-compatibility, not wired to anything —
   RFC 0050 already established there's no stochastic behavior yet to seed).
3. **A `ScenarioLoader`** — resolves every name reference (agent-to-agent, agent-to-event) within
   the scenario's own file set, falling back to parsing a reference as a literal `KirId` already
   present in the target store if no local name matches (a narrow, real bridge to pre-existing
   ledger content, short of full document ingestion); constructs every `KirObject`/`KirRelationship`/
   `KirEvent` the scenario implies and appends them; returns a ready-to-run `SimulationConfig` +
   default `DecisionEngine` assignment (`RuleBasedAgent` for every agent — the only deterministic,
   goal-string-driven engine that exists, RFC 0050).
4. **`ekos simulate <scenario.yaml>`** — the CLI command every prior RFC in this continuation
   deferred as a named non-goal, built now because the user picked this option specifically to
   close the "nothing is runnable" gap; parsing/loading without a way to invoke it wouldn't actually
   close that gap.
5. **Scenario-scoped storage, not the real workspace ledger** — `ekos simulate` opens (or creates) a
   dedicated ledger at `.ekos/simulations/<scenario-id>/ledger.db`, never the real
   `.ekos/ledger/ledger.db`, for the reason given in Motivation. An explicit `--ledger <path>` flag
   overrides this for advanced/programmatic use.

## Non-goals

- **No `world.sources` document ingestion.** Real, deferred; would mean wiring the full compiler
  pipeline into scenario loading, a different-sized RFC. Scenario `knowledge:` references stay
  scoped to what a scenario author can name directly: other scenario entities, or pre-existing
  ledger ids they already know.
- **No Phase 11 Virtual Social Environment** (`VirtualForum`, channels, `publish_message`/`reply`/
  `like`/`share`/`follow`) — separate RFC, unaffected by this one.
- **No `--seed`-driven randomness.** `simulation.seed` parses (so a scenario author's file doesn't
  silently fail validation once Phase 9 lands) but the CLI prints a one-line notice that it's
  currently a no-op, rather than silently ignoring stated intent.
- **No per-agent decision-engine selection in YAML** (e.g. an `engine: llm` field) — every agent
  gets `RuleBasedAgent`; richer engine selection is real, deferred work, blocked on an LLM-backed
  `DecisionEngine` actually existing (RFC 0050 Non-goal, still true).
- **No scenario "linting" beyond structural parse errors and reference-resolution errors** (unknown
  agent/event name). Whether a scenario is *interesting* (goals that never resolve to an action,
  agents nobody ever interacts with) is left to the scenario author.
- **No deletion/cleanup command for a scenario's ledger.** `.ekos/simulations/<id>/` is an ordinary
  directory a user can `rm -rf` themselves; no ledger-level tombstone exists anywhere in this
  codebase to build one on top of (RFC 0043).

## Design

### Storage: scenario-scoped, not the real workspace ledger

The single most consequential decision in this RFC. `open_store(config, cwd)` (`cli/src/commands/
store.rs`) is what every other CLI command uses, and it always resolves to the workspace's one real
ledger. `ekos simulate` deliberately does **not** call it by default: `SimulatedAgent`/`Proposition`/
`Custom("ActionExecuted")` entities are fictional, generated fresh on every run, and — because the
ledger has no delete/tombstone mechanism anywhere in this codebase (RFC 0043's own confirmed
finding) — anything appended to the real ledger stays there forever, structurally indistinguishable
at the storage layer from real, evidence-backed compiled knowledge (both are just `KirObject`s with
a `Custom()` kind and a `properties` bag). Defaulting to a dedicated per-scenario ledger
(`.ekos/simulations/<scenario-id>/ledger.db`) costs nothing — the CLI still runs scenarios, still
prints results — and avoids a real, currently-irreversible risk. `--ledger <path>` stays available
for a caller who deliberately wants otherwise (e.g. a future RFC that wants a simulation to reason
over real, previously-compiled enterprise knowledge as its `world` — a genuinely interesting future
direction, not attempted here).

### `AgentDefinition` → KIR, reusing RFC 0049/0050 conventions unmodified

```yaml
id: alice
name: Alice
role: founder
goals: [retain_control, "support:acme"]
beliefs: [bob_wants_to_replace_me]
fears: [public_scandal]
knowledge: [alice_saw_theft]
relationships:
  bob: { trust: 0.5 }
resources: { influence: 0.8, money: 0.5, information: 0.9 }
```

- `role`/`goals`/`fears`/`resources` → `KirObject::new(name, ObjectKind::Custom("SimulatedAgent"))`
  with those four keys set as `properties`, verbatim RFC 0049 convention. `goals` entries using
  RFC 0050's `RuleBasedAgent` sub-convention (`"support:<name>"`/`"oppose:<name>"`) work
  automatically — no new code needed in `decision.rs`.
- `relationships.bob.trust: 0.5` → a `Custom("Trusts")` relationship from Alice to whatever `bob`
  resolves to, `properties: {"value": 0.5, "status": "confirmed"}` — `"confirmed"`, not
  `"unconfirmed"`, matching RFC 0050's own `bump_trust` convention (a scenario author asserting a
  starting trust value is asserting world truth for this simulation, not a hypothesis pending
  review — an `"unconfirmed"` status would make `build_world` silently exclude it, breaking
  `FormAlliance`'s precondition check the moment a scenario tried to use it).
- `knowledge: [alice_saw_theft]` → a `Custom("Knows")` relationship from Alice to whatever
  `alice_saw_theft` resolves to (an agent, a scenario-authored event, or an existing ledger id).
- `beliefs: [bob_wants_to_replace_me]` → a `Custom("Proposition")` object named after the belief
  text, plus a `Custom("Believes")` claim relationship from Alice to it,
  `properties: {"status": "unconfirmed", "confidence": 0.6}` — RFC 0049's own documented
  convention, exercised by real parsing code for the first time (`devlog_49.md`'s "Still open"
  section named exactly this as untested).

### `ScenarioDefinition` and name resolution

```yaml
id: open_source_conflict
name: "The Battle for Project X"
agents:
  - alice.yaml
  - bob.yaml
  - { id: charlie, name: Charlie, role: bystander }   # inline definition, same schema
events:
  - id: alice_saw_theft
    kind: Observed
    subject: alice
    payload: { note: "Alice saw money taken from the till" }
simulation:
  rounds: 5
  seed: 42   # parsed; CLI notes it's currently a no-op (see Non-goals)
```

`agents` accepts a mix of file-path strings (resolved relative to the scenario file's own
directory) and inline mappings (`#[serde(untagged)] enum AgentRef { Path(String),
Inline(AgentDefinition) }`) — both the source document's own reusable-per-agent-file pattern and a
fully self-contained single-file scenario are first-class, not one bolted onto the other.

Loading proceeds in two passes, the same "assign names before resolving references" shape every
compiler pass in this codebase already uses when producing KIR from a source with human-readable
identifiers:

1. **Names pass** — a fresh `KirId` is generated for every agent and every scenario-authored event;
   `HashMap<String, KirId>` records `agent.id`/`event.id` → the generated id. A duplicate name
   (an agent and an event sharing an id, or two agents with the same id) is a load error, not a
   silent overwrite.
2. **Resolution pass** — `relationships`/`knowledge`/`beliefs` entries are turned into
   `KirRelationship`/`KirObject`s using the names map; a name not found there falls back to
   `KirId::from_str` + a store lookup (`get_object`/`get_event`) before failing — the narrow bridge
   to pre-existing ledger content named in Scope.

### CLI (`ekos simulate <scenario.yaml>`)

```bash
ekos simulate scenario.yaml
ekos simulate scenario.yaml --rounds 10          # override simulation.rounds
ekos simulate scenario.yaml --ledger custom.db   # advanced: write elsewhere, including the real ledger
```

Opens (creating if absent) the scenario-scoped ledger, loads the scenario, runs
`simulation.rounds` (or `--rounds`) rounds via `Simulation::run`, and prints a per-round,
per-agent summary (`round 0: alice -> FormAlliance(bob); bob -> Oppose(acme); charlie -> DoNothing`)
plus validation-failure notices, to stdout — enough to see a scenario actually happen without
needing `--json`/a UI.

## Alternatives Considered

- **Wiring `world.sources` document ingestion in this RFC** — rejected; a full pipeline hookup
  (Observer → ArtifactStore → recovery → KIR → ledger, scoped to arbitrary "reports/*.md" style
  inputs) is its own RFC-sized problem, and nothing this RFC needs actually requires it — the
  minimal scenario-authored `events:` section proves out `knowledge:` resolution completely.
- **Defaulting `ekos simulate` to the real workspace ledger** (matching every other CLI command's
  `open_store` convention) — rejected outright once RFC 0043's "no delete/tombstone" finding was
  checked against what this command would actually write: permanent commingling of fictional and
  real ledger content, for a command whose entire purpose is throwaway, repeatable scenario runs.
  The scenario-scoped default costs nothing and removes a real risk.
- **A full expression/rule DSL for `relationships`/`knowledge` resolution** (e.g. supporting
  computed references, wildcards, templating) — rejected; plain name-or-existing-id resolution
  covers every example in the source document's own scenario schema without inventing a language
  nothing else in this codebase has, the same call RFC 0050 made for action preconditions.
- **Per-agent decision-engine selection via a YAML field now** — rejected; with only one
  deterministic reference engine in existence (RFC 0050), a selection field would have exactly one
  legal value. Real once an LLM-backed engine exists to select between.

## Testing

- `simulation` unit tests (`scenario.rs`): `AgentDefinition` round-trips through YAML into the
  expected `properties` shape; `relationships`/`knowledge`/`beliefs` resolve correctly against a
  hand-built names map; an unknown reference name (with no matching existing-ledger id either)
  produces `ScenarioError::UnknownReference`, not a panic or a silently-dropped edge; a duplicate
  name across agents/events produces `ScenarioError::DuplicateName`.
- `simulation` integration test: a two-file scenario (`scenario.yaml` + `alice.yaml`/`bob.yaml` on
  disk in a tempdir) loads, runs `simulation.rounds` rounds via the loaded `SimulationConfig`, and
  produces the same shape of result RFC 0050's own fixture asserted on by hand — proving the loader
  produces a `Simulation` indistinguishable, from the engine's point of view, from one built
  directly in Rust. A second test proves the `beliefs:` → `Proposition`/`Believes` path specifically
  (closing `devlog_49.md`'s named gap).
- CLI smoke test (`cli` crate, or a `demo/`-style manual check, whichever proves cheaper): running
  `ekos simulate` against a real scenario file writes to `.ekos/simulations/<id>/ledger.db`, not
  `.ekos/ledger/ledger.db` — a regression here would silently reintroduce the exact risk this RFC's
  Design section exists to avoid.
- Full workspace: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --check`.

## Acceptance Criteria

- [x] `AgentDefinition`/`ScenarioDefinition` YAML schemas implemented, matching the source
      document's §9.1/§15 shapes except where explicitly deviated (documented above), reusing RFC
      0049/0050 conventions with zero changes to `kir`/`runtime`/`simulation`'s pre-existing types
      (`ActionKind` gained one new associated function, `all()`, used both by the loader's default
      vocabulary and available for any other caller — not a behavior change to anything existing).
- [x] `ScenarioLoader` resolves all name references within a scenario's own file set, with a
      documented, tested fallback to existing-ledger-id resolution, and a real (not silently
      swallowed) error for unknown names and duplicate names — `ScenarioError::UnknownReference`/
      `DuplicateName`, both covered by dedicated tests.
- [x] `beliefs:` → `Proposition` + `Believes` path exercised by a real test
      (`beliefs_reify_as_proposition_plus_believes_claim`) — closing the gap named in
      `devlog_49.md`.
- [x] `ekos simulate <scenario.yaml>` runs end-to-end against a real two-file scenario on disk —
      verified both by `scenario_loads_from_disk_and_runs_end_to_end` and by a manual CLI smoke
      test against a real 3-file scenario (`scenario.yaml` + `alice.yaml` + `bob.yaml`) producing
      the expected `alice -> Support(bob)` / `bob -> DoNothing(-)` output across 2 rounds.
- [x] `ekos simulate` writes to a scenario-scoped ledger by default, never the real workspace
      ledger — verified by the same manual smoke test: only `.ekos/simulations/<id>/ledger.db` was
      created, `.ekos/ledger/` never existed.
- [x] `simulation.seed` parses without error but produces a visible (not silent) no-op notice —
      confirmed in the same smoke test's printed output.
- [x] No Phase 11 (Virtual Social Environment) or document-ingestion code anywhere in this change —
      confirmed out of scope, not partially started.
- [x] Full workspace `cargo build/test/clippy/fmt` clean.

One implementation-time fix beyond the original plan: `clippy::large_enum_variant` flagged
`AgentRef` (`Path(String)` at ~24 bytes next to `Inline(AgentDefinition)` at ~264 bytes) — fixed by
boxing the inline variant (`Inline(Box<AgentDefinition>)`), a purely mechanical change with no
schema or behavior effect.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0051-scenario-definition.md` | This RFC, all Acceptance Criteria checked |
| `ekos/crates/simulation/Cargo.toml` | Add `serde_yaml` dependency |
| `ekos/crates/simulation/src/action.rs` | `ActionKind::all()` |
| `ekos/crates/simulation/src/decision.rs` | `RuleBasedAgent`'s doc comment clarified: goal-string target matching is case-sensitive against `name`, distinct from a scenario's own lowercase reference ids — a real gotcha found while writing this RFC's own test fixture |
| `ekos/crates/simulation/src/scenario.rs` | `AgentDefinition`, `ScenarioDefinition`, `AgentRef` (boxed inline variant), `EventDef`, `SimulationSettings`, `load_scenario`/`load_scenario_from_path`, `ScenarioError`; 12 unit tests |
| `ekos/crates/simulation/src/lib.rs` | `pub mod scenario;` + re-exports |
| `ekos/crates/simulation/tests/scenario_fixture.rs` | New integration test: multi-file scenario on disk, end-to-end load + run, inline-agent variant, unknown-reference error path |
| `ekos/crates/cli/Cargo.toml` | Add `ekos-simulation`, `serde_yaml` dependencies |
| `ekos/crates/cli/src/commands/simulate.rs` | New CLI command: scenario-scoped ledger open/create, load, run, print summary |
| `ekos/crates/cli/src/commands/mod.rs` | `pub mod simulate;` |
| `ekos/crates/cli/src/bin/ekos.rs` | New `Commands::Simulate { scenario, rounds, ledger }` variant + dispatch |
