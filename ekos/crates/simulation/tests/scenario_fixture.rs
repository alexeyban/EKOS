//! RFC 0051's end-to-end proof: real YAML files on disk, loaded through
//! `load_scenario_from_path` (not the pure `load_scenario` core the
//! `scenario` module's own unit tests exercise), producing a `Simulation`
//! indistinguishable from one built directly in Rust (RFC 0050's own
//! `simulation_fixture.rs`).

use ekos_kir::RelationshipKind;
use ekos_ledger::Ledger;
use ekos_simulation::{ActionKind, Simulation, load_scenario_from_path};
use std::fs;
use tempfile::tempdir;

const SCENARIO_YAML: &str = r#"
id: test_scenario
name: Test Scenario
agents:
  - alice.yaml
  - bob.yaml
events:
  - id: theft
    subject: alice
    payload: { note: "money disappeared" }
simulation:
  rounds: 1
"#;

const ALICE_YAML: &str = r#"
id: alice
name: Alice
role: founder
goals: ["support:Bob"]
knowledge: [theft, bob]
relationships:
  bob: { trust: 0.5 }
resources:
  influence: 0.8
"#;

const BOB_YAML: &str = r#"
id: bob
name: Bob
"#;

#[test]
fn scenario_loads_from_disk_and_runs_end_to_end() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("scenario.yaml"), SCENARIO_YAML).unwrap();
    fs::write(dir.path().join("alice.yaml"), ALICE_YAML).unwrap();
    fs::write(dir.path().join("bob.yaml"), BOB_YAML).unwrap();

    let ledger_dir = tempdir().unwrap();
    let ledger = Ledger::open(&ledger_dir.path().join("ledger.db")).unwrap();

    let loaded = load_scenario_from_path(&ledger, &dir.path().join("scenario.yaml")).unwrap();
    assert_eq!(loaded.rounds, 1);
    assert_eq!(loaded.config.agents.len(), 2);
    assert_eq!(loaded.config.available_actions.len(), 12);

    let alice_id = loaded.names["alice"];
    let bob_id = loaded.names["bob"];
    let theft_id = loaded.names["theft"];

    // Sanity: everything the scenario described actually made it into the
    // ledger before the simulation even runs.
    let theft = ledger.get_event(&theft_id).unwrap().unwrap();
    assert_eq!(theft.subject, alice_id);
    let trusts = ledger
        .relationships_for(&alice_id)
        .unwrap()
        .into_iter()
        .any(|r| {
            r.from == alice_id
                && r.to == bob_id
                && matches!(&r.kind, RelationshipKind::Custom(k) if k == "Trusts")
        });
    assert!(
        trusts,
        "Alice's Trusts(bob) edge must exist before any round runs"
    );

    let sim = Simulation::new(&ledger, loaded.config, loaded.engines);
    let result = sim.run_round(0).unwrap();

    assert!(result.validation_failures.is_empty());
    let alice_decision = &result
        .decisions
        .iter()
        .find(|(id, _)| *id == alice_id)
        .unwrap()
        .1;
    assert_eq!(alice_decision.action.kind, ActionKind::Support);
    assert_eq!(alice_decision.action.target, Some(bob_id));

    let bob_decision = &result
        .decisions
        .iter()
        .find(|(id, _)| *id == bob_id)
        .unwrap()
        .1;
    assert_eq!(bob_decision.action.kind, ActionKind::DoNothing);
}

#[test]
fn inline_agent_definitions_work_without_any_agent_files() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("scenario.yaml"),
        r#"
id: inline_scenario
name: Inline Scenario
agents:
  - id: alice
    name: Alice
  - id: bob
    name: Bob
simulation:
  rounds: 1
"#,
    )
    .unwrap();

    let ledger_dir = tempdir().unwrap();
    let ledger = Ledger::open(&ledger_dir.path().join("ledger.db")).unwrap();

    let loaded = load_scenario_from_path(&ledger, &dir.path().join("scenario.yaml")).unwrap();
    assert_eq!(loaded.config.agents.len(), 2);
    assert_eq!(loaded.rounds, 1);
}

#[test]
fn world_sources_document_is_ingested_and_referenceable_by_an_agents_knowledge() {
    // RFC 0055: a real markdown "report" under world.sources, an agent whose
    // knowledge: references it by the same path string listed there — the
    // full "listed, named the same, resolvable from an agent's knowledge:"
    // loop, proven end to end rather than by construction.
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("reports")).unwrap();
    fs::write(
        dir.path().join("reports/report_01.md"),
        "# Incident Report\n\nAlice witnessed the theft firsthand.",
    )
    .unwrap();
    fs::write(
        dir.path().join("scenario.yaml"),
        r#"
id: world_sources_scenario
name: World Sources Scenario
agents:
  - id: alice
    name: Alice
    knowledge: ["reports/report_01.md"]
world:
  sources: ["reports/report_01.md"]
simulation:
  rounds: 1
"#,
    )
    .unwrap();

    let ledger_dir = tempdir().unwrap();
    let ledger = Ledger::open(&ledger_dir.path().join("ledger.db")).unwrap();

    let loaded = load_scenario_from_path(&ledger, &dir.path().join("scenario.yaml")).unwrap();
    let alice_id = loaded.names["alice"];
    let doc_id = loaded.names["reports/report_01.md"];

    let doc = ledger.get_object(&doc_id).unwrap().unwrap();
    assert_eq!(
        doc.kind,
        ekos_kir::ObjectKind::Custom("Document".to_string())
    );

    let runtime = ekos_runtime::Runtime::over(&ledger);
    let observation = runtime.agent_observation(&alice_id, None).unwrap();
    assert!(
        observation.objects.iter().any(|o| o.id == doc_id),
        "Alice's own observation must include the document her knowledge: references"
    );
}

#[test]
fn unknown_reference_in_a_real_file_produces_a_real_error() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("scenario.yaml"),
        r#"
id: broken_scenario
name: Broken Scenario
agents:
  - id: alice
    name: Alice
    knowledge: [nonexistent]
"#,
    )
    .unwrap();

    let ledger_dir = tempdir().unwrap();
    let ledger = Ledger::open(&ledger_dir.path().join("ledger.db")).unwrap();

    let result = load_scenario_from_path(&ledger, &dir.path().join("scenario.yaml"));
    assert!(
        result.is_err(),
        "an unresolved knowledge reference must not be silently dropped"
    );
}
