//! Read-only state reconstruction from the Semantic Knowledge Ledger.
//!
//! The Runtime is the consumer API of EKOS. AI agents and CLI users go through
//! the Runtime — never directly to the Ledger. The Runtime has no `&mut self`
//! methods that affect the ledger (RFC 0005).

pub mod ai;

use chrono::{DateTime, Utc};
use ekos_kir::{KirEvidence, KirGraph, KirId, KirObject, KirRelationship, RelationshipKind};
use ekos_ledger::{KnowledgeStore, LedgerError};
use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use thiserror::Error;

pub use ai::{AiAnswer, AiError, AiRuntime, AiRuntimeConfig};

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ekos_kir::{
        KirEvidence, KirObject, KirRelationship, ObjectKind, RelationshipKind, SourceLocation,
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
}
