//! Read-only state reconstruction from the Semantic Knowledge Ledger.
//!
//! The Runtime is the consumer API of EKOS. AI agents and CLI users go through
//! the Runtime — never directly to the Ledger. The Runtime has no `&mut self`
//! methods that affect the ledger (RFC 0005).

pub mod ai;

use chrono::{DateTime, Utc};
use ekos_kir::{
    KirEvent, KirEvidence, KirGraph, KirId, KirObject, KirRelationship, RelationshipKind,
};
use ekos_ledger::{KnowledgeStore, LedgerError};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use thiserror::Error;

pub use ai::{AiAnswer, AiError, AiRuntime, AiRuntimeConfig, ConversationTurn};

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("ledger error: {0}")]
    Ledger(#[from] LedgerError),
}

/// A fully reconstructed view of one object: its current state, all direct
/// relationships, and all associated evidence fragments.
#[derive(Debug, Clone, Serialize)]
pub struct ObjectState {
    pub object: KirObject,
    pub relationships: Vec<KirRelationship>,
    pub evidence: Vec<KirEvidence>,
}

/// Which way to walk edges from the root in [`Runtime::trace_impact`]
/// (RFC 0018).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactDirection {
    /// What depends on this object — follow edges where `rel.to == current`.
    Dependents,
    /// What this object depends on — follow edges where `rel.from == current`.
    Dependencies,
}

/// One node reached while tracing impact: which hop it was found at, the
/// object itself, and the relationship that led to it (RFC 0018).
#[derive(Debug, Clone, Serialize)]
pub struct ImpactHop {
    pub hop: u32,
    pub object: KirObject,
    pub via: KirRelationship,
}

/// A named, time-scoped, entity-scoped view over the ledger (RFC 0048) — the
/// induced subgraph over a chosen set of entities, as of a point in time. Not
/// new storage: computed from existing `KnowledgeStore` queries every time,
/// the same "read-model built by querying the ledger" shape `ObjectState`/
/// `ImpactHop` already are. `resources` (per-entity numeric pools) and
/// `channels` (communication venues) are conventions read off `KirObject`
/// properties and `ObjectKind::Custom("Channel")` respectively — see this
/// RFC's Design section — not separate fields here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct World {
    pub name: String,
    pub time: DateTime<Utc>,
    pub round: Option<u32>,
    pub objects: Vec<KirObject>,
    pub relationships: Vec<KirRelationship>,
    /// Events whose id was in scope and (when `at` was given) had already
    /// occurred by then. Added by RFC 0049 — a real gap found while building
    /// agent knowledge on top of `World`: the source document's own
    /// "knowledge: - event_001" example means a scope id can name an event,
    /// not just an object, and the initial RFC 0048 implementation only ever
    /// tried `get_object`, silently dropping any event id from the world.
    #[serde(default)]
    pub events: Vec<KirEvent>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Read-only view over the Knowledge Ledger. Backend-agnostic since
/// RFC 0016: works over any [`KnowledgeStore`] (the SQLite `Ledger` or the
/// fact engine's `FactLedger`).
pub struct Runtime<'a> {
    ledger: &'a dyn KnowledgeStore,
}

impl<'a> Runtime<'a> {
    pub fn new<S: KnowledgeStore>(ledger: &'a S) -> Self {
        Self { ledger }
    }

    /// Construct from an already-erased store reference.
    pub fn over(ledger: &'a dyn KnowledgeStore) -> Self {
        Self { ledger }
    }

    // ── Current-state queries ─────────────────────────────────────────────────

    /// Load a single object by ID. Returns `None` if unknown.
    pub fn load_object(&self, id: &KirId) -> Result<Option<KirObject>, RuntimeError> {
        Ok(self.ledger.get_object(id)?)
    }

    /// BFS neighbourhood graph up to `depth` hops from `id`.
    ///
    /// - depth=0 → root object only, no relationships
    /// - depth=1 → root + direct neighbours + connecting rels
    /// - depth=N → N hops outward
    pub fn load_neighborhood(&self, id: &KirId, depth: u32) -> Result<KirGraph, RuntimeError> {
        let mut graph = KirGraph::new();
        let mut visited: HashSet<KirId> = HashSet::new();
        let mut queue: VecDeque<(KirId, u32)> = VecDeque::new();

        let root = match self.ledger.get_object(id)? {
            Some(obj) => obj,
            None => return Ok(graph),
        };

        visited.insert(root.id);
        queue.push_back((root.id, 0));
        graph.add_object(root);

        while let Some((current_id, current_depth)) = queue.pop_front() {
            if current_depth >= depth {
                continue;
            }

            for rel in self.ledger.relationships_for(&current_id)? {
                // An unconfirmed RFC 0029 identity candidate is a hypothesis,
                // not a fact — never traversed as if it were a real edge.
                if rel.is_pending_review() {
                    continue;
                }

                let neighbour_id = if rel.from == current_id {
                    rel.to
                } else {
                    rel.from
                };

                // Avoid duplicate relationships.
                if !graph.relationships.iter().any(|r| r.id == rel.id) {
                    graph.add_relationship(rel);
                }

                if !visited.contains(&neighbour_id) {
                    visited.insert(neighbour_id);
                    if let Some(neighbour) = self.ledger.get_object(&neighbour_id)? {
                        graph.add_object(neighbour);
                        queue.push_back((neighbour_id, current_depth + 1));
                    }
                }
            }
        }

        Ok(graph)
    }

    /// Directed, relationship-kind-filtered impact trace (RFC 0018).
    ///
    /// Reuses `load_neighborhood`'s cycle-safe BFS shape but only follows
    /// edges in `direction` and (if `kinds` is non-empty) only edges whose
    /// kind is in `kinds`. Traversal stops past `max_hops`. The root itself
    /// is never included in the output. Like `load_neighborhood`, a
    /// neighbour is recorded the first time it's reached — a second edge
    /// into an already-visited object is not separately reported.
    pub fn trace_impact(
        &self,
        id: &KirId,
        direction: ImpactDirection,
        kinds: &[RelationshipKind],
        max_hops: u32,
    ) -> Result<Vec<ImpactHop>, RuntimeError> {
        let mut hops = Vec::new();
        let mut visited: HashSet<KirId> = HashSet::new();
        let mut queue: VecDeque<(KirId, u32)> = VecDeque::new();

        if self.ledger.get_object(id)?.is_none() {
            return Ok(hops);
        }

        visited.insert(*id);
        queue.push_back((*id, 0));

        while let Some((current_id, current_depth)) = queue.pop_front() {
            if current_depth >= max_hops {
                continue;
            }

            for rel in self.ledger.relationships_for(&current_id)? {
                // Same rationale as `load_neighborhood`: an unreviewed RFC
                // 0029 candidate must not be walked as if it were a real
                // dependency/dependent edge.
                if rel.is_pending_review() {
                    continue;
                }

                let neighbour_id = match direction {
                    ImpactDirection::Dependents if rel.to == current_id => rel.from,
                    ImpactDirection::Dependencies if rel.from == current_id => rel.to,
                    _ => continue,
                };

                if !kinds.is_empty() && !kinds.contains(&rel.kind) {
                    continue;
                }

                if visited.contains(&neighbour_id) {
                    continue;
                }
                visited.insert(neighbour_id);

                if let Some(neighbour) = self.ledger.get_object(&neighbour_id)? {
                    let next_depth = current_depth + 1;
                    hops.push(ImpactHop {
                        hop: next_depth,
                        object: neighbour,
                        via: rel,
                    });
                    queue.push_back((neighbour_id, next_depth));
                }
            }
        }

        Ok(hops)
    }

    /// Reconstruct the full current state of an object: object + relationships + evidence.
    pub fn reconstruct_state(&self, id: &KirId) -> Result<Option<ObjectState>, RuntimeError> {
        let object = match self.ledger.get_object(id)? {
            Some(obj) => obj,
            None => return Ok(None),
        };

        let relationships = self.ledger.relationships_for(id)?;

        let mut evidence = Vec::new();
        for ev_id in &object.evidence {
            if let Some(ev) = self.ledger.get_evidence(ev_id)? {
                evidence.push(ev);
            }
        }

        Ok(Some(ObjectState {
            object,
            relationships,
            evidence,
        }))
    }

    /// Full-text search over object names and kinds. Returns ranked `(id, name)` matches.
    pub fn find_objects(&self, query: &str) -> Result<Vec<(KirId, String)>, RuntimeError> {
        Ok(self.ledger.find_objects(query)?)
    }

    /// Every object currently in the ledger (RFC 0010 — EKL entity enumeration).
    pub fn list_objects(&self) -> Result<Vec<KirObject>, RuntimeError> {
        Ok(self.ledger.all_objects()?)
    }

    /// Every relationship currently in the ledger (RFC 0010 — EKL entity enumeration).
    pub fn list_relationships(&self) -> Result<Vec<KirRelationship>, RuntimeError> {
        Ok(self.ledger.all_relationships()?)
    }

    /// Every object as it existed at or before `at` (RFC 0096 — the bulk
    /// primitive EKL's `AS OF` clause needs; `reconstruct_state_at` below
    /// only ever handled one id at a time).
    pub fn list_objects_at(&self, at: DateTime<Utc>) -> Result<Vec<KirObject>, RuntimeError> {
        Ok(self.ledger.all_objects_at(at)?)
    }

    /// Every relationship as it existed at or before `at` (RFC 0096).
    pub fn list_relationships_at(
        &self,
        at: DateTime<Utc>,
    ) -> Result<Vec<KirRelationship>, RuntimeError> {
        Ok(self.ledger.all_relationships_at(at)?)
    }

    /// All relationships touching `id`, in either direction (RFC 0013 —
    /// `ekos_dependents` impact analysis). Callers filter by `to == id` for
    /// incoming edges (dependents) or `from == id` for dependencies.
    pub fn relationships_for(&self, id: &KirId) -> Result<Vec<KirRelationship>, RuntimeError> {
        Ok(self.ledger.relationships_for(id)?)
    }

    // ── Historical queries ────────────────────────────────────────────────────

    /// Reconstruct the state of an object as it existed at or before `at`.
    /// Returns `None` if the object had not been committed by that point.
    pub fn reconstruct_state_at(
        &self,
        id: &KirId,
        at: DateTime<Utc>,
    ) -> Result<Option<ObjectState>, RuntimeError> {
        let object = match self.ledger.object_at(id, at)? {
            Some(obj) => obj,
            None => return Ok(None),
        };

        let relationships = self.ledger.relationships_at(id, at)?;

        let mut evidence = Vec::new();
        for ev_id in &object.evidence {
            if let Some(ev) = self.ledger.get_evidence(ev_id)? {
                evidence.push(ev);
            }
        }

        Ok(Some(ObjectState {
            object,
            relationships,
            evidence,
        }))
    }

    /// Every historical version of one object's own id, oldest to newest
    /// (RFC 0047) — the full version sequence, not a single point-in-time
    /// cut like `reconstruct_state_at`.
    pub fn object_history(&self, id: &KirId) -> Result<Vec<KirObject>, RuntimeError> {
        Ok(self.ledger.object_history(id)?)
    }

    /// Every historical version of one relationship's own id, oldest to
    /// newest (RFC 0047).
    pub fn relationship_history(&self, id: &KirId) -> Result<Vec<KirRelationship>, RuntimeError> {
        Ok(self.ledger.relationship_history(id)?)
    }

    /// Build a [`World`] (RFC 0048): the induced subgraph over `entity_ids` —
    /// every requested object that exists, plus every relationship between
    /// two objects *both* in the scope (an edge to something outside
    /// `entity_ids` is not part of this world). `at` selects historical state
    /// via the same point-in-time machinery `reconstruct_state_at` already
    /// uses (`None` means current state, `Utc::now()` is recorded as the
    /// world's own `time`). An unconfirmed claim between two in-scope objects
    /// is excluded, same rationale as `load_neighborhood`/`trace_impact`: a
    /// hypothesis isn't part of the world's confirmed state.
    pub fn build_world(
        &self,
        name: impl Into<String>,
        entity_ids: &[KirId],
        at: Option<DateTime<Utc>>,
        round: Option<u32>,
    ) -> Result<World, RuntimeError> {
        let scope: HashSet<KirId> = entity_ids.iter().copied().collect();

        // Object and event ids are disjoint by construction (payload shape
        // decides entity kind — `subject`/`fragment`/`from`+`to` vs. neither
        // — never both), so trying object first and falling back to event
        // for anything it misses is unambiguous, not a guess.
        let mut objects = Vec::new();
        let mut events = Vec::new();
        for id in entity_ids {
            let object = match at {
                Some(t) => self.ledger.object_at(id, t)?,
                None => self.ledger.get_object(id)?,
            };
            match object {
                Some(object) => objects.push(object),
                None => {
                    if let Some(event) = self.ledger.get_event(id)?
                        && at.is_none_or(|t| event.occurred_at <= t)
                    {
                        events.push(event);
                    }
                }
            }
        }

        let mut relationships = Vec::new();
        let mut seen: HashSet<KirId> = HashSet::new();
        for id in entity_ids {
            let rels = match at {
                Some(t) => self.ledger.relationships_at(id, t)?,
                None => self.ledger.relationships_for(id)?,
            };
            for rel in rels {
                if rel.is_pending_review() {
                    continue;
                }
                let other = if rel.from == *id { rel.to } else { rel.from };
                if !scope.contains(&other) {
                    continue;
                }
                if seen.insert(rel.id) {
                    relationships.push(rel);
                }
            }
        }

        Ok(World {
            name: name.into(),
            time: at.unwrap_or_else(Utc::now),
            round,
            objects,
            relationships,
            events,
            metadata: HashMap::new(),
        })
    }

    /// An agent's observation of the world (RFC 0049, source document §9.3):
    /// the world scoped to exactly what `agent_id` has a `Knows` edge to,
    /// plus the agent itself. Built directly on `build_world` — an
    /// observation *is* a world, just scoped by the agent's own knowledge
    /// instead of a caller-supplied id list, so it inherits the same
    /// unconfirmed-claim exclusion and event support for free.
    pub fn agent_observation(
        &self,
        agent_id: &KirId,
        at: Option<DateTime<Utc>>,
    ) -> Result<World, RuntimeError> {
        let rels = match at {
            Some(t) => self.ledger.relationships_at(agent_id, t)?,
            None => self.ledger.relationships_for(agent_id)?,
        };
        let mut scope = vec![*agent_id];
        scope.extend(rels.iter().filter_map(|r| {
            let knows = matches!(&r.kind, RelationshipKind::Custom(k) if k == "Knows")
                && !r.is_pending_review();
            (r.from == *agent_id && knows).then_some(r.to)
        }));
        self.build_world(format!("{agent_id}-observation"), &scope, at, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ekos_kir::{
        EventKind, KirEvidence, KirObject, KirRelationship, ObjectKind, RelationshipKind,
        SourceLocation,
    };
    use tempfile::TempDir;

    fn temp_ledger() -> (ekos_ledger::Ledger, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ledger.db");
        (ekos_ledger::Ledger::open(&path).unwrap(), dir)
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn obj(name: &str) -> KirObject {
        KirObject::new(name, ObjectKind::Table)
    }
    fn fk(from: KirId, to: KirId) -> KirRelationship {
        KirRelationship::new(RelationshipKind::ForeignKey, from, to)
    }
    fn same_as_unconfirmed(from: KirId, to: KirId) -> KirRelationship {
        let mut rel =
            KirRelationship::new(RelationshipKind::Custom("SameAs".to_string()), from, to);
        rel.properties
            .insert("status".into(), serde_json::json!("unconfirmed"));
        rel
    }
    fn same_as_confirmed(from: KirId, to: KirId) -> KirRelationship {
        let mut rel =
            KirRelationship::new(RelationshipKind::Custom("SameAs".to_string()), from, to);
        rel.properties
            .insert("status".into(), serde_json::json!("confirmed"));
        rel
    }
    fn knows(agent: KirId, target: KirId) -> KirRelationship {
        KirRelationship::new(RelationshipKind::Custom("Knows".to_string()), agent, target)
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    #[test]
    fn load_object_unknown_returns_none() {
        let (ledger, _dir) = temp_ledger();
        let rt = Runtime::new(&ledger);
        assert!(rt.load_object(&KirId::new()).unwrap().is_none());
    }

    #[test]
    fn relationships_for_returns_both_directions() {
        let (ledger, _dir) = temp_ledger();
        let orders = obj("orders");
        let customers = obj("customers");
        let items = obj("order_items");
        for o in [&orders, &customers, &items] {
            ledger.append_object(o).unwrap();
        }
        // orders → customers (outgoing) and order_items → orders (incoming).
        ledger
            .append_relationship(&fk(orders.id, customers.id))
            .unwrap();
        ledger
            .append_relationship(&fk(items.id, orders.id))
            .unwrap();

        let rt = Runtime::new(&ledger);
        let rels = rt.relationships_for(&orders.id).unwrap();
        assert_eq!(rels.len(), 2, "both directions must be returned");
        assert!(rels.iter().any(|r| r.to == orders.id && r.from == items.id));
        assert!(
            rels.iter()
                .any(|r| r.from == orders.id && r.to == customers.id)
        );
    }

    #[test]
    fn load_object_known_returns_object() {
        let (ledger, _dir) = temp_ledger();
        let o = obj("orders");
        let id = o.id;
        ledger.append_object(&o).unwrap();
        let rt = Runtime::new(&ledger);
        let found = rt.load_object(&id).unwrap().unwrap();
        assert_eq!(found.name, "orders");
    }

    #[test]
    fn load_neighborhood_depth_0_is_root_only() {
        let (ledger, _dir) = temp_ledger();
        let a = obj("a");
        let b = obj("b");
        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();
        ledger.append_relationship(&fk(a.id, b.id)).unwrap();

        let rt = Runtime::new(&ledger);
        let g = rt.load_neighborhood(&a.id, 0).unwrap();
        assert_eq!(g.objects.len(), 1);
        assert_eq!(g.relationships.len(), 0);
    }

    /// RFC 0029 identity candidates are hypotheses until reviewed — an
    /// unconfirmed `SameAs` edge must not be walked as if it were a real
    /// relationship. A confirmed one, or any structural relationship
    /// (`ForeignKey`, etc.), is unaffected.
    #[test]
    fn load_neighborhood_excludes_unconfirmed_same_as_but_keeps_confirmed() {
        let (ledger, _dir) = temp_ledger();
        let a = obj("a");
        let b = obj("b");
        let c = obj("c");
        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();
        ledger.append_object(&c).unwrap();
        ledger
            .append_relationship(&same_as_unconfirmed(a.id, b.id))
            .unwrap();
        ledger
            .append_relationship(&same_as_confirmed(a.id, c.id))
            .unwrap();

        let rt = Runtime::new(&ledger);
        let g = rt.load_neighborhood(&a.id, 1).unwrap();
        let object_ids: Vec<_> = g.objects.iter().map(|o| o.id).collect();
        assert!(
            !object_ids.contains(&b.id),
            "unconfirmed SameAs must not be traversed"
        );
        assert!(
            object_ids.contains(&c.id),
            "confirmed SameAs must still be traversed"
        );
    }

    #[test]
    fn load_neighborhood_depth_1_returns_direct_neighbours() {
        let (ledger, _dir) = temp_ledger();
        let a = obj("a");
        let b = obj("b");
        let c = obj("c");
        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();
        ledger.append_object(&c).unwrap();
        ledger.append_relationship(&fk(a.id, b.id)).unwrap();
        ledger.append_relationship(&fk(b.id, c.id)).unwrap();

        let rt = Runtime::new(&ledger);
        let g = rt.load_neighborhood(&a.id, 1).unwrap();
        assert_eq!(g.objects.len(), 2, "a and b; c is 2 hops away");
        assert_eq!(g.relationships.len(), 1);
    }

    #[test]
    fn load_neighborhood_depth_2_returns_two_hops() {
        let (ledger, _dir) = temp_ledger();
        let a = obj("a");
        let b = obj("b");
        let c = obj("c");
        let d = obj("d");
        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();
        ledger.append_object(&c).unwrap();
        ledger.append_object(&d).unwrap();
        ledger.append_relationship(&fk(a.id, b.id)).unwrap();
        ledger.append_relationship(&fk(b.id, c.id)).unwrap();
        ledger.append_relationship(&fk(c.id, d.id)).unwrap();

        let rt = Runtime::new(&ledger);
        let g = rt.load_neighborhood(&a.id, 2).unwrap();
        assert_eq!(g.objects.len(), 3, "a, b, c; d is 3 hops away");
        assert_eq!(g.relationships.len(), 2);
    }

    #[test]
    fn load_neighborhood_handles_cycles() {
        let (ledger, _dir) = temp_ledger();
        let a = obj("a");
        let b = obj("b");
        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();
        ledger.append_relationship(&fk(a.id, b.id)).unwrap();
        ledger.append_relationship(&fk(b.id, a.id)).unwrap();

        let rt = Runtime::new(&ledger);
        let g = rt.load_neighborhood(&a.id, 5).unwrap();
        assert_eq!(g.objects.len(), 2, "cycle must not loop forever");
    }

    #[test]
    fn trace_impact_dependents_and_dependencies_are_disjoint() {
        let (ledger, _dir) = temp_ledger();
        let a = obj("a");
        let b = obj("b");
        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();
        // a → b: a depends on b, b has a as a dependent.
        ledger.append_relationship(&fk(a.id, b.id)).unwrap();

        let rt = Runtime::new(&ledger);
        let dependents = rt
            .trace_impact(&b.id, ImpactDirection::Dependents, &[], 5)
            .unwrap();
        assert_eq!(dependents.len(), 1);
        assert_eq!(dependents[0].object.id, a.id);

        let dependencies = rt
            .trace_impact(&b.id, ImpactDirection::Dependencies, &[], 5)
            .unwrap();
        assert!(
            dependencies.is_empty(),
            "b has no outgoing edges, so no dependencies"
        );

        let a_dependencies = rt
            .trace_impact(&a.id, ImpactDirection::Dependencies, &[], 5)
            .unwrap();
        assert_eq!(a_dependencies.len(), 1);
        assert_eq!(a_dependencies[0].object.id, b.id);
    }

    /// The concrete bug found running `ekos_impact` against a real
    /// workspace: after `ekos identity scan`, every unconfirmed `SameAs`
    /// candidate touching an object was being traced as if it were a real
    /// dependent/dependency. An unconfirmed match must be excluded; a
    /// confirmed one must still show up.
    #[test]
    fn trace_impact_excludes_unconfirmed_same_as_but_keeps_confirmed() {
        let (ledger, _dir) = temp_ledger();
        let table = obj("fact_patient_coded_value");
        let unrelated = obj("unrelated_transform_node");
        let reviewed = obj("reviewed_transform_node");
        ledger.append_object(&table).unwrap();
        ledger.append_object(&unrelated).unwrap();
        ledger.append_object(&reviewed).unwrap();
        ledger
            .append_relationship(&same_as_unconfirmed(unrelated.id, table.id))
            .unwrap();
        ledger
            .append_relationship(&same_as_confirmed(reviewed.id, table.id))
            .unwrap();

        let rt = Runtime::new(&ledger);
        let dependents = rt
            .trace_impact(&table.id, ImpactDirection::Dependents, &[], 5)
            .unwrap();
        let ids: Vec<_> = dependents.iter().map(|h| h.object.id).collect();
        assert!(
            !ids.contains(&unrelated.id),
            "unconfirmed SameAs candidate must not count as a dependent"
        );
        assert!(
            ids.contains(&reviewed.id),
            "confirmed SameAs candidate must still count as a dependent"
        );
    }

    #[test]
    fn trace_impact_filters_by_kind() {
        let (ledger, _dir) = temp_ledger();
        let a = obj("a");
        let b = obj("b");
        let c = obj("c");
        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();
        ledger.append_object(&c).unwrap();
        ledger.append_relationship(&fk(a.id, b.id)).unwrap();
        ledger
            .append_relationship(&KirRelationship::new(
                RelationshipKind::CoupledWith,
                a.id,
                c.id,
            ))
            .unwrap();

        let rt = Runtime::new(&ledger);
        let only_fk = rt
            .trace_impact(
                &a.id,
                ImpactDirection::Dependencies,
                &[RelationshipKind::ForeignKey],
                5,
            )
            .unwrap();
        assert_eq!(only_fk.len(), 1);
        assert_eq!(only_fk[0].object.id, b.id);
    }

    #[test]
    fn trace_impact_handles_cycles() {
        let (ledger, _dir) = temp_ledger();
        let a = obj("a");
        let b = obj("b");
        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();
        ledger.append_relationship(&fk(a.id, b.id)).unwrap();
        ledger.append_relationship(&fk(b.id, a.id)).unwrap();

        let rt = Runtime::new(&ledger);
        let hops = rt
            .trace_impact(&a.id, ImpactDirection::Dependencies, &[], 5)
            .unwrap();
        assert_eq!(hops.len(), 1, "cycle must not loop forever");
        assert_eq!(hops[0].object.id, b.id);
    }

    #[test]
    fn trace_impact_max_hops_bounds_traversal() {
        let (ledger, _dir) = temp_ledger();
        let a = obj("a");
        let b = obj("b");
        let c = obj("c");
        let d = obj("d");
        let e = obj("e");
        for o in [&a, &b, &c, &d, &e] {
            ledger.append_object(o).unwrap();
        }
        ledger.append_relationship(&fk(a.id, b.id)).unwrap();
        ledger.append_relationship(&fk(b.id, c.id)).unwrap();
        ledger.append_relationship(&fk(c.id, d.id)).unwrap();
        ledger.append_relationship(&fk(d.id, e.id)).unwrap();

        let rt = Runtime::new(&ledger);
        let hops = rt
            .trace_impact(&a.id, ImpactDirection::Dependencies, &[], 2)
            .unwrap();
        assert_eq!(hops.len(), 2, "only b (hop 1) and c (hop 2) within bound");
        assert!(hops.iter().all(|h| h.hop <= 2));
    }

    #[test]
    fn reconstruct_state_returns_object_rels_evidence() {
        let (ledger, _dir) = temp_ledger();
        let ev = KirEvidence::new(SourceLocation::file("schema.sql"), "CREATE TABLE orders");
        let ev_id = ev.id;
        ledger.append_evidence(&ev).unwrap();

        let mut o = obj("orders");
        o.evidence.push(ev_id);
        let id = o.id;
        let other = obj("customers");
        ledger.append_object(&o).unwrap();
        ledger.append_object(&other).unwrap();
        ledger.append_relationship(&fk(id, other.id)).unwrap();

        let rt = Runtime::new(&ledger);
        let state = rt.reconstruct_state(&id).unwrap().unwrap();
        assert_eq!(state.object.name, "orders");
        assert_eq!(state.relationships.len(), 1);
        assert_eq!(state.evidence.len(), 1);
        assert_eq!(state.evidence[0].fragment, "CREATE TABLE orders");
    }

    #[test]
    fn reconstruct_state_at_before_write_returns_none() {
        let (ledger, _dir) = temp_ledger();
        let o = obj("orders");
        let id = o.id;
        let before = Utc::now() - chrono::Duration::seconds(60);
        ledger.append_object(&o).unwrap();

        let rt = Runtime::new(&ledger);
        assert!(rt.reconstruct_state_at(&id, before).unwrap().is_none());
    }

    #[test]
    fn find_objects_matches_by_name_prefix() {
        let (ledger, _dir) = temp_ledger();
        ledger.append_object(&obj("order_items")).unwrap();
        ledger.append_object(&obj("customers")).unwrap();

        let rt = Runtime::new(&ledger);
        let results = rt.find_objects("order*").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "order_items");
    }

    #[test]
    fn find_objects_no_match_returns_empty() {
        let (ledger, _dir) = temp_ledger();
        ledger.append_object(&obj("customers")).unwrap();

        let rt = Runtime::new(&ledger);
        assert!(rt.find_objects("zzz-nonexistent").unwrap().is_empty());
    }

    #[test]
    fn list_objects_returns_every_object() {
        let (ledger, _dir) = temp_ledger();
        ledger.append_object(&obj("orders")).unwrap();
        ledger.append_object(&obj("customers")).unwrap();

        let rt = Runtime::new(&ledger);
        assert_eq!(rt.list_objects().unwrap().len(), 2);
    }

    #[test]
    fn list_relationships_returns_every_relationship() {
        let (ledger, _dir) = temp_ledger();
        ledger
            .append_relationship(&fk(KirId::new(), KirId::new()))
            .unwrap();
        ledger
            .append_relationship(&fk(KirId::new(), KirId::new()))
            .unwrap();

        let rt = Runtime::new(&ledger);
        assert_eq!(rt.list_relationships().unwrap().len(), 2);
    }

    #[test]
    fn reconstruct_state_at_after_write_returns_state() {
        let (ledger, _dir) = temp_ledger();
        let o = obj("orders");
        let id = o.id;
        ledger.append_object(&o).unwrap();
        let after = Utc::now() + chrono::Duration::seconds(60);

        let rt = Runtime::new(&ledger);
        assert!(rt.reconstruct_state_at(&id, after).unwrap().is_some());
    }

    // ── RFC 0047: object_history / relationship_history ────────────────────

    #[test]
    fn object_history_returns_every_version_through_the_runtime() {
        let (ledger, _dir) = temp_ledger();
        let mut o = obj("orders");
        let id = o.id;
        ledger.append_object(&o).unwrap();
        o.properties
            .insert("row_count".into(), serde_json::json!(1));
        ledger.append_object(&o).unwrap();

        let rt = Runtime::new(&ledger);
        let history = rt.object_history(&id).unwrap();
        assert_eq!(history.len(), 2);
        assert!(!history[0].properties.contains_key("row_count"));
        assert_eq!(history[1].properties["row_count"], serde_json::json!(1));
    }

    #[test]
    fn relationship_history_returns_every_version_through_the_runtime() {
        let (ledger, _dir) = temp_ledger();
        let (a, b) = (KirId::new(), KirId::new());
        let mut rel = fk(a, b);
        let id = rel.id;
        ledger.append_relationship(&rel).unwrap();
        rel.properties.insert("weight".into(), serde_json::json!(5));
        ledger.append_relationship(&rel).unwrap();

        let rt = Runtime::new(&ledger);
        let history = rt.relationship_history(&id).unwrap();
        assert_eq!(history.len(), 2);
        assert!(!history[0].properties.contains_key("weight"));
        assert_eq!(history[1].properties["weight"], serde_json::json!(5));
    }

    // ── RFC 0048: World / build_world ───────────────────────────────────────

    #[test]
    fn build_world_returns_induced_subgraph_only() {
        // a-b in scope, c outside: the a-c edge must not appear even though
        // both a and c individually exist in the ledger.
        let (ledger, _dir) = temp_ledger();
        let a = obj("alice");
        let b = obj("bob");
        let c = obj("charlie");
        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();
        ledger.append_object(&c).unwrap();
        ledger.append_relationship(&fk(a.id, b.id)).unwrap();
        ledger.append_relationship(&fk(a.id, c.id)).unwrap();

        let rt = Runtime::new(&ledger);
        let world = rt
            .build_world("test-world", &[a.id, b.id], None, Some(1))
            .unwrap();

        assert_eq!(world.objects.len(), 2);
        assert_eq!(world.relationships.len(), 1);
        assert_eq!(world.relationships[0].to, b.id);
        assert_eq!(world.round, Some(1));
    }

    #[test]
    fn build_world_excludes_unconfirmed_claims() {
        let (ledger, _dir) = temp_ledger();
        let a = obj("alice");
        let b = obj("bob");
        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();
        ledger.append_relationship(&fk(a.id, b.id)).unwrap();
        ledger
            .append_relationship(&same_as_unconfirmed(a.id, b.id))
            .unwrap();

        let rt = Runtime::new(&ledger);
        let world = rt
            .build_world("test-world", &[a.id, b.id], None, None)
            .unwrap();

        assert_eq!(
            world.relationships.len(),
            1,
            "the unconfirmed claim must not appear in the world"
        );
    }

    #[test]
    fn build_world_skips_unknown_ids() {
        let (ledger, _dir) = temp_ledger();
        let a = obj("alice");
        ledger.append_object(&a).unwrap();

        let rt = Runtime::new(&ledger);
        let world = rt
            .build_world("test-world", &[a.id, KirId::new()], None, None)
            .unwrap();
        assert_eq!(world.objects.len(), 1);
    }

    #[test]
    fn build_world_serializes_and_reloads() {
        let (ledger, _dir) = temp_ledger();
        let a = obj("alice");
        let b = obj("bob");
        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();
        ledger.append_relationship(&fk(a.id, b.id)).unwrap();

        let rt = Runtime::new(&ledger);
        let world = rt
            .build_world("test-world", &[a.id, b.id], None, Some(3))
            .unwrap();

        let json = serde_json::to_string(&world).unwrap();
        let back: World = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "test-world");
        assert_eq!(back.round, Some(3));
        assert_eq!(back.objects.len(), 2);
        assert_eq!(back.relationships.len(), 1);
    }

    #[test]
    fn build_world_includes_events_in_scope() {
        let (ledger, _dir) = temp_ledger();
        let subject = obj("alice");
        ledger.append_object(&subject).unwrap();
        let event = KirEvent {
            id: KirId::new(),
            kind: EventKind::Custom("Accuse".to_string()),
            subject: subject.id,
            payload: serde_json::json!({"target": "bob"}),
            evidence: Vec::new(),
            occurred_at: Utc::now(),
        };
        ledger.append_event(&event).unwrap();

        let rt = Runtime::new(&ledger);
        let world = rt
            .build_world("test-world", &[subject.id, event.id], None, None)
            .unwrap();
        assert_eq!(world.objects.len(), 1);
        assert_eq!(world.events.len(), 1);
        assert_eq!(world.events[0].id, event.id);
    }

    // ── RFC 0049: agent_observation ─────────────────────────────────────────

    #[test]
    fn agent_observation_is_scoped_to_its_own_knows_edges() {
        let (ledger, _dir) = temp_ledger();
        let alice = obj("alice");
        let known = obj("known-thing");
        let unknown = obj("unknown-thing");
        ledger.append_object(&alice).unwrap();
        ledger.append_object(&known).unwrap();
        ledger.append_object(&unknown).unwrap();
        ledger
            .append_relationship(&knows(alice.id, known.id))
            .unwrap();
        // No Knows edge to `unknown` -- it must not appear in the observation
        // even though it exists in the ledger.

        let rt = Runtime::new(&ledger);
        let observation = rt.agent_observation(&alice.id, None).unwrap();

        let object_ids: Vec<KirId> = observation.objects.iter().map(|o| o.id).collect();
        assert!(object_ids.contains(&alice.id), "the agent observes itself");
        assert!(object_ids.contains(&known.id));
        assert!(!object_ids.contains(&unknown.id));
    }

    #[test]
    fn agent_observation_excludes_unconfirmed_knows_edge() {
        // Defense in depth: Knows is specified as always-confirmed by
        // convention, but the exclusion path is still exercised in case a
        // caller ever constructs one with status set anyway.
        let (ledger, _dir) = temp_ledger();
        let alice = obj("alice");
        let maybe_known = obj("maybe-known-thing");
        ledger.append_object(&alice).unwrap();
        ledger.append_object(&maybe_known).unwrap();
        let mut unconfirmed_knows = knows(alice.id, maybe_known.id);
        unconfirmed_knows
            .properties
            .insert("status".into(), serde_json::json!("unconfirmed"));
        ledger.append_relationship(&unconfirmed_knows).unwrap();

        let rt = Runtime::new(&ledger);
        let observation = rt.agent_observation(&alice.id, None).unwrap();
        let object_ids: Vec<KirId> = observation.objects.iter().map(|o| o.id).collect();
        assert!(!object_ids.contains(&maybe_known.id));
    }

    #[test]
    fn two_agents_with_different_knows_edges_observe_different_worlds() {
        // The source document's own Alice/Bob/Charlie asymmetric-observation
        // example, at the Runtime level.
        let (ledger, _dir) = temp_ledger();
        let alice = obj("alice");
        let bob = obj("bob");
        let theft_event = obj("theft-event"); // stand-in "thing that happened"
        ledger.append_object(&alice).unwrap();
        ledger.append_object(&bob).unwrap();
        ledger.append_object(&theft_event).unwrap();
        ledger
            .append_relationship(&knows(alice.id, theft_event.id))
            .unwrap();
        // Bob has no Knows edge to the event at all.

        let rt = Runtime::new(&ledger);
        let alice_view = rt.agent_observation(&alice.id, None).unwrap();
        let bob_view = rt.agent_observation(&bob.id, None).unwrap();

        assert!(alice_view.objects.iter().any(|o| o.id == theft_event.id));
        assert!(
            !bob_view.objects.iter().any(|o| o.id == theft_event.id),
            "Bob never gets what he didn't observe"
        );
    }
}
