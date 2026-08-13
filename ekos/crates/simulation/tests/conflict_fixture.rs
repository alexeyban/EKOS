//! RFC 0052's own integration test: the one genuine same-round collision
//! this engine can currently produce — two agents `PostMessage`-ing a
//! shared `Channel` with capacity for only one. Neither agent can see the
//! other's round-N action during validation (RFC 0050's own invariant,
//! unchanged), so both pass validation against the same pre-round snapshot;
//! only one can actually succeed at execution time, in priority order.

use ekos_kir::{KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use ekos_ledger::{KnowledgeStore, Ledger};
use ekos_simulation::{
    Action, ActionKind, ConflictError, Decision, DecisionContext, DecisionEngine, Simulation,
    SimulationConfig,
};
use std::collections::HashMap;
use tempfile::tempdir;

/// A test-local engine that always posts to a fixed channel, at a fixed
/// confidence — both agents share the same confidence so priority ordering
/// among them is decided entirely by RFC 0052's seeded tie-break, not by
/// `Decision.confidence` differing.
struct AlwaysPostMessage {
    channel: KirId,
}

impl DecisionEngine for AlwaysPostMessage {
    fn decide(&self, _ctx: &DecisionContext) -> Decision {
        Decision {
            action: Action::new(ActionKind::PostMessage).with_target(self.channel),
            reasoning_summary: "always posts to the configured channel".to_string(),
            confidence: 0.7,
        }
    }
}

/// Alice and Bob, a `Channel` with capacity for exactly one post, and a
/// `Knows` edge from each agent to the channel (so `PostMessage`'s target
/// is actually in each agent's own observation — required by
/// `validate_action`'s structural check, unrelated to this RFC).
fn build(store: &dyn KnowledgeStore) -> (KirId, KirId, KirId) {
    let alice = KirObject::new("Alice", ObjectKind::Custom("SimulatedAgent".to_string()));
    let bob = KirObject::new("Bob", ObjectKind::Custom("SimulatedAgent".to_string()));
    let mut channel = KirObject::new("general", ObjectKind::Custom("Channel".to_string()));
    channel.properties.insert(
        "resources".to_string(),
        serde_json::json!({ "capacity": 1.0 }),
    );

    store.append_object(&alice).unwrap();
    store.append_object(&bob).unwrap();
    store.append_object(&channel).unwrap();

    for agent in [alice.id, bob.id] {
        store
            .append_relationship(&KirRelationship::new(
                RelationshipKind::Custom("Knows".to_string()),
                agent,
                channel.id,
            ))
            .unwrap();
    }

    (alice.id, bob.id, channel.id)
}

fn config(alice: KirId, bob: KirId, seed: Option<u64>) -> SimulationConfig {
    SimulationConfig {
        agents: vec![alice, bob],
        available_actions: vec![ActionKind::PostMessage, ActionKind::DoNothing],
        seed,
    }
}

fn engines(alice: KirId, bob: KirId, channel: KirId) -> HashMap<KirId, Box<dyn DecisionEngine>> {
    let mut engines: HashMap<KirId, Box<dyn DecisionEngine>> = HashMap::new();
    engines.insert(alice, Box::new(AlwaysPostMessage { channel }));
    engines.insert(bob, Box::new(AlwaysPostMessage { channel }));
    engines
}

#[test]
fn exactly_one_agent_wins_a_shared_channel_capacity_race() {
    let dir = tempdir().unwrap();
    let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
    let (alice, bob, channel) = build(&ledger);

    let sim = Simulation::new(
        &ledger,
        config(alice, bob, Some(7)),
        engines(alice, bob, channel),
    );
    let result = sim.run_round(0).unwrap();

    assert_eq!(
        result.events.len(),
        1,
        "only one PostMessage should succeed"
    );
    assert_eq!(
        result.conflict_failures.len(),
        1,
        "the loser must be recorded as a conflict, not silently dropped"
    );
    assert!(matches!(
        result.conflict_failures[0].1,
        ConflictError::ChannelCapacityExhausted { .. }
    ));
    assert!(
        result.validation_failures.is_empty(),
        "both decisions were reasonable when made"
    );
}

/// Runs the race once under a given seed and returns the winner's own name
/// (not a raw `KirId` — a fresh ledger means fresh, unpredictable ids each
/// run, so identity has to be compared by name across independent runs).
fn winner_name(seed: u64) -> String {
    let dir = tempdir().unwrap();
    let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
    let (alice, bob, channel) = build(&ledger);
    let sim = Simulation::new(
        &ledger,
        config(alice, bob, Some(seed)),
        engines(alice, bob, channel),
    );
    let result = sim.run_round(0).unwrap();
    let winner_id = result.events[0].subject;
    ledger.get_object(&winner_id).unwrap().unwrap().name
}

#[test]
fn the_same_seed_produces_the_same_winner_across_independent_runs() {
    assert_eq!(winner_name(7), winner_name(7));
}

#[test]
fn different_seeds_can_produce_different_winners() {
    // Empirically confirmed (not guessed): seeds 1 and 2 land on opposite
    // sides of the tie-break shuffle for this two-agent race. A future
    // change to the shuffle algorithm could shift which seeds do this —
    // the point of the test is that the seed influences the outcome at
    // all, not that these exact values are load-bearing forever.
    assert_ne!(winner_name(1), winner_name(2));
}
