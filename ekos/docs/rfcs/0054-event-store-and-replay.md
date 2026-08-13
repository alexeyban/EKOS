# RFC 0054 — Event Store (closing Phase 12) and Simulation Replay (Phase 13)

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-13

---

## Motivation

Eighth RFC in the continuation: RFC 0047 (claims/temporal validity), RFC 0048 (World Model), RFC
0049 (Agent Model), RFC 0050 (Decision/Action/Simulation Engine), RFC 0051 (Scenario Definition),
RFC 0052 (Conflict Resolution), RFC 0053 (Virtual Social Environment), and now RFC 0054 — the
user's explicit choice: close Phase 12 (§17, Event Store) honestly, then build Phase 13 (§18,
Simulation Replay) on top of it.

Checked before designing, same discipline as the prior seven RFCs — and Phase 12 turned out to be
almost entirely already done:

- The source document's own Phase 12 example event
  (`{id, round, timestamp, actor, action, target, content, observed_by}`) — compared field by
  field against `execute_action`'s existing `Custom("ActionExecuted")` payload (RFC 0050/0052/0053)
  — matches on every field except `observed_by`. `id`/`round`/`timestamp`/`actor`/`action`/
  `target`/`content` are all already there.
- `observed_by` is *not* missing data, only a missing *query* over data that already exists: every
  observer of an event already gets a `Custom("Knows")` edge pointing at it (RFC 0050's own
  `observers_for`/`append_knows`). Baking a redundant list into the event's own payload would
  duplicate what the `Knows` edges already represent, and could go stale if edges were ever added
  later — the same "derive, don't duplicate" call RFC 0053 made for `PostedIn`.
- Phase 13's own text names exactly why this matters beyond bookkeeping: "the event log becomes
  the basis for replay." Checked concretely: `RoundResult` (RFC 0050) exists only in memory,
  returned once from `run_round` and gone when the caller's process exits — there is currently no
  way to reconstruct "what happened in round N" from a ledger alone, after the fact. That gap, not
  `observed_by`, is Phase 12's one real remaining piece of work, and it's a hard prerequisite for
  Phase 13, not a separate concern.
- `KnowledgeStore` still has no bulk "every event" query (re-confirmed, same finding RFC 0053
  made) — so a durable, replayable log needs the same kind of relationship index RFC 0053 built for
  channels (`PostedIn`), generalized to every executed action, not just messages.

## Scope

1. **A durable simulation log** — every executed action (RFC 0050's `execute_action`, all 12
   kinds, not just `PostMessage`) gets a `Custom("LoggedIn")` relationship from its event to a
   single, per-ledger `Custom("SimulationLog")` root object, found-or-created once per `run_round`
   call. Since a scenario already gets its own dedicated ledger (RFC 0051's safety design), "the
   log for this ledger" is naturally singular — no scenario-id discriminator needed.
2. **`observed_by(store, event_id) -> Vec<KirId>`** — a query over existing `Knows` edges, not new
   storage, closing Phase 12's one real gap the way the source document's own example implies it
   (a derivable view, not a second copy of the same fact).
3. **`Replay`** (`crates/simulation/src/replay.rs`) — a read-only controller over a *previously
   simulated* ledger: `rounds()` (every round number that was ever recorded), `events_in_round(n)`,
   `jump_to(n)` (the one timestamp every event in round `n` shares — `run_round` computes
   `occurred_at` once per round and reuses it for every event that round), `inspect_agent`/
   `inspect_graph` (thin wrappers over the point-in-time machinery that already exists —
   `Runtime::agent_observation`/`build_world` with `at` set to `jump_to`'s result), and
   `observed_by`.
4. **`ekos replay <scenario.yaml> [--round N]`** — prints a recorded simulation's rounds (or one
   round, narrowed), resolving names from the ledger's own objects directly (not by re-running
   `load_scenario_from_path`, which would re-append scenario data as a write side effect — replay
   only reads).

## Non-goals

- **No interactive stepping session** (`start`/`pause`/`next round` as literal, stateful REPL
  commands) — `Replay`'s methods are the stateless query primitives a caller (CLI or otherwise)
  can build a stepping UI on top of; the CLI ships a non-interactive "print rounds" / "print one
  round" cut. A real interactive replay session is deferred, genuinely useful future work once
  something actually needs it.
- **No `observed_by` baked into the event payload itself.** A derived query over `Knows` edges,
  not a second, duplicable copy — see Motivation.
- **No new `KnowledgeStore` trait methods** — `LoggedIn` reuses `append_relationship`/
  `relationships_for`; `all_objects()` (already there) locates the singular log root.
- **No metrics, turning-point detection, or report generation** (Phases 14-16) — real, separate,
  unscoped forks; this RFC only makes their eventual prerequisite (a durable, queryable event log)
  exist.
- **No video/report rendering of a replay** — out of scope, downstream of this.

## Design

### The log (`crates/simulation/src/simulation.rs`)

```rust
pub(crate) fn find_log_object(store: &dyn KnowledgeStore) -> Result<Option<KirObject>, LedgerError> {
    Ok(store.all_objects()?.into_iter().find(|o| o.kind == ObjectKind::Custom("SimulationLog".to_string())))
}

pub(crate) fn find_or_create_log(store: &dyn KnowledgeStore) -> Result<KirId, SimulationError> {
    if let Some(existing) = find_log_object(store)? {
        return Ok(existing.id);
    }
    let log = KirObject::new("simulation-log", ObjectKind::Custom("SimulationLog".to_string()));
    store.append_object(&log)?;
    Ok(log.id)
}
```

`run_round` resolves the log root once per call (not once per action — a small, deliberate
optimization over the naturally-small, throwaway scenario ledgers RFC 0051 already established as
this engine's storage model) and passes it into `execute_action`, which appends one
`Custom("LoggedIn")` relationship (event → log root) after every executed action, unconditionally
— `DoNothing` included, matching the source document's own "every simulation action must produce
an immutable event" instruction literally, not just for message-shaped actions.

### `observed_by` (`crates/simulation/src/simulation.rs`)

```rust
pub fn observed_by(store: &dyn KnowledgeStore, event_id: &KirId) -> Result<Vec<KirId>, SimulationError> {
    Ok(store.relationships_for(event_id)?.into_iter()
        .filter(|r| r.to == *event_id && matches!(&r.kind, RelationshipKind::Custom(k) if k == "Knows"))
        .map(|r| r.from)
        .collect())
}
```

### `Replay` (`crates/simulation/src/replay.rs`)

```rust
pub struct Replay<'a> { /* store + the one log root found via find_log_object */ }

impl<'a> Replay<'a> {
    pub fn open(store: &'a dyn KnowledgeStore) -> Result<Self, ReplayError>; // errors if nothing was ever simulated in this ledger
    pub fn rounds(&self) -> Result<Vec<u32>, ReplayError>;
    pub fn events_in_round(&self, round: u32) -> Result<Vec<KirEvent>, ReplayError>;
    pub fn jump_to(&self, round: u32) -> Result<DateTime<Utc>, ReplayError>;
    pub fn inspect_agent(&self, agent: &KirId, round: u32) -> Result<World, ReplayError>;
    pub fn inspect_graph(&self, entities: &[KirId], round: u32) -> Result<World, ReplayError>;
    pub fn observed_by(&self, event_id: &KirId) -> Result<Vec<KirId>, ReplayError>;
}
```

`jump_to(round)` resolves to that round's own **pre-round** snapshot — `occurred_at` is captured
before that round's actions execute (RFC 0050's own invariant: every agent decides from a shared
snapshot, never from another agent's same-round action), so `inspect_agent`/`inspect_graph(round)`
show what agents actually observed *deciding* round `round`, not what it produced. A later round's
pre-round snapshot naturally includes everything an earlier round completed, since real time only
moves forward between separate `run_round` calls. `inspect_agent`/`inspect_graph` do not
reimplement point-in-time reconstruction — `jump_to` supplies the one timestamp
`Runtime::agent_observation`/`build_world` (RFC 0048/0049) already know how to reconstruct state as
of. `Replay`'s only genuinely new contribution is locating *which*
timestamp corresponds to *which round number*, something nothing in the codebase could answer
before this RFC's log existed.

### CLI (`ekos replay <scenario.yaml> [--round N] [--ledger <path>]`)

Opens the same scenario-scoped ledger `ekos simulate` would (reusing its path-resolution helper),
builds a name map directly from `ledger.all_objects()` (never by calling `load_scenario_from_path`
again, which would re-append scenario data as an unwanted write side effect of what should be a
pure read), and prints either every recorded round or one requested round, each event's actor,
kind, target, and `observed_by` resolved back to names.

## Alternatives Considered

- **Baking `observed_by` into the event payload at creation time** — rejected; duplicates the
  `Knows` edges that already represent exactly this fact, with a real staleness risk if an edge
  were ever added after the event's creation (not possible today, but a needless constraint to
  build in for zero benefit).
- **A per-round marker object instead of one flat per-ledger log** — rejected; `payload["round"]`
  already identifies which round an event belongs to (set since RFC 0050), so a per-round object
  would be a second way to express information the event already carries — `Replay::rounds()`/
  `events_in_round()` filter the one flat log client-side instead.
- **An interactive replay REPL in this RFC** — rejected; the stateless query primitives
  (`rounds`/`events_in_round`/`jump_to`/`inspect_*`) are what an interactive session would be built
  from, and building the session itself with no concrete UI consumer yet risks the same
  "designed but not exercised" pattern this session has avoided seven times running.
- **Re-loading the scenario via `load_scenario_from_path` inside `ekos replay`** — rejected once
  checked: `load_scenario_from_path` always writes (it's a loader), so calling it again during
  replay would re-append every agent/relationship a second time. Reading names straight from the
  ledger's own `all_objects()` is both simpler and correctly read-only.

## Testing

- `simulation` unit tests: `find_or_create_log` is idempotent (a second call finds the same object,
  doesn't create a duplicate); `observed_by` matches `execute_action`'s own visibility fanout
  exactly for public/private/self-only actions (a direct regression test tying RFC 0050's
  `observers_for` to this RFC's query).
- `replay` unit tests: `Replay::open` errors cleanly on a fresh, never-simulated ledger;
  `rounds()`/`events_in_round()` correctly partition a multi-round log; `jump_to` returns the same
  timestamp for every event in a round; `inspect_agent` at a past round returns a `World` that
  does *not* include a `Knows` edge granted only in a later round (proving reconstruction is
  genuinely historical, not just "whatever `agent_observation` returns right now").
- Integration test: run a real 2-3 round `Simulation` (reusing RFC 0050-0053's own patterns), then
  open a *fresh* `Replay` over the same ledger and confirm it reconstructs the same round-by-round
  story the original `RoundResult`s reported — proving the log is a faithful, independent record,
  not just an echo of in-memory state.
- Full workspace: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --check`.

## Acceptance Criteria

- [x] Every executed action (all 12 kinds, including `DoNothing`) is indexed under a single,
      found-or-created `SimulationLog` root per ledger — no new `KnowledgeStore` trait methods.
- [x] `observed_by` matches `execute_action`'s existing visibility fanout exactly, verified by
      `observed_by_matches_visibility_fanout_for_each_bucket`, not just by construction.
- [x] `Replay::open` on a fresh ledger errors cleanly (`NothingSimulatedYet`), not a panic or an
      empty-but-silently-wrong result.
- [x] `Replay::inspect_agent`/`inspect_graph` at a past round reconstruct genuinely historical
      state — `inspect_graph_shows_each_rounds_own_pre_round_snapshot` and
      `replay_matches_live_results_on_{sqlite,fact_ledger}_backend` both verify a later round's
      completed effect is absent from an earlier round's reconstruction.
- [x] `ekos replay` reads only — verified live: entry count in the scenario ledger (`sqlite3
      ledger.db "SELECT COUNT(*) FROM entries"`) was identical (21) before and after running
      `ekos replay`.
- [x] No interactive replay session, no metrics/turning-point/report code — confirmed out of
      scope, not partially started.
- [x] Full workspace `cargo build/test/clippy/fmt` clean.

A real, pre-existing correctness bug was found and fixed along the way, not part of the original
plan: `Ledger::relationships_at` (SQLite backend) and `FactLedger::relationships_at` both only ever
reconstructed a relationship's *current* version, filtered by timestamp — never an actually
historical one, whenever the current version happened to postdate the query time (RFC 0011's
documented limitation, previously "kept for parity" between the two backends rather than fixed).
`Replay::inspect_graph`'s own test surfaced this immediately, since it queries a relationship
(`FormAlliance`'s `Trusts` bump) updated more than once — exactly the case the bug broke. Both
backends fixed to match `object_at`'s already-correct "reconstruct at the point-in-time cut, not
the current version" pattern; regression tests added to `ekos-ledger` directly (not just exercised
indirectly through `Replay`), since the fix benefits every caller of `relationships_at`/
`build_world`/`agent_observation`, not only this RFC.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0054-event-store-and-replay.md` | This RFC, all Acceptance Criteria checked |
| `ekos/crates/ledger/src/lib.rs` | **Real bug fix**: `relationships_at` rewritten for true multi-version point-in-time reconstruction (was: current-version-only, RFC 0011); 1 new regression test |
| `ekos/crates/ledger/src/fact_ledger.rs` | Same fix, `FactLedger` backend; 1 new regression test |
| `ekos/crates/simulation/src/simulation.rs` | `find_log_object`/`find_or_create_log`; `observed_by`; `execute_action` indexes every event under the log; 2 new tests |
| `ekos/crates/simulation/src/replay.rs` | New: `Replay`, `ReplayError`; 4 unit tests |
| `ekos/crates/simulation/src/lib.rs` | `pub mod replay;` + re-exports |
| `ekos/crates/simulation/tests/replay_fixture.rs` | New: multi-round simulate-then-replay integration test, both ledger backends |
| `ekos/crates/cli/src/commands/simulate.rs` | `scenario_ledger_path` made `pub(crate)`, shared with `commands::replay` |
| `ekos/crates/cli/src/commands/replay.rs` | New CLI command |
| `ekos/crates/cli/src/commands/mod.rs` | `pub mod replay;` |
| `ekos/crates/cli/src/bin/ekos.rs` | New `Commands::Replay { scenario, round, ledger }` variant + dispatch |
