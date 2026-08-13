//! RFC 0050's own integration test: a compact three-agent scenario (Alice,
//! Bob, Charlie — the same naming convention RFC 0047-0049's shared
//! `runtime/tests/graph_layer_fixture.rs` fixture uses, though this is a
//! separate, smaller fixture local to this crate — a cross-crate `tests/`
//! module can't import another crate's integration-test code, only its
//! public library API). Proves one full round of the simulation loop:
//! shared pre-round observation, a real `FormAlliance` precondition +
//! effect, public-action `Knows` fanout, and round-to-round determinism.

use ekos_kir::{KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use ekos_ledger::{FactLedger, KnowledgeStore, Ledger};
use ekos_runtime::{Runtime, World};
use ekos_simulation::{
    Action, ActionKind, AlwaysDoNothing, Decision, DecisionContext, DecisionEngine, RuleBasedAgent,
    Simulation, SimulationConfig,
};
use std::collections::HashMap;
use tempfile::tempdir;

/// A test-local engine that always proposes `FormAlliance` toward a fixed
/// target — proves the loop end to end for an action kind `RuleBasedAgent`
/// doesn't itself understand (it only resolves `support:`/`oppose:` goals).
struct AllianceSeeker {
    target: KirId,
}

impl DecisionEngine for AllianceSeeker {
    fn decide(&self, _ctx: &DecisionContext) -> Decision {
        Decision {
            action: Action::new(ActionKind::FormAlliance).with_target(self.target),
            reasoning_summary: "always seeks alliance with the configured target".to_string(),
            confidence: 0.9,
        }
    }
}

struct Fixture {
    alice: KirId,
    bob: KirId,
    charlie: KirId,
    acme: KirId,
}

/// Alice trusts Bob at 0.5 (clears `FormAlliance`'s `> 0.4` precondition)
/// and knows about Bob (so he's in her scope); Bob knows about Acme and has
/// an `oppose:Acme` goal; Charlie knows about nothing and never acts —
/// present purely to prove public-action `Knows` fanout reaches a
/// non-acting observer.
fn build_and_load(store: &dyn KnowledgeStore) -> Fixture {
    let alice = KirObject::new("Alice", ObjectKind::Custom("SimulatedAgent".to_string()));
    let bob = KirObject::new("Bob", ObjectKind::Custom("SimulatedAgent".to_string()));
    let mut charlie = KirObject::new("Charlie", ObjectKind::Custom("SimulatedAgent".to_string()));
    let acme = KirObject::new("Acme", ObjectKind::Custom("Organization".to_string()));
    charlie
        .properties
        .insert("goals".to_string(), serde_json::json!([]));

    let mut bob_with_goal = bob.clone();
    bob_with_goal
        .properties
        .insert("goals".to_string(), serde_json::json!(["oppose:Acme"]));

    let fx = Fixture {
        alice: alice.id,
        bob: bob_with_goal.id,
        charlie: charlie.id,
        acme: acme.id,
    };

    store.append_object(&alice).unwrap();
    store.append_object(&bob_with_goal).unwrap();
    store.append_object(&charlie).unwrap();
    store.append_object(&acme).unwrap();

    let mut trusts = KirRelationship::new(
        RelationshipKind::Custom("Trusts".to_string()),
        fx.alice,
        fx.bob,
    );
    trusts
        .properties
        .insert("value".to_string(), serde_json::json!(0.5));
    store.append_relationship(&trusts).unwrap();

    store
        .append_relationship(&KirRelationship::new(
            RelationshipKind::Custom("Knows".to_string()),
            fx.alice,
            fx.bob,
        ))
        .unwrap();
    store
        .append_relationship(&KirRelationship::new(
            RelationshipKind::Custom("Knows".to_string()),
            fx.bob,
            fx.acme,
        ))
        .unwrap();

    fx
}

fn engines(fx: &Fixture) -> HashMap<KirId, Box<dyn DecisionEngine>> {
    let mut engines: HashMap<KirId, Box<dyn DecisionEngine>> = HashMap::new();
    engines.insert(fx.alice, Box::new(AllianceSeeker { target: fx.bob }));
    engines.insert(fx.bob, Box::new(RuleBasedAgent));
    engines.insert(fx.charlie, Box::new(AlwaysDoNothing));
    engines
}

fn config(fx: &Fixture) -> SimulationConfig {
    SimulationConfig {
        agents: vec![fx.alice, fx.bob, fx.charlie],
        available_actions: vec![
            ActionKind::FormAlliance,
            ActionKind::Support,
            ActionKind::Oppose,
            ActionKind::DoNothing,
        ],
        seed: None,
    }
}

fn assert_round_behaves(store: &dyn KnowledgeStore, fx: &Fixture) {
    let sim = Simulation::new(store, config(fx), engines(fx));
    let result = sim.run_round(0).unwrap();

    assert!(
        result.validation_failures.is_empty(),
        "no decision in this scenario should fail validation: {:?}",
        result.validation_failures
    );
    assert_eq!(result.decisions.len(), 3);
    assert_eq!(result.events.len(), 3);

    let alice_decision = &result
        .decisions
        .iter()
        .find(|(id, _)| *id == fx.alice)
        .unwrap()
        .1;
    assert_eq!(alice_decision.action.kind, ActionKind::FormAlliance);
    assert_eq!(alice_decision.action.target, Some(fx.bob));

    let bob_decision = &result
        .decisions
        .iter()
        .find(|(id, _)| *id == fx.bob)
        .unwrap()
        .1;
    assert_eq!(bob_decision.action.kind, ActionKind::Oppose);
    assert_eq!(bob_decision.action.target, Some(fx.acme));

    let charlie_decision = &result
        .decisions
        .iter()
        .find(|(id, _)| *id == fx.charlie)
        .unwrap()
        .1;
    assert_eq!(charlie_decision.action.kind, ActionKind::DoNothing);

    // FormAlliance's worked effect: Trusts(Alice -> Bob) bumped by 0.2.
    let trusts = store
        .relationships_for(&fx.alice)
        .unwrap()
        .into_iter()
        .find(|r| {
            r.from == fx.alice
                && r.to == fx.bob
                && matches!(&r.kind, RelationshipKind::Custom(k) if k == "Trusts")
        })
        .expect("Trusts relationship must still exist");
    let value = trusts.properties["value"].as_f64().unwrap();
    assert!(
        (value - 0.7).abs() < 1e-9,
        "expected trust bumped from 0.5 to 0.7, got {value}"
    );

    // Public-action Knows fanout: Charlie took no action himself, but
    // FormAlliance and Oppose are both public, so he must end up knowing
    // about all three of this round's events (the two public ones, plus his
    // own self-only DoNothing).
    let charlie_knows: Vec<_> = store
        .relationships_for(&fx.charlie)
        .unwrap()
        .into_iter()
        .filter(|r| {
            r.from == fx.charlie && matches!(&r.kind, RelationshipKind::Custom(k) if k == "Knows")
        })
        .collect();
    assert_eq!(
        charlie_knows.len(),
        3,
        "Charlie should know about all 3 of this round's events (2 public + his own)"
    );
    for event in &result.events {
        assert!(
            charlie_knows.iter().any(|r| r.to == event.id),
            "Charlie must know about event {}",
            event.id
        );
    }

    // Charlie's own observation must now reflect what he was told about.
    let rt = Runtime::over(store);
    let charlie_view: World = rt.agent_observation(&fx.charlie, None).unwrap();
    assert_eq!(charlie_view.events.len(), 3);
}

#[test]
fn round_behaves_on_sqlite_backend() {
    let dir = tempdir().unwrap();
    let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
    let fx = build_and_load(&ledger);
    assert_round_behaves(&ledger, &fx);
}

#[test]
fn round_behaves_on_fact_ledger_backend() {
    let dir = tempdir().unwrap();
    let ledger = FactLedger::open(&dir.path().join("factledger")).unwrap();
    let fx = build_and_load(&ledger);
    assert_round_behaves(&ledger, &fx);
}

/// Determinism check (RFC 0050 Acceptance Criteria): the same round, run
/// from two independently-loaded but identical starting states, must
/// produce the same decisions and executed action kinds/targets. Timestamps
/// and freshly-generated event/relationship ids are allowed to differ;
/// decision content must not.
#[test]
fn same_round_from_same_starting_state_is_deterministic() {
    let dir_a = tempdir().unwrap();
    let ledger_a = Ledger::open(&dir_a.path().join("ledger.db")).unwrap();
    let fx_a = build_and_load(&ledger_a);
    let sim_a = Simulation::new(&ledger_a, config(&fx_a), engines(&fx_a));
    let result_a = sim_a.run_round(0).unwrap();

    let dir_b = tempdir().unwrap();
    let ledger_b = Ledger::open(&dir_b.path().join("ledger.db")).unwrap();
    let fx_b = build_and_load(&ledger_b);
    let sim_b = Simulation::new(&ledger_b, config(&fx_b), engines(&fx_b));
    let result_b = sim_b.run_round(0).unwrap();

    let shape = |result: &ekos_simulation::RoundResult, fx: &Fixture| {
        let mut by_role: Vec<(&str, ActionKind, bool)> = result
            .decisions
            .iter()
            .map(|(id, decision)| {
                let role = if *id == fx.alice {
                    "alice"
                } else if *id == fx.bob {
                    "bob"
                } else {
                    "charlie"
                };
                let target_matches_expected = match role {
                    "alice" => decision.action.target == Some(fx.bob),
                    "bob" => decision.action.target == Some(fx.acme),
                    _ => decision.action.target.is_none(),
                };
                (role, decision.action.kind, target_matches_expected)
            })
            .collect();
        by_role.sort_by_key(|(role, _, _)| *role);
        by_role
    };

    assert_eq!(shape(&result_a, &fx_a), shape(&result_b, &fx_b));
    assert!(result_a.validation_failures.is_empty());
    assert!(result_b.validation_failures.is_empty());
}
