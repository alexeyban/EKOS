//! RFC 0210 §3.5 — link every `SchemaChange` to the table (or view) it changes.
//!
//! Runs at `ekos commit` beside RFC 0163's procedure lineage, for the same reason: a migration
//! script names a table another file created, and only a whole-ledger read sees both. Matching is
//! by **unique** name, the rule `procedure_lineage` uses — a name matching several objects, or
//! none, is counted and left unlinked, never guessed. Deterministic ids make a re-run append
//! nothing.

use crate::procedure_lineage::NameIndex;
use ekos_kir::{KirGraph, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde_json::Value;

/// change → the table or view it changes.
pub const CHANGES: &str = "Changes";
const CHANGE: &str = "SchemaChange";

/// What one linking run did, for `ekos commit`'s summary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaLinkStats {
    pub changes: usize,
    pub linked: usize,
    /// Changes whose table name matched several objects.
    pub ambiguous: usize,
    /// Changes whose table name matched nothing compiled.
    pub missing: usize,
}

fn is_custom(o: &KirObject, kind: &str) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == kind)
}

/// The relation a change is about: its table, or for a view change the view's own name.
fn target(o: &KirObject) -> Option<&str> {
    let p = |k: &str| o.properties.get(k).and_then(Value::as_str);
    match p("operation") {
        Some("create_view" | "drop_view") => p("name"),
        _ => p("table"),
    }
}

pub fn link_changes(graph: &mut KirGraph) -> SchemaLinkStats {
    let mut relations = NameIndex::default();
    for o in &graph.objects {
        if o.kind == ObjectKind::Table || is_custom(o, "View") {
            relations.add_object(o);
        }
    }
    let mut stats = SchemaLinkStats::default();
    let mut new = Vec::new();
    for o in graph.objects.iter().filter(|o| is_custom(o, CHANGE)) {
        stats.changes += 1;
        let Some(name) = target(o) else {
            continue; // a dropped index or a sequence names no relation
        };
        match relations.resolve(name) {
            Ok((to, _)) => {
                stats.linked += 1;
                new.push(KirRelationship::deterministic(
                    RelationshipKind::Custom(CHANGES.into()),
                    o.id,
                    to,
                    "",
                ));
            }
            Err(true) => stats.ambiguous += 1,
            Err(false) => stats.missing += 1,
        }
    }
    graph.relationships.extend(new);
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::KirId;
    use serde_json::json;

    fn change(op: &str, table: Option<&str>, name: Option<&str>) -> KirObject {
        let mut o = KirObject::new(op, ObjectKind::Custom(CHANGE.into()));
        o.properties.insert("operation".into(), json!(op));
        o.properties.insert("table".into(), json!(table));
        o.properties.insert("name".into(), json!(name));
        o
    }

    #[test]
    fn changes_link_to_their_table_by_unique_name() {
        let table = KirObject::new("AD_WF_Responsible", ObjectKind::Table);
        let view = KirObject::new("RV_X", ObjectKind::Custom("View".into()));
        let twin_a = KirObject::new("dual", ObjectKind::Table);
        let twin_b = KirObject::new("dual", ObjectKind::Custom("View".into()));
        let (tid, vid) = (table.id, view.id);
        let mut g = KirGraph::new();
        for o in [
            table,
            view,
            twin_a,
            twin_b,
            change("add_column", Some("ad_wf_responsible"), None),
            change("create_view", None, Some("RV_X")),
            change("modify_column", Some("dual"), None),
            change("add_column", Some("nowhere"), None),
            change("drop_index", None, Some("i")),
        ] {
            g.add_object(o);
        }
        let stats = link_changes(&mut g);
        assert_eq!(
            stats,
            SchemaLinkStats {
                changes: 5,
                linked: 2,
                ambiguous: 1,
                missing: 1
            }
        );
        let targets: Vec<KirId> = g.relationships.iter().map(|r| r.to).collect();
        assert_eq!(targets, [tid, vid]);
        let again = {
            let mut g2 = g.clone();
            g2.relationships.clear();
            link_changes(&mut g2);
            g2.relationships.iter().map(|r| r.id).collect::<Vec<_>>()
        };
        assert_eq!(
            again,
            g.relationships.iter().map(|r| r.id).collect::<Vec<_>>()
        );
    }
}
