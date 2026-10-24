//! RFC 0219 §3.3 — link each `Ticket` to the objects that cite it.
//!
//! RFC 0210's `SchemaChangeSet`s and RFC 0209's `JavaType` / `JavaMethod` anchors record the
//! tickets they name in a `tickets` property; the git analyzer makes one `Ticket` per ticket its
//! commits name. A `TicketFor` edge joins them, by exact key, only when the ticket exists — so a
//! question about a schema change reaches the commits, dates and subject that explain it.

use ekos_kir::{KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

/// The kinds whose `tickets` property is linked.
const CITING_KINDS: &[&str] = &["SchemaChangeSet", "JavaType", "JavaMethod"];

/// Links made, by the citing object's kind.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct TicketLinkStats {
    pub tickets: usize,
    pub links: BTreeMap<String, usize>,
    /// Ticket ids cited by an object but named by no commit.
    pub cited_without_commits: usize,
}

/// `TicketFor` from each ticket to each object citing it.
pub fn link(objects: &[KirObject]) -> (Vec<KirRelationship>, TicketLinkStats) {
    let tickets: HashMap<&str, KirId> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == "Ticket"))
        .map(|o| (o.name.as_str(), o.id))
        .collect();
    let mut stats = TicketLinkStats {
        tickets: tickets.len(),
        ..Default::default()
    };
    let mut out = Vec::new();
    let mut missing = std::collections::BTreeSet::new();
    for o in objects {
        let ObjectKind::Custom(kind) = &o.kind else {
            continue;
        };
        if !CITING_KINDS.contains(&kind.as_str()) {
            continue;
        }
        let cited = o
            .properties
            .get("tickets")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for t in cited.iter().filter_map(Value::as_str) {
            match tickets.get(t) {
                Some(&ticket) => {
                    out.push(KirRelationship::deterministic(
                        RelationshipKind::Custom("TicketFor".into()),
                        ticket,
                        o.id,
                        "",
                    ));
                    *stats.links.entry(kind.clone()).or_default() += 1;
                }
                None => {
                    missing.insert(t.to_string());
                }
            }
        }
    }
    stats.cited_without_commits = missing.len();
    (out, stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(name: &str, kind: &str, tickets: Value) -> KirObject {
        KirObject::new(name, ObjectKind::Custom(kind.into())).with_property("tickets", tickets)
    }

    #[test]
    fn citing_objects_link_to_existing_tickets_only() {
        let ticket = KirObject::new("IDEMPIERE-6203", ObjectKind::Custom("Ticket".into()));
        let objects = vec![
            ticket.clone(),
            obj("set", "SchemaChangeSet", json!(["IDEMPIERE-6203"])),
            obj("m", "JavaMethod", json!(["IDEMPIERE-6203", "IDEMPIERE-1"])),
            obj("c", "SchemaChange", json!(["IDEMPIERE-6203"])),
        ];
        let (rels, stats) = link(&objects);
        assert_eq!(rels.len(), 2, "a SchemaChange is reached through its set");
        assert!(rels.iter().all(|r| r.from == ticket.id));
        assert_eq!(stats.links["SchemaChangeSet"], 1);
        assert_eq!(stats.cited_without_commits, 1);
    }
}
