//! RFC 0075 — links `Custom("TransformNode")` Source/Sink nodes (RFC 0027 Transformation IR) to
//! the real `Table`/`Dataset` object they read from or write to. Closes the cross-referencing gap
//! RFC 0074's Data Architecture view found and documented explicitly rather than papering over:
//! a `TransformNode`'s `properties["object_name"]` is the raw identifier as parsed from source
//! text (e.g. `dbo.cust_mstr`), never a relationship to the actual compiled `Table` object —
//! `docs-gen`'s Data Stores and Transformations & Lineage sections had no way to cross-reference
//! each other.

use ekos_kir::{KirGraph, KirId, KirRelationship, ObjectKind, RelationshipKind};
use std::collections::HashMap;
use uuid::Uuid;

/// Deterministic id for a `ReadsFrom`/`WritesTo` edge — matches `crate_topology_analyzer.rs`'s
/// `depends_on_kir_id` (RFC 0072): a `TransformNode` reading or writing one specific `Table` is a
/// boolean fact per `(node, table, direction)` triple, with no legitimate multiplicity the way
/// e.g. `ForeignKey`'s column-pairing has — so this is exactly the shape RFC 0072 already proved
/// safe to dedupe by a stable id. A fresh random id every `commit` run (this crate's other
/// relationship-emitting code, `rollup.rs`, already avoids this for `Contains`) would reproduce
/// the same real duplicate-accumulation bug RFC 0072 fixed elsewhere, this time for a
/// newly-added relationship kind that never had the chance to accumulate duplicates in the first
/// place — worth getting right from the start rather than fixing later.
fn reads_writes_kir_id(kind_label: &str, from: KirId, to: KirId) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("{kind_label}:{from}:{to}").as_bytes(),
    ))
}

/// For every compiled `Custom("TransformNode")` Source/Sink node, looks up
/// `properties["object_name"]` (case-insensitively — the same normalization
/// `sql_analyzer.rs`'s own FK-matching pass already applies internally) against every compiled
/// `Table`/`Dataset` object's name, and links on an **unambiguous** match only: exactly one table
/// with that normalized name. Two unrelated schemas both defining a `customers` table is real and
/// common in multi-system data estates; guessing which one a bare, unqualified `object_name`
/// refers to would silently fabricate a false lineage edge — deliberately not attempted. A name
/// with zero or 2+ matching tables is skipped, not guessed at, mirroring the same "no confidence
/// threshold reliably separates correct fuzzy matches from incorrect ones" judgment RFC 0060 made
/// for identity resolution, applied here without needing that machinery at all (this is exact,
/// case-insensitive string matching, not fuzzy scoring). New relationships cite the
/// `TransformNode`'s own evidence — the same source fragment that already established its
/// `object_name` is the real evidence for "this node reads/writes that table", not a fabricated
/// new record.
pub fn link_transform_nodes_to_tables(graph: &mut KirGraph) {
    let mut tables_by_name: HashMap<String, Vec<KirId>> = HashMap::new();
    for obj in &graph.objects {
        if matches!(obj.kind, ObjectKind::Table | ObjectKind::Dataset) {
            tables_by_name
                .entry(obj.name.to_lowercase())
                .or_default()
                .push(obj.id);
        }
    }
    if tables_by_name.is_empty() {
        return;
    }

    let mut new_relationships = Vec::new();
    for obj in &graph.objects {
        if !matches!(&obj.kind, ObjectKind::Custom(s) if s == "TransformNode") {
            continue;
        }
        let kind_label = match obj.properties.get("node_type").and_then(|v| v.as_str()) {
            Some("Source") => "ReadsFrom",
            Some("Sink") => "WritesTo",
            _ => continue,
        };
        let Some(object_name) = obj.properties.get("object_name").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(matches) = tables_by_name.get(&object_name.to_lowercase()) else {
            continue;
        };
        let [table_id] = matches.as_slice() else {
            continue;
        };

        let mut rel = KirRelationship::new(
            RelationshipKind::Custom(kind_label.to_string()),
            obj.id,
            *table_id,
        );
        rel.id = reads_writes_kir_id(kind_label, obj.id, *table_id);
        rel.evidence = obj.evidence.clone();
        new_relationships.push(rel);
    }

    graph.relationships.extend(new_relationships);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::KirObject;

    fn transform_node(name: &str, node_type: &str, object_name: &str) -> KirObject {
        let mut obj = KirObject::new(name, ObjectKind::Custom("TransformNode".to_string()))
            .with_property("node_type", serde_json::json!(node_type))
            .with_property("object_name", serde_json::json!(object_name));
        obj.evidence.push(KirId::new());
        obj
    }

    #[test]
    fn links_a_source_node_to_the_one_matching_table() {
        let table = KirObject::new("customers", ObjectKind::Table);
        let source = transform_node("etl.sql:0", "Source", "customers");
        let mut graph = KirGraph {
            objects: vec![table.clone(), source.clone()],
            relationships: Vec::new(),
            events: Vec::new(),
            evidence: Vec::new(),
        };

        link_transform_nodes_to_tables(&mut graph);

        assert_eq!(graph.relationships.len(), 1);
        let rel = &graph.relationships[0];
        assert_eq!(rel.kind, RelationshipKind::Custom("ReadsFrom".to_string()));
        assert_eq!(rel.from, source.id);
        assert_eq!(rel.to, table.id);
        assert_eq!(rel.evidence, source.evidence);
    }

    #[test]
    fn links_a_sink_node_with_writes_to_kind() {
        let table = KirObject::new("customer_orders", ObjectKind::Table);
        let sink = transform_node("etl.sql:3", "Sink", "customer_orders");
        let mut graph = KirGraph {
            objects: vec![table.clone(), sink.clone()],
            relationships: Vec::new(),
            events: Vec::new(),
            evidence: Vec::new(),
        };

        link_transform_nodes_to_tables(&mut graph);

        assert_eq!(graph.relationships.len(), 1);
        assert_eq!(
            graph.relationships[0].kind,
            RelationshipKind::Custom("WritesTo".to_string())
        );
        assert_eq!(graph.relationships[0].from, sink.id);
        assert_eq!(graph.relationships[0].to, table.id);
    }

    #[test]
    fn matches_case_insensitively() {
        let table = KirObject::new("Customers", ObjectKind::Table);
        let source = transform_node("etl.sql:0", "Source", "CUSTOMERS");
        let mut graph = KirGraph {
            objects: vec![table, source],
            relationships: Vec::new(),
            events: Vec::new(),
            evidence: Vec::new(),
        };

        link_transform_nodes_to_tables(&mut graph);
        assert_eq!(graph.relationships.len(), 1);
    }

    #[test]
    fn does_not_link_an_ambiguous_name_shared_by_two_tables() {
        let table_a = KirObject::new("customers", ObjectKind::Table);
        let table_b = KirObject::new("customers", ObjectKind::Table);
        let source = transform_node("etl.sql:0", "Source", "customers");
        let mut graph = KirGraph {
            objects: vec![table_a, table_b, source],
            relationships: Vec::new(),
            events: Vec::new(),
            evidence: Vec::new(),
        };

        link_transform_nodes_to_tables(&mut graph);
        assert!(
            graph.relationships.is_empty(),
            "an unqualified name matching two distinct tables must not be guessed at"
        );
    }

    #[test]
    fn does_not_link_when_no_table_matches() {
        let source = transform_node("etl.sql:0", "Source", "nonexistent_table");
        let mut graph = KirGraph {
            objects: vec![source],
            relationships: Vec::new(),
            events: Vec::new(),
            evidence: Vec::new(),
        };

        link_transform_nodes_to_tables(&mut graph);
        assert!(graph.relationships.is_empty());
    }

    #[test]
    fn skips_non_source_sink_transform_nodes_like_filter_and_join() {
        let table = KirObject::new("customers", ObjectKind::Table);
        let filter = KirObject::new("etl.sql:1", ObjectKind::Custom("TransformNode".to_string()))
            .with_property("node_type", serde_json::json!("Filter"))
            .with_property("excerpt", serde_json::json!("status = 'active'"));
        let mut graph = KirGraph {
            objects: vec![table, filter],
            relationships: Vec::new(),
            events: Vec::new(),
            evidence: Vec::new(),
        };

        link_transform_nodes_to_tables(&mut graph);
        assert!(graph.relationships.is_empty());
    }

    #[test]
    fn is_deterministic_across_two_independent_runs() {
        let table = KirObject::new("customers", ObjectKind::Table);
        let source = transform_node("etl.sql:0", "Source", "customers");

        let mut run1 = KirGraph {
            objects: vec![table.clone(), source.clone()],
            relationships: Vec::new(),
            events: Vec::new(),
            evidence: Vec::new(),
        };
        let mut run2 = KirGraph {
            objects: vec![table, source],
            relationships: Vec::new(),
            events: Vec::new(),
            evidence: Vec::new(),
        };

        link_transform_nodes_to_tables(&mut run1);
        link_transform_nodes_to_tables(&mut run2);

        assert_eq!(run1.relationships[0].id, run2.relationships[0].id);
    }

    #[test]
    fn on_no_tables_at_all_does_nothing() {
        let source = transform_node("etl.sql:0", "Source", "customers");
        let mut graph = KirGraph {
            objects: vec![source],
            relationships: Vec::new(),
            events: Vec::new(),
            evidence: Vec::new(),
        };

        link_transform_nodes_to_tables(&mut graph);
        assert!(graph.relationships.is_empty());
    }
}
