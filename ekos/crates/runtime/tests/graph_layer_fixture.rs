//! RFC 0047's own §45-style test fixture: 5 people, 3 organizations, 10
//! events, 15 relationships, 5 claims — load, query, serialize, reload, in
//! both ledger backends. Proves the graph-layer extensions (valid_from/
//! valid_until, generalized is_pending_review, object_history/
//! relationship_history) work end to end without constructing anything
//! world/agent/simulation-shaped: entities are plain `ObjectKind::Person`/
//! `Custom("Organization")` objects, events are plain `KirEvent`s, nothing
//! here is a belief, a goal, or a simulation round.

use chrono::{Duration, Utc};
use ekos_kir::{
    EventKind, KirEvent, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
};
use ekos_ledger::{FactLedger, KnowledgeStore, Ledger};
use ekos_runtime::{ImpactDirection, Runtime};
use tempfile::tempdir;

struct Fixture {
    people: Vec<KirObject>,
    organizations: Vec<KirObject>,
    events: Vec<KirEvent>,
    relationships: Vec<KirRelationship>,
    claims: Vec<KirRelationship>,
}

fn build_fixture() -> Fixture {
    let people: Vec<KirObject> = ["Alice", "Bob", "Charlie", "Diana", "Eve"]
        .iter()
        .map(|name| KirObject::new(*name, ObjectKind::Person))
        .collect();
    let organizations: Vec<KirObject> = ["Acme Corp", "Globex", "Initech"]
        .iter()
        .map(|name| KirObject::new(*name, ObjectKind::Custom("Organization".to_string())))
        .collect();

    let events: Vec<KirEvent> = (0..10)
        .map(|i| KirEvent {
            id: KirId::new(),
            kind: if i % 2 == 0 {
                EventKind::Created
            } else {
                EventKind::Modified
            },
            subject: people[i % people.len()].id,
            payload: serde_json::json!({ "note": format!("event {i}") }),
            evidence: Vec::new(),
            occurred_at: Utc::now(),
        })
        .collect();

    // 15 ordinary, confirmed-fact relationships: each person works for an
    // org, and a handful of person-to-person KNOWS edges. No `status`
    // property anywhere here — compiler-derived facts always pass
    // is_pending_review() unaffected (RFC 0047).
    let mut relationships = Vec::new();
    for (i, person) in people.iter().enumerate() {
        let org = &organizations[i % organizations.len()];
        relationships.push(KirRelationship::new(
            RelationshipKind::Custom("WorksFor".to_string()),
            person.id,
            org.id,
        ));
    }
    for i in 0..people.len() {
        for j in (i + 1)..people.len() {
            if relationships.len() >= 15 {
                break;
            }
            relationships.push(KirRelationship::new(
                RelationshipKind::Custom("Knows".to_string()),
                people[i].id,
                people[j].id,
            ));
        }
    }
    relationships.truncate(15);
    assert_eq!(relationships.len(), 15);

    // 5 claims: unconfirmed, confidence-scored assertions between people —
    // exactly the "hypothesis, not observed fact" shape RFC 0029 proved out
    // for identity candidates, generalized here to arbitrary kinds. Two
    // carry a valid_from/valid_until window.
    let claim_kinds = ["Opposes", "Trusts", "Distrusts", "Supports", "Suspects"];
    let claims: Vec<KirRelationship> = claim_kinds
        .iter()
        .enumerate()
        .map(|(i, kind)| {
            let from = people[i % people.len()].id;
            let to = people[(i + 1) % people.len()].id;
            let mut rel =
                KirRelationship::new(RelationshipKind::Custom(kind.to_string()), from, to);
            rel.properties
                .insert("status".into(), serde_json::json!("unconfirmed"));
            rel.properties.insert(
                "confidence".into(),
                serde_json::json!(0.6 + i as f64 * 0.05),
            );
            if i < 2 {
                let now = Utc::now();
                rel = rel.with_validity(
                    Some(now - Duration::days(1)),
                    Some(now + Duration::days(30)),
                );
            }
            rel
        })
        .collect();
    assert_eq!(claims.len(), 5);

    Fixture {
        people,
        organizations,
        events,
        relationships,
        claims,
    }
}

fn load_fixture(store: &dyn KnowledgeStore, fx: &Fixture) {
    for p in &fx.people {
        store.append_object(p).unwrap();
    }
    for o in &fx.organizations {
        store.append_object(o).unwrap();
    }
    for e in &fx.events {
        store.append_event(e).unwrap();
    }
    for r in &fx.relationships {
        store.append_relationship(r).unwrap();
    }
    for c in &fx.claims {
        store.append_relationship(c).unwrap();
    }
}

/// The actual cross-backend assertions, run once per backend via the two
/// `#[test]` functions below.
fn assert_fixture_behaves(store: &dyn KnowledgeStore, fx: &Fixture) {
    // Load: counts match exactly.
    assert_eq!(store.object_count().unwrap(), 8, "5 people + 3 orgs");
    assert_eq!(
        store.relationship_count().unwrap(),
        20,
        "15 relationships + 5 claims"
    );

    // Query: unconfirmed claims are excluded from traversal, confirmed
    // relationships are not.
    let rt = Runtime::over(store);
    let alice = fx.people[0].id;

    let neighborhood = rt.load_neighborhood(&alice, 1).unwrap();
    for claim in &fx.claims {
        if claim.from == alice || claim.to == alice {
            assert!(
                !neighborhood.relationships.iter().any(|r| r.id == claim.id),
                "unconfirmed claim {} touching Alice must not appear in her neighborhood",
                claim.id
            );
        }
    }
    let works_for = fx
        .relationships
        .iter()
        .find(|r| r.from == alice)
        .expect("Alice has a WorksFor relationship");
    assert!(
        neighborhood
            .relationships
            .iter()
            .any(|r| r.id == works_for.id),
        "a confirmed fact must still appear in the neighborhood"
    );

    // trace_impact must exclude the same unconfirmed claims.
    let hops = rt
        .trace_impact(&alice, ImpactDirection::Dependencies, &[], 3)
        .unwrap();
    for claim in &fx.claims {
        assert!(
            !hops.iter().any(|h| h.via.id == claim.id),
            "trace_impact must not walk unconfirmed claim {}",
            claim.id
        );
    }

    // valid_from/valid_until round-trip through the store.
    let claim_with_validity = &fx.claims[0];
    let reloaded = store
        .get_relationship(&claim_with_validity.id)
        .unwrap()
        .unwrap();
    assert!(reloaded.valid_from.is_some());
    assert!(reloaded.valid_until.is_some());
    assert_eq!(reloaded.valid_from, claim_with_validity.valid_from);

    // object_history / relationship_history: single-version fixture data,
    // so history has exactly one entry each, matching current state.
    let alice_history = rt.object_history(&alice).unwrap();
    assert_eq!(alice_history.len(), 1);
    assert_eq!(alice_history[0].name, "Alice");

    let works_for_history = rt.relationship_history(&works_for.id).unwrap();
    assert_eq!(works_for_history.len(), 1);
    assert_eq!(works_for_history[0].id, works_for.id);

    // Serialize the full current state and reload it — round-trip via
    // Runtime::list_objects/list_relationships (the same shape EKL/MCP
    // consume), not the backend's own storage format.
    let all_objects = rt.list_objects().unwrap();
    let all_relationships = rt.list_relationships().unwrap();
    assert_eq!(all_objects.len(), 8);
    assert_eq!(all_relationships.len(), 20);

    let json = serde_json::to_string(&all_objects).unwrap();
    let reloaded_objects: Vec<KirObject> = serde_json::from_str(&json).unwrap();
    assert_eq!(reloaded_objects.len(), 8);

    let json = serde_json::to_string(&all_relationships).unwrap();
    let reloaded_relationships: Vec<KirRelationship> = serde_json::from_str(&json).unwrap();
    assert_eq!(reloaded_relationships.len(), 20);
    let reloaded_claim = reloaded_relationships
        .iter()
        .find(|r| r.id == claim_with_validity.id)
        .unwrap();
    assert_eq!(reloaded_claim.valid_from, claim_with_validity.valid_from);
    assert!(reloaded_claim.is_pending_review());
}

#[test]
fn fixture_works_on_sqlite_backend() {
    let dir = tempdir().unwrap();
    let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
    let fx = build_fixture();
    load_fixture(&ledger, &fx);
    assert_fixture_behaves(&ledger, &fx);
}

#[test]
fn fixture_works_on_fact_ledger_backend() {
    let dir = tempdir().unwrap();
    let ledger = FactLedger::open(&dir.path().join("factledger")).unwrap();
    let fx = build_fixture();
    load_fixture(&ledger, &fx);
    assert_fixture_behaves(&ledger, &fx);
}

// ── RFC 0048: World, built over this fixture ────────────────────────────────

/// One `Channel` object (RFC 0048's "channels are a naming convention, not a
/// new primitive" — an ordinary `Custom("Channel")` object) carrying resource
/// properties (also a convention, not a new field) — appended alongside the
/// RFC 0047 fixture, not baked into `build_fixture` itself, so RFC 0047's own
/// tests stay untouched by this addition.
fn assert_world_behaves(store: &dyn KnowledgeStore, fx: &Fixture) {
    let channel = KirObject::new("public-forum", ObjectKind::Custom("Channel".to_string()))
        .with_property(
            "resources",
            serde_json::json!({ "capacity": 100, "trust_baseline": 0.5 }),
        );
    store.append_object(&channel).unwrap();

    let alice = fx.people[0].id;
    let bob = fx.people[1].id;
    let scope = [alice, bob, channel.id];

    let rt = Runtime::over(store);
    let world = rt.build_world("act-1", &scope, None, Some(1)).unwrap();

    // All three in-scope objects present; the channel's resources convention
    // round-trips as ordinary properties, nothing new to read it.
    assert_eq!(world.objects.len(), 3);
    let reloaded_channel = world
        .objects
        .iter()
        .find(|o| o.id == channel.id)
        .expect("channel must be in the world");
    assert_eq!(
        reloaded_channel.properties["resources"]["capacity"],
        serde_json::json!(100)
    );

    // The Alice<->Bob relationship (WorksFor doesn't connect them directly,
    // but a Knows edge should, per build_fixture's construction) is in scope;
    // an edge to an org outside the scope must not appear.
    let alice_org_edge = fx
        .relationships
        .iter()
        .find(|r| r.from == alice && fx.organizations.iter().any(|o| o.id == r.to))
        .expect("Alice has a WorksFor edge to an org");
    assert!(
        !world
            .relationships
            .iter()
            .any(|r| r.id == alice_org_edge.id),
        "an edge to an object outside the world's scope must not appear"
    );

    // Serialize/reload the world itself.
    let json = serde_json::to_string(&world).unwrap();
    let back: ekos_runtime::World = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "act-1");
    assert_eq!(back.round, Some(1));
    assert_eq!(back.objects.len(), 3);
}

#[test]
fn world_works_on_sqlite_backend() {
    let dir = tempdir().unwrap();
    let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
    let fx = build_fixture();
    load_fixture(&ledger, &fx);
    assert_world_behaves(&ledger, &fx);
}

#[test]
fn world_works_on_fact_ledger_backend() {
    let dir = tempdir().unwrap();
    let ledger = FactLedger::open(&dir.path().join("factledger")).unwrap();
    let fx = build_fixture();
    load_fixture(&ledger, &fx);
    assert_world_behaves(&ledger, &fx);
}

// ── RFC 0049: agent_observation, built over this fixture's real people ─────

/// The source document's own Alice/Bob/Charlie asymmetric-observation
/// example (§9.3), reusing the fixture's real people and events instead of
/// inventing new ones: Alice gets a `Knows` edge to one event, Bob gets a
/// `Knows` edge to a different one, and each agent's observation must
/// reflect only what it was given -- never the other's, never the whole
/// graph.
fn assert_agent_observation_behaves(store: &dyn KnowledgeStore, fx: &Fixture) {
    let alice = fx.people[0].id;
    let bob = fx.people[1].id;
    let alice_event = fx.events[0].id;
    let bob_event = fx.events[1].id;

    store
        .append_relationship(&KirRelationship::new(
            RelationshipKind::Custom("Knows".to_string()),
            alice,
            alice_event,
        ))
        .unwrap();
    store
        .append_relationship(&KirRelationship::new(
            RelationshipKind::Custom("Knows".to_string()),
            bob,
            bob_event,
        ))
        .unwrap();

    let rt = Runtime::over(store);
    let alice_view = rt.agent_observation(&alice, None).unwrap();
    let bob_view = rt.agent_observation(&bob, None).unwrap();

    assert!(alice_view.events.iter().any(|e| e.id == alice_event));
    assert!(
        !alice_view.events.iter().any(|e| e.id == bob_event),
        "Alice must not observe what only Bob knows"
    );
    assert!(bob_view.events.iter().any(|e| e.id == bob_event));
    assert!(
        !bob_view.events.iter().any(|e| e.id == alice_event),
        "Bob must not observe what only Alice knows"
    );
}

#[test]
fn agent_observation_works_on_sqlite_backend() {
    let dir = tempdir().unwrap();
    let ledger = Ledger::open(&dir.path().join("ledger.db")).unwrap();
    let fx = build_fixture();
    load_fixture(&ledger, &fx);
    assert_agent_observation_behaves(&ledger, &fx);
}

#[test]
fn agent_observation_works_on_fact_ledger_backend() {
    let dir = tempdir().unwrap();
    let ledger = FactLedger::open(&dir.path().join("factledger")).unwrap();
    let fx = build_fixture();
    load_fixture(&ledger, &fx);
    assert_agent_observation_behaves(&ledger, &fx);
}
