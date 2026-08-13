//! Scenario/agent YAML definition and loading (RFC 0051, source document
//! §9.1/§15). Turns human-authored files into the same `KirObject`/
//! `KirRelationship`/`KirEvent` shapes RFC 0049/0050 already established as
//! conventions — no new KIR types, no new `KnowledgeStore` methods.

use crate::action::ActionKind;
use crate::decision::{DecisionEngine, RuleBasedAgent};
use crate::ingest::{IngestError, ingest_sources};
use crate::simulation::SimulationConfig;
use chrono::Utc;
use ekos_kir::{
    EventKind, KirEvent, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
};
use ekos_ledger::{KnowledgeStore, LedgerError};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScenarioError {
    #[error("failed to read {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("failed to parse YAML from {0}: {1}")]
    Yaml(PathBuf, serde_yaml::Error),
    #[error("duplicate name '{0}' in scenario")]
    DuplicateName(String),
    #[error("unknown reference '{0}' (not a scenario-local name and not an existing ledger id)")]
    UnknownReference(String),
    #[error("ledger error: {0}")]
    Ledger(#[from] LedgerError),
    #[error("world.sources ingestion failed: {0}")]
    Ingest(#[from] IngestError),
}

/// `relationships: { <name>: { trust: <value> } }` — source document §9.1.
#[derive(Debug, Clone, Deserialize)]
pub struct RelationshipDef {
    pub trust: f64,
}

/// An agent's own file (or an inline entry in a scenario's `agents:` list) —
/// source document §9.1, minus `knowledge`'s literal event ids (resolved
/// against the scenario's own name registry instead, see [`load_scenario`]).
#[derive(Debug, Clone, Deserialize)]
pub struct AgentDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub goals: Vec<String>,
    #[serde(default)]
    pub fears: Vec<String>,
    #[serde(default)]
    pub beliefs: Vec<String>,
    #[serde(default)]
    pub knowledge: Vec<String>,
    #[serde(default)]
    pub relationships: HashMap<String, RelationshipDef>,
    #[serde(default)]
    pub resources: HashMap<String, f64>,
}

/// A scenario's `agents:` list mixes file-path references and inline
/// definitions (source document uses the file-path style; a fully
/// self-contained scenario is just as legitimate, so both are first-class).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum AgentRef {
    Path(String),
    Inline(Box<AgentDefinition>),
}

fn default_event_kind() -> String {
    "Observed".to_string()
}

/// A scenario-authored seed event — RFC 0051's own minimal substitute for
/// real document ingestion, still useful on its own for a scenario that
/// wants a lightweight, hand-authored fact rather than a whole document
/// (RFC 0055's `world.sources` is the richer alternative — see
/// [`WorldDefinition`]). Gives agents' `knowledge:` lists something concrete
/// to reference without wiring the full compiler pipeline.
#[derive(Debug, Clone, Deserialize)]
pub struct EventDef {
    pub id: String,
    #[serde(default = "default_event_kind")]
    pub kind: String,
    pub subject: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

/// `world: { sources: [...] }` (RFC 0055, source document §15) — real
/// documents to ingest via the actual `ekos-plugin-localdocs` connector and
/// `LocalDocAnalyzerPass`, resolved relative to the scenario file's own
/// directory (matching `agents:`'s convention). An omitted `world:` block
/// (the default) ingests nothing — every pre-RFC-0055 scenario file keeps
/// working unmodified.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorldDefinition {
    #[serde(default)]
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SimulationSettings {
    #[serde(default = "default_rounds")]
    pub rounds: u32,
    /// Parsed for forward compatibility with RFC 0050's Phase 9 conflict
    /// rules; not wired to anything yet — nothing in the current engine is
    /// stochastic (RFC 0050's own Non-goals), so a seed has nothing to seed.
    #[serde(default)]
    pub seed: Option<u64>,
}

fn default_rounds() -> u32 {
    1
}

impl Default for SimulationSettings {
    fn default() -> Self {
        Self {
            rounds: default_rounds(),
            seed: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub agents: Vec<AgentRef>,
    #[serde(default)]
    pub events: Vec<EventDef>,
    #[serde(default)]
    pub world: WorldDefinition,
    #[serde(default)]
    pub simulation: SimulationSettings,
}

/// The result of loading a scenario: everything has already been appended to
/// the store, and what's returned is ready to hand straight to
/// `Simulation::new`.
pub struct LoadedScenario {
    /// Every agent/event name resolved during loading, for a caller that
    /// wants to inspect results by the scenario's own names rather than raw
    /// `KirId`s.
    pub names: HashMap<String, KirId>,
    pub config: SimulationConfig,
    pub engines: HashMap<KirId, Box<dyn DecisionEngine>>,
    pub rounds: u32,
    pub seed: Option<u64>,
}

fn read_yaml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ScenarioError> {
    let content =
        std::fs::read_to_string(path).map_err(|e| ScenarioError::Io(path.to_path_buf(), e))?;
    serde_yaml::from_str(&content).map_err(|e| ScenarioError::Yaml(path.to_path_buf(), e))
}

/// Reads a scenario file plus every file-path agent it references (resolved
/// relative to the scenario file's own directory), ingests any
/// `world.sources` documents (RFC 0055), then loads it.
pub fn load_scenario_from_path(
    store: &dyn KnowledgeStore,
    scenario_path: &Path,
) -> Result<LoadedScenario, ScenarioError> {
    let scenario: ScenarioDefinition = read_yaml(scenario_path)?;
    let base_dir = scenario_path.parent().unwrap_or_else(|| Path::new("."));

    let mut agents = Vec::with_capacity(scenario.agents.len());
    for agent_ref in &scenario.agents {
        match agent_ref {
            AgentRef::Inline(def) => agents.push((**def).clone()),
            AgentRef::Path(rel) => agents.push(read_yaml(&base_dir.join(rel))?),
        }
    }

    let world_objects = ingest_sources(store, base_dir, &scenario.world.sources)?;

    load_scenario(store, &scenario, &agents, &world_objects)
}

/// Resolves `raw` against the scenario's own name registry first, falling
/// back to treating it as a literal `KirId` already present in `store` — the
/// narrow bridge to pre-existing ledger content named in the RFC's Scope.
fn resolve_name(
    store: &dyn KnowledgeStore,
    names: &HashMap<String, KirId>,
    raw: &str,
) -> Result<KirId, ScenarioError> {
    if let Some(id) = names.get(raw) {
        return Ok(*id);
    }
    if let Ok(id) = KirId::from_str(raw)
        && (store.get_object(&id)?.is_some() || store.get_event(&id)?.is_some())
    {
        return Ok(id);
    }
    Err(ScenarioError::UnknownReference(raw.to_string()))
}

/// The pure core: turns an already-parsed scenario plus its already-resolved
/// agent definitions into ledger writes and a ready-to-run
/// `SimulationConfig`. Two passes, the same "assign names before resolving
/// references" shape every KIR-producing compiler pass in this codebase
/// already uses: agents, events, and `world_objects` get registered by name
/// first; only then are `relationships`/`knowledge`/`beliefs` resolved
/// against that registry.
///
/// `world_objects` (RFC 0055) are already-ingested, already-appended-to-
/// `store` `KirObject`s (`Document`/`Section`/`Table`, from
/// `ingest_sources`) — this function only registers their existing names,
/// never re-appends them, which is what keeps this "pure" core testable
/// without real files on disk: a test can hand it a small hand-built
/// `Vec<KirObject>` instead of needing documents in a tempdir.
pub fn load_scenario(
    store: &dyn KnowledgeStore,
    scenario: &ScenarioDefinition,
    agents: &[AgentDefinition],
    world_objects: &[KirObject],
) -> Result<LoadedScenario, ScenarioError> {
    let mut names: HashMap<String, KirId> = HashMap::new();

    // Pass 1a: agents.
    let mut agent_objects: Vec<(AgentDefinition, KirObject)> = Vec::with_capacity(agents.len());
    for def in agents {
        let mut obj = KirObject::new(
            def.name.clone(),
            ObjectKind::Custom("SimulatedAgent".to_string()),
        );
        if let Some(role) = &def.role {
            obj.properties
                .insert("role".to_string(), serde_json::json!(role));
        }
        if !def.goals.is_empty() {
            obj.properties
                .insert("goals".to_string(), serde_json::json!(def.goals));
        }
        if !def.fears.is_empty() {
            obj.properties
                .insert("fears".to_string(), serde_json::json!(def.fears));
        }
        if !def.resources.is_empty() {
            obj.properties
                .insert("resources".to_string(), serde_json::json!(def.resources));
        }
        if names.insert(def.id.clone(), obj.id).is_some() {
            return Err(ScenarioError::DuplicateName(def.id.clone()));
        }
        agent_objects.push((def.clone(), obj));
    }

    // Pass 1b: scenario-authored seed events (this RFC's substitute for
    // world.sources document ingestion). Subjects must be agent names
    // already registered above.
    let mut events: Vec<KirEvent> = Vec::with_capacity(scenario.events.len());
    for ev in &scenario.events {
        let subject = *names
            .get(&ev.subject)
            .ok_or_else(|| ScenarioError::UnknownReference(ev.subject.clone()))?;
        let event = KirEvent {
            id: KirId::new(),
            kind: EventKind::Custom(ev.kind.clone()),
            subject,
            payload: ev.payload.clone(),
            evidence: Vec::new(),
            occurred_at: Utc::now(),
        };
        if names.insert(ev.id.clone(), event.id).is_some() {
            return Err(ScenarioError::DuplicateName(ev.id.clone()));
        }
        events.push(event);
    }

    // Pass 1c: already-ingested world.sources objects (RFC 0055) — already
    // appended to `store` by `ingest_sources`; only their names get
    // registered here, alongside agents and scenario-events.
    for obj in world_objects {
        if names.insert(obj.name.clone(), obj.id).is_some() {
            return Err(ScenarioError::DuplicateName(obj.name.clone()));
        }
    }

    for (_, obj) in &agent_objects {
        store.append_object(obj)?;
    }
    for event in &events {
        store.append_event(event)?;
    }

    // Pass 2: relationships, knowledge, beliefs — every reference resolved
    // against the now-complete name registry (or an existing ledger id).
    for (def, obj) in &agent_objects {
        for (target_name, rel_def) in &def.relationships {
            let target = resolve_name(store, &names, target_name)?;
            let mut rel = KirRelationship::new(
                RelationshipKind::Custom("Trusts".to_string()),
                obj.id,
                target,
            );
            // "confirmed", not "unconfirmed": a scenario author asserting a
            // starting trust value is asserting this simulation's world
            // truth, not a hypothesis pending review — matching RFC 0050's
            // own bump_trust convention. An "unconfirmed" status here would
            // make build_world silently exclude it, breaking FormAlliance's
            // precondition check the moment a scenario tried to use it.
            rel.properties
                .insert("value".to_string(), serde_json::json!(rel_def.trust));
            rel.properties
                .insert("status".to_string(), serde_json::json!("confirmed"));
            store.append_relationship(&rel)?;
        }

        for know_name in &def.knowledge {
            let target = resolve_name(store, &names, know_name)?;
            let rel = KirRelationship::new(
                RelationshipKind::Custom("Knows".to_string()),
                obj.id,
                target,
            );
            store.append_relationship(&rel)?;
        }

        for belief_text in &def.beliefs {
            // RFC 0049's own documented Proposition/Believes convention,
            // exercised here by real parsing code for the first time
            // (devlog_49.md named this as untested).
            let proposition = KirObject::new(
                belief_text.clone(),
                ObjectKind::Custom("Proposition".to_string()),
            );
            store.append_object(&proposition)?;
            let mut rel = KirRelationship::new(
                RelationshipKind::Custom("Believes".to_string()),
                obj.id,
                proposition.id,
            );
            rel.properties
                .insert("status".to_string(), serde_json::json!("unconfirmed"));
            rel.properties
                .insert("confidence".to_string(), serde_json::json!(0.6));
            store.append_relationship(&rel)?;
        }
    }

    let agent_ids: Vec<KirId> = agent_objects.iter().map(|(_, obj)| obj.id).collect();
    let mut engines: HashMap<KirId, Box<dyn DecisionEngine>> = HashMap::new();
    for id in &agent_ids {
        // Every agent defaults to RuleBasedAgent — the only deterministic
        // reference engine that exists (RFC 0050). Per-agent engine
        // selection is real, deferred work (see this RFC's Non-goals).
        engines.insert(*id, Box::new(RuleBasedAgent));
    }

    let config = SimulationConfig {
        agents: agent_ids,
        available_actions: ActionKind::all(),
        // RFC 0052: what used to be parsed-but-ignored (RFC 0051's own
        // devlog named this gap explicitly) now actually drives priority
        // tie-breaking and resource-contention ordering.
        seed: scenario.simulation.seed,
    };

    Ok(LoadedScenario {
        names,
        config,
        engines,
        rounds: scenario.simulation.rounds,
        seed: scenario.simulation.seed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_ledger::Ledger;
    use tempfile::tempdir;

    fn scenario_and_agents() -> (ScenarioDefinition, Vec<AgentDefinition>) {
        let alice = AgentDefinition {
            id: "alice".to_string(),
            name: "Alice".to_string(),
            role: Some("founder".to_string()),
            goals: vec!["support:acme".to_string()],
            fears: vec![],
            beliefs: vec!["bob_wants_to_replace_me".to_string()],
            knowledge: vec!["alice_saw_theft".to_string()],
            relationships: HashMap::from([("bob".to_string(), RelationshipDef { trust: 0.5 })]),
            resources: HashMap::from([("influence".to_string(), 0.8)]),
        };
        let bob = AgentDefinition {
            id: "bob".to_string(),
            name: "Bob".to_string(),
            role: None,
            goals: vec![],
            fears: vec![],
            beliefs: vec![],
            knowledge: vec![],
            relationships: HashMap::new(),
            resources: HashMap::new(),
        };
        let scenario = ScenarioDefinition {
            id: "test_scenario".to_string(),
            name: "Test Scenario".to_string(),
            agents: vec![],
            events: vec![EventDef {
                id: "alice_saw_theft".to_string(),
                kind: default_event_kind(),
                subject: "alice".to_string(),
                payload: serde_json::json!({ "note": "saw it happen" }),
            }],
            world: WorldDefinition::default(),
            simulation: SimulationSettings {
                rounds: 3,
                seed: Some(42),
            },
        };
        (scenario, vec![alice, bob])
    }

    #[test]
    fn agent_properties_carry_role_goals_resources() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let (scenario, agents) = scenario_and_agents();

        let loaded = load_scenario(&ledger, &scenario, &agents, &[]).unwrap();

        let alice_id = loaded.names["alice"];
        let alice_obj = ledger.get_object(&alice_id).unwrap().unwrap();
        assert_eq!(alice_obj.properties["role"], serde_json::json!("founder"));
        assert_eq!(
            alice_obj.properties["goals"],
            serde_json::json!(["support:acme"])
        );
        assert_eq!(
            alice_obj.properties["resources"]["influence"],
            serde_json::json!(0.8)
        );
    }

    #[test]
    fn relationships_resolve_to_confirmed_trusts_edges() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let (scenario, agents) = scenario_and_agents();

        let loaded = load_scenario(&ledger, &scenario, &agents, &[]).unwrap();
        let alice_id = loaded.names["alice"];
        let bob_id = loaded.names["bob"];

        let trusts = ledger
            .relationships_for(&alice_id)
            .unwrap()
            .into_iter()
            .find(|r| {
                r.from == alice_id
                    && r.to == bob_id
                    && matches!(&r.kind, RelationshipKind::Custom(k) if k == "Trusts")
            })
            .expect("Trusts relationship must exist");
        assert_eq!(trusts.properties["value"], serde_json::json!(0.5));
        assert_eq!(trusts.properties["status"], serde_json::json!("confirmed"));
    }

    #[test]
    fn knowledge_resolves_to_a_scenario_authored_event() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let (scenario, agents) = scenario_and_agents();

        let loaded = load_scenario(&ledger, &scenario, &agents, &[]).unwrap();
        let alice_id = loaded.names["alice"];
        let event_id = loaded.names["alice_saw_theft"];

        let knows = ledger
            .relationships_for(&alice_id)
            .unwrap()
            .into_iter()
            .any(|r| {
                r.from == alice_id
                    && r.to == event_id
                    && matches!(&r.kind, RelationshipKind::Custom(k) if k == "Knows")
            });
        assert!(knows, "Alice must Know about her scenario-authored event");
    }

    #[test]
    fn beliefs_reify_as_proposition_plus_believes_claim() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let (scenario, agents) = scenario_and_agents();

        let loaded = load_scenario(&ledger, &scenario, &agents, &[]).unwrap();
        let alice_id = loaded.names["alice"];

        let believes = ledger
            .relationships_for(&alice_id)
            .unwrap()
            .into_iter()
            .find(|r| {
                r.from == alice_id
                    && matches!(&r.kind, RelationshipKind::Custom(k) if k == "Believes")
            })
            .expect("Believes relationship must exist");
        assert_eq!(
            believes.properties["status"],
            serde_json::json!("unconfirmed")
        );

        let proposition = ledger.get_object(&believes.to).unwrap().unwrap();
        assert_eq!(proposition.name, "bob_wants_to_replace_me");
        assert_eq!(
            proposition.kind,
            ObjectKind::Custom("Proposition".to_string())
        );
    }

    #[test]
    fn unknown_reference_is_a_real_error_not_a_dropped_edge() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let mut agent = AgentDefinition {
            id: "alice".to_string(),
            name: "Alice".to_string(),
            role: None,
            goals: vec![],
            fears: vec![],
            beliefs: vec![],
            knowledge: vec!["nobody_by_this_name".to_string()],
            relationships: HashMap::new(),
            resources: HashMap::new(),
        };
        let scenario = ScenarioDefinition {
            id: "s".to_string(),
            name: "S".to_string(),
            agents: vec![],
            events: vec![],
            world: WorldDefinition::default(),
            simulation: SimulationSettings::default(),
        };

        let result = load_scenario(&ledger, &scenario, std::slice::from_mut(&mut agent), &[]);
        let Err(err) = result else {
            panic!("expected an UnknownReference error");
        };
        assert!(
            matches!(err, ScenarioError::UnknownReference(name) if name == "nobody_by_this_name")
        );
    }

    #[test]
    fn duplicate_agent_name_is_a_real_error() {
        let dir = tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let make = |id: &str| AgentDefinition {
            id: id.to_string(),
            name: id.to_string(),
            role: None,
            goals: vec![],
            fears: vec![],
            beliefs: vec![],
            knowledge: vec![],
            relationships: HashMap::new(),
            resources: HashMap::new(),
        };
        let agents = vec![make("alice"), make("alice")];
        let scenario = ScenarioDefinition {
            id: "s".to_string(),
            name: "S".to_string(),
            agents: vec![],
            events: vec![],
            world: WorldDefinition::default(),
            simulation: SimulationSettings::default(),
        };

        let result = load_scenario(&ledger, &scenario, &agents, &[]);
        let Err(err) = result else {
            panic!("expected a DuplicateName error");
        };
        assert!(matches!(err, ScenarioError::DuplicateName(name) if name == "alice"));
    }

    #[test]
    fn agent_ref_untagged_parses_both_path_and_inline() {
        let yaml = r#"
agents:
  - alice.yaml
  - id: bob
    name: Bob
"#;
        #[derive(Deserialize)]
        struct Wrapper {
            agents: Vec<AgentRef>,
        }
        let parsed: Wrapper = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(&parsed.agents[0], AgentRef::Path(p) if p == "alice.yaml"));
        assert!(matches!(&parsed.agents[1], AgentRef::Inline(def) if def.id == "bob"));
    }

    #[test]
    fn scenario_yaml_round_trips_through_the_full_schema() {
        let yaml = r#"
id: open_source_conflict
name: "The Battle for Project X"
events:
  - id: alice_saw_theft
    subject: alice
    payload: { note: "saw it" }
simulation:
  rounds: 5
  seed: 42
"#;
        let scenario: ScenarioDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(scenario.id, "open_source_conflict");
        assert_eq!(scenario.simulation.rounds, 5);
        assert_eq!(scenario.simulation.seed, Some(42));
        assert_eq!(scenario.events[0].kind, "Observed");
    }
}
