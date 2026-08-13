//! `Replay` (RFC 0054, source document §18) — a read-only controller over a
//! *previously simulated* ledger. Does not reimplement point-in-time
//! reconstruction: `jump_to` locates the one timestamp a round's events
//! share, then hands it to `Runtime::agent_observation`/`build_world` (RFC
//! 0048/0049), which already know how to reconstruct state as of any
//! moment. `Replay`'s only genuinely new contribution is answering "which
//! timestamp corresponds to which round," something nothing in this
//! codebase could do before RFC 0054's `SimulationLog` existed.

use crate::simulation::{SimulationError, find_log_object, observed_by};
use chrono::{DateTime, Utc};
use ekos_kir::{KirEvent, KirId, RelationshipKind};
use ekos_ledger::{KnowledgeStore, LedgerError};
use ekos_runtime::{Runtime, RuntimeError, World};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("ledger error: {0}")]
    Ledger(#[from] LedgerError),
    #[error("runtime error: {0}")]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Simulation(#[from] SimulationError),
    #[error("this ledger has no recorded simulation yet (no SimulationLog found)")]
    NothingSimulatedYet,
    #[error("round {0} was never recorded")]
    UnknownRound(u32),
}

pub struct Replay<'a> {
    store: &'a dyn KnowledgeStore,
    log_root: KirId,
}

impl<'a> Replay<'a> {
    /// Opens an existing simulation log — errors, rather than silently
    /// creating one, if this ledger has never recorded a round (`Replay` is
    /// read-only in spirit; only `Simulation::run_round` should ever create
    /// the log root).
    pub fn open(store: &'a dyn KnowledgeStore) -> Result<Self, ReplayError> {
        let log_root = find_log_object(store)?
            .map(|o| o.id)
            .ok_or(ReplayError::NothingSimulatedYet)?;
        Ok(Self { store, log_root })
    }

    fn all_logged_events(&self) -> Result<Vec<KirEvent>, ReplayError> {
        let mut events = Vec::new();
        for rel in self.store.relationships_for(&self.log_root)? {
            if rel.to == self.log_root
                && matches!(&rel.kind, RelationshipKind::Custom(k) if k == "LoggedIn")
                && let Some(event) = self.store.get_event(&rel.from)?
            {
                events.push(event);
            }
        }
        events.sort_by_key(|e| e.occurred_at);
        Ok(events)
    }

    /// Every round number this ledger has ever recorded, ascending —
    /// derived from `payload["round"]` (set by `execute_action` since RFC
    /// 0050), not a second, separately-tracked list.
    pub fn rounds(&self) -> Result<Vec<u32>, ReplayError> {
        let mut rounds: Vec<u32> = self
            .all_logged_events()?
            .iter()
            .filter_map(|e| e.payload.get("round").and_then(|v| v.as_u64()))
            .map(|n| n as u32)
            .collect();
        rounds.sort_unstable();
        rounds.dedup();
        Ok(rounds)
    }

    /// Every event recorded in `round`, in the order they executed.
    pub fn events_in_round(&self, round: u32) -> Result<Vec<KirEvent>, ReplayError> {
        let events: Vec<KirEvent> = self
            .all_logged_events()?
            .into_iter()
            .filter(|e| e.payload.get("round").and_then(|v| v.as_u64()) == Some(u64::from(round)))
            .collect();
        if events.is_empty() {
            return Err(ReplayError::UnknownRound(round));
        }
        Ok(events)
    }

    /// The single timestamp every event in `round` shares — `run_round`
    /// computes `occurred_at` once per round, *before* that round's actions
    /// execute (RFC 0050's own invariant: every agent decides from a shared
    /// pre-round snapshot, never from another agent's same-round action),
    /// and reuses it for every event that round. This is therefore the
    /// timestamp of round `round`'s own **pre-round** snapshot — what its
    /// agents actually observed while deciding — not a post-round one. A
    /// later round's pre-round snapshot naturally includes everything an
    /// earlier round completed, since real time only moves forward between
    /// separate `Simulation::run_round` calls.
    pub fn jump_to(&self, round: u32) -> Result<DateTime<Utc>, ReplayError> {
        let events = self.events_in_round(round)?;
        Ok(events[0].occurred_at)
    }

    /// `agent`'s observation exactly as it was when deciding in `round` —
    /// genuinely historical: a `Knows` edge granted *during or after*
    /// `round` is not visible here, because `jump_to`'s timestamp predates
    /// every write `round` itself made.
    pub fn inspect_agent(&self, agent: &KirId, round: u32) -> Result<World, ReplayError> {
        let at = self.jump_to(round)?;
        let runtime = Runtime::over(self.store);
        Ok(runtime.agent_observation(agent, Some(at))?)
    }

    /// The induced subgraph over `entities` exactly as it was going into
    /// `round` (before `round`'s own actions executed) — a thin wrapper
    /// over `Runtime::build_world`, not a reimplementation of it.
    pub fn inspect_graph(&self, entities: &[KirId], round: u32) -> Result<World, ReplayError> {
        let at = self.jump_to(round)?;
        let runtime = Runtime::over(self.store);
        Ok(runtime.build_world(
            format!("replay-round-{round}"),
            entities,
            Some(at),
            Some(round),
        )?)
    }

    pub fn observed_by(&self, event_id: &KirId) -> Result<Vec<KirId>, ReplayError> {
        Ok(observed_by(self.store, event_id)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Action, ActionKind, AlwaysDoNothing, Decision, DecisionContext, DecisionEngine, Simulation,
        SimulationConfig,
    };
    use ekos_ledger::Ledger;
    use std::collections::HashMap;
    use tempfile::tempdir;

    #[test]
    fn open_errors_cleanly_on_a_fresh_never_simulated_ledger() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let result = Replay::open(&ledger);
        let Err(err) = result else {
            panic!("expected NothingSimulatedYet");
        };
        assert!(matches!(err, ReplayError::NothingSimulatedYet));
    }

    struct AlwaysFormAlliance {
        target: KirId,
    }
    impl DecisionEngine for AlwaysFormAlliance {
        fn decide(&self, _ctx: &DecisionContext) -> Decision {
            Decision {
                action: Action::new(ActionKind::FormAlliance).with_target(self.target),
                reasoning_summary: "always seeks alliance".to_string(),
                confidence: 0.9,
            }
        }
    }

    fn run_two_rounds() -> (tempfile::TempDir, Ledger, KirId, KirId) {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();

        let alice_obj = ekos_kir::KirObject::new(
            "Alice",
            ekos_kir::ObjectKind::Custom("SimulatedAgent".to_string()),
        );
        let bob_obj = ekos_kir::KirObject::new(
            "Bob",
            ekos_kir::ObjectKind::Custom("SimulatedAgent".to_string()),
        );
        let alice = alice_obj.id;
        let bob = bob_obj.id;
        ledger.append_object(&alice_obj).unwrap();
        ledger.append_object(&bob_obj).unwrap();

        let mut trust = ekos_kir::KirRelationship::new(
            RelationshipKind::Custom("Trusts".to_string()),
            alice,
            bob,
        );
        trust
            .properties
            .insert("value".to_string(), serde_json::json!(0.5));
        ledger.append_relationship(&trust).unwrap();
        ledger
            .append_relationship(&ekos_kir::KirRelationship::new(
                RelationshipKind::Custom("Knows".to_string()),
                alice,
                bob,
            ))
            .unwrap();

        let mut engines: HashMap<KirId, Box<dyn DecisionEngine>> = HashMap::new();
        engines.insert(alice, Box::new(AlwaysFormAlliance { target: bob }));
        engines.insert(bob, Box::new(AlwaysDoNothing));

        let config = SimulationConfig {
            agents: vec![alice, bob],
            available_actions: vec![ActionKind::FormAlliance, ActionKind::DoNothing],
            seed: None,
        };
        let sim = Simulation::new(&ledger, config, engines);
        sim.run(2).unwrap();

        (dir, ledger, alice, bob)
    }

    #[test]
    fn rounds_and_events_in_round_partition_a_multi_round_log() {
        let (_dir, ledger, _alice, _bob) = run_two_rounds();
        let replay = Replay::open(&ledger).unwrap();

        assert_eq!(replay.rounds().unwrap(), vec![0, 1]);
        let round0 = replay.events_in_round(0).unwrap();
        let round1 = replay.events_in_round(1).unwrap();
        assert_eq!(round0.len(), 2, "both agents act every round");
        assert_eq!(round1.len(), 2);
    }

    #[test]
    fn jump_to_gives_the_same_timestamp_for_every_event_in_a_round() {
        let (_dir, ledger, _alice, _bob) = run_two_rounds();
        let replay = Replay::open(&ledger).unwrap();

        let events = replay.events_in_round(0).unwrap();
        let at = replay.jump_to(0).unwrap();
        for event in &events {
            assert_eq!(event.occurred_at, at);
        }
    }

    #[test]
    fn inspect_graph_shows_each_rounds_own_pre_round_snapshot() {
        let (_dir, ledger, alice, bob) = run_two_rounds();
        let replay = Replay::open(&ledger).unwrap();

        // jump_to(round) resolves to the same timestamp Simulation::run_round
        // itself used for that round's Observe step (RFC 0050's own
        // invariant: every agent decides from a pre-round snapshot) — so
        // inspect_graph(round=N) shows what agents actually saw *deciding*
        // round N, not what round N produced. FormAlliance bumps
        // Trusts(alice, bob) from 0.5 to 0.7 during round 0's execution and
        // to 0.9 during round 1's — round 0's own pre-round snapshot must
        // therefore still show the original seed value, 0.5; round 1's must
        // show 0.7 (round 0's completed effect, since round 1's Observe step
        // runs strictly after round 0 finished).
        let value_at = |round: u32| {
            let world = replay.inspect_graph(&[alice, bob], round).unwrap();
            let trust = world
                .relationships
                .iter()
                .find(|r| {
                    r.from == alice
                        && r.to == bob
                        && matches!(&r.kind, RelationshipKind::Custom(k) if k == "Trusts")
                })
                .unwrap();
            trust.properties["value"].as_f64().unwrap()
        };

        let value0 = value_at(0);
        assert!(
            (value0 - 0.5).abs() < 1e-9,
            "round 0's own pre-round snapshot must show the original seed value, got {value0}"
        );
        let value1 = value_at(1);
        assert!(
            (value1 - 0.7).abs() < 1e-9,
            "round 1's pre-round snapshot must reflect round 0's completed effect, got {value1}"
        );
    }
}
