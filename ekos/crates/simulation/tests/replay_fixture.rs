//! RFC 0054's own end-to-end proof: run a real multi-round simulation, then
//! open a *fresh* `Replay` over the same ledger and confirm it reconstructs
//! the same round-by-round story the original `RoundResult`s reported —
//! proving the log is a faithful, independent record, not just an echo of
//! in-memory state. Both ledger backends, matching every other fixture in
//! this continuation.

use ekos_kir::{KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use ekos_ledger::{FactLedger, KnowledgeStore, Ledger};
use ekos_simulation::{
    Action, ActionKind, AlwaysDoNothing, Decision, DecisionContext, DecisionEngine, Replay,
    Simulation, SimulationConfig,
};
use std::collections::HashMap;
use tempfile::tempdir;

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

fn seed(store: &dyn KnowledgeStore) -> (KirId, KirId) {
    let alice = KirObject::new("Alice", ObjectKind::Custom("SimulatedAgent".to_string()));
    let bob = KirObject::new("Bob", ObjectKind::Custom("SimulatedAgent".to_string()));
    store.append_object(&alice).unwrap();
    store.append_object(&bob).unwrap();

    let mut trust = KirRelationship::new(
        RelationshipKind::Custom("Trusts".to_string()),
        alice.id,
        bob.id,
    );
    trust
        .properties
        .insert("value".to_string(), serde_json::json!(0.5));
    store.append_relationship(&trust).unwrap();
    store
        .append_relationship(&KirRelationship::new(
            RelationshipKind::Custom("Knows".to_string()),
            alice.id,
            bob.id,
        ))
        .unwrap();

    (alice.id, bob.id)
}

fn run_and_replay(store: &dyn KnowledgeStore) {
    let (alice, bob) = seed(store);

    let mut engines: HashMap<KirId, Box<dyn DecisionEngine>> = HashMap::new();
    engines.insert(alice, Box::new(AlwaysFormAlliance { target: bob }));
    engines.insert(bob, Box::new(AlwaysDoNothing));

    let config = SimulationConfig {
        agents: vec![alice, bob],
        available_actions: vec![ActionKind::FormAlliance, ActionKind::DoNothing],
        seed: None,
    };
    let sim = Simulation::new(store, config, engines);
    let results = sim.run(3).unwrap();

    let replay = Replay::open(store).unwrap();
    assert_eq!(replay.rounds().unwrap(), vec![0, 1, 2]);

    for result in &results {
        let recorded = replay.events_in_round(result.round).unwrap();
        assert_eq!(
            recorded.len(),
            result.events.len(),
            "round {}: the log must record exactly as many events as run_round returned",
            result.round
        );
        let mut recorded_ids: Vec<String> = recorded.iter().map(|e| e.id.to_string()).collect();
        let mut live_ids: Vec<String> = result.events.iter().map(|e| e.id.to_string()).collect();
        recorded_ids.sort();
        live_ids.sort();
        assert_eq!(
            recorded_ids, live_ids,
            "round {}: the log must contain exactly the events run_round produced, not a different set",
            result.round
        );

        for event in &recorded {
            let observers = replay.observed_by(&event.id).unwrap();
            assert!(
                !observers.is_empty(),
                "every logged event must have at least its own actor as an observer"
            );
        }
    }

    // Historical reconstruction across genuinely multiple versions of the
    // same relationship (this is the exact scenario that surfaced the
    // relationships_at bug fixed alongside this RFC): round 0's own
    // pre-round snapshot must show the original seed value; round 2's must
    // show the value after two completed FormAlliance bumps.
    let trust_value_going_into = |round: u32| {
        let world = replay.inspect_graph(&[alice, bob], round).unwrap();
        world
            .relationships
            .iter()
            .find(|r| {
                r.from == alice
                    && r.to == bob
                    && matches!(&r.kind, RelationshipKind::Custom(k) if k == "Trusts")
            })
            .unwrap()
            .properties["value"]
            .as_f64()
            .unwrap()
    };

    assert!(
        (trust_value_going_into(0) - 0.5).abs() < 1e-9,
        "round 0's own pre-round snapshot must show the original seed value"
    );
    assert!(
        (trust_value_going_into(2) - 0.9).abs() < 1e-9,
        "round 2's pre-round snapshot must reflect both of round 0 and round 1's completed bumps"
    );
}

#[test]
fn replay_matches_live_results_on_sqlite_backend() {
    let dir = tempdir().unwrap();
    let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
    run_and_replay(&ledger);
}

#[test]
fn replay_matches_live_results_on_fact_ledger_backend() {
    let dir = tempdir().unwrap();
    let ledger = FactLedger::open(&dir.path().join("factledger")).unwrap();
    run_and_replay(&ledger);
}
