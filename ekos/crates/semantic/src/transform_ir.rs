//! Unified Transformation IR (RFC 0027, Phase 1).
//!
//! The shared target representation every format-specific parser (SQL, Pentaho,
//! stored-procedure embedded SQL — Phases 2/3, not this module) compiles into,
//! so transformation logic recovered from one source format can be diffed
//! against logic recovered from another. See `docs/rfcs/0027-unified-transformation-semantics.md`.
//!
//! This module defines only the IR and its lowering into KIR. No format-specific
//! parser lives here.

use chrono::{DateTime, Utc};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Tests ────────────────────────────────────────────────────────────────────
//
// TDD per the implementation plan: one deterministic-serialization test per
// `TransformNode` variant, written before the variant's lowering logic below.
// Rust resolves items across a whole module regardless of textual order, so
// these can reference `TransformNode` etc. even though the types themselves
// are defined further down this file.

#[cfg(test)]
mod tests {
    use super::*;

    /// Two independently-constructed nodes with identical logical content must
    /// hash identically via `ArtifactId::compute` — the same content-addressing
    /// mechanism `ObservationArtifact`/`KnowledgeArtifact` already rely on
    /// (RFC 0027's "Determinism and content-addressability" section).
    fn content_id(node: &TransformNode) -> ekos_artifact::ArtifactId {
        let value = serde_json::to_value(node).expect("TransformNode must serialize");
        ekos_artifact::ArtifactId::compute(&value)
    }

    #[test]
    fn source_node_serializes_deterministically() {
        let a = TransformNode::Source {
            object_name: "dbo.cust_mstr".into(),
            columns: vec!["id".into(), "name".into()],
        };
        let b = TransformNode::Source {
            object_name: "dbo.cust_mstr".into(),
            columns: vec!["id".into(), "name".into()],
        };
        assert_eq!(content_id(&a), content_id(&b));

        let c = TransformNode::Source {
            object_name: "dbo.cust_mstr".into(),
            columns: vec!["id".into()],
        };
        assert_ne!(content_id(&a), content_id(&c));
    }

    #[test]
    fn filter_node_serializes_deterministically() {
        let a = TransformNode::Filter {
            condition: "status = 'active'".into(),
        };
        let b = TransformNode::Filter {
            condition: "status = 'active'".into(),
        };
        assert_eq!(content_id(&a), content_id(&b));

        let c = TransformNode::Filter {
            condition: "status = 'inactive'".into(),
        };
        assert_ne!(content_id(&a), content_id(&c));
    }

    #[test]
    fn join_node_serializes_deterministically() {
        let a = TransformNode::Join {
            left: NodeId(0),
            right: NodeId(1),
            keys: vec![("id".into(), "customer_id".into())],
            kind: JoinKind::Inner,
        };
        let b = TransformNode::Join {
            left: NodeId(0),
            right: NodeId(1),
            keys: vec![("id".into(), "customer_id".into())],
            kind: JoinKind::Inner,
        };
        assert_eq!(content_id(&a), content_id(&b));

        let c = TransformNode::Join {
            left: NodeId(0),
            right: NodeId(1),
            keys: vec![("id".into(), "customer_id".into())],
            kind: JoinKind::Left,
        };
        assert_ne!(content_id(&a), content_id(&c));
    }

    #[test]
    fn aggregate_node_serializes_deterministically() {
        let a = TransformNode::Aggregate {
            group_by: vec!["region".into()],
            aggs: vec![AggExpr {
                output: "total".into(),
                func: "sum".into(),
                arg: "amount".into(),
            }],
        };
        let b = TransformNode::Aggregate {
            group_by: vec!["region".into()],
            aggs: vec![AggExpr {
                output: "total".into(),
                func: "sum".into(),
                arg: "amount".into(),
            }],
        };
        assert_eq!(content_id(&a), content_id(&b));

        let c = TransformNode::Aggregate {
            group_by: vec!["region".into()],
            aggs: vec![AggExpr {
                output: "total".into(),
                func: "avg".into(),
                arg: "amount".into(),
            }],
        };
        assert_ne!(content_id(&a), content_id(&c));
    }

    #[test]
    fn calculate_node_serializes_deterministically() {
        let a = TransformNode::Calculate {
            output: "full_name".into(),
            expr: "first_name || ' ' || last_name".into(),
        };
        let b = TransformNode::Calculate {
            output: "full_name".into(),
            expr: "first_name || ' ' || last_name".into(),
        };
        assert_eq!(content_id(&a), content_id(&b));

        let c = TransformNode::Calculate {
            output: "full_name".into(),
            expr: "last_name || ', ' || first_name".into(),
        };
        assert_ne!(content_id(&a), content_id(&c));
    }

    #[test]
    fn sink_node_serializes_deterministically() {
        let a = TransformNode::Sink {
            object_name: "gold.dim_customer".into(),
            columns: vec!["id".into(), "full_name".into()],
        };
        let b = TransformNode::Sink {
            object_name: "gold.dim_customer".into(),
            columns: vec!["id".into(), "full_name".into()],
        };
        assert_eq!(content_id(&a), content_id(&b));

        let c = TransformNode::Sink {
            object_name: "gold.dim_customer_v2".into(),
            columns: vec!["id".into(), "full_name".into()],
        };
        assert_ne!(content_id(&a), content_id(&c));
    }

    #[test]
    fn unmapped_node_serializes_deterministically() {
        let a = TransformNode::Unmapped {
            raw: "<step><type>Unknown</type></step>".into(),
            reason: "unrecognized step type".into(),
        };
        let b = TransformNode::Unmapped {
            raw: "<step><type>Unknown</type></step>".into(),
            reason: "unrecognized step type".into(),
        };
        assert_eq!(content_id(&a), content_id(&b));

        let c = TransformNode::Unmapped {
            raw: "<step><type>Unknown</type></step>".into(),
            reason: "control flow present, not modeled".into(),
        };
        assert_ne!(content_id(&a), content_id(&c));
    }

    fn origin() -> TransformOrigin {
        TransformOrigin {
            source_path: "jobs/load_customers.ktr".into(),
            source_kind: "pentaho-ktr".into(),
            extracted_at: DateTime::parse_from_rfc3339("2026-08-04T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        }
    }

    fn sample_graph() -> TransformGraph {
        TransformGraph {
            nodes: vec![
                TransformNode::Source {
                    object_name: "dbo.cust_mstr".into(),
                    columns: vec!["id".into(), "status".into()],
                },
                TransformNode::Filter {
                    condition: "status = 'active'".into(),
                },
                TransformNode::Sink {
                    object_name: "gold.dim_customer".into(),
                    columns: vec!["id".into()],
                },
            ],
            edges: vec![(NodeId(0), NodeId(1)), (NodeId(1), NodeId(2))],
            origin: origin(),
        }
    }

    #[test]
    fn transform_node_kir_id_is_stable_across_repeated_lowering() {
        let g1 = sample_graph();
        let g2 = sample_graph();
        let id1 = transform_node_kir_id(&g1.origin, 0);
        let id2 = transform_node_kir_id(&g2.origin, 0);
        assert_eq!(
            id1, id2,
            "same (origin, node_index) must yield the same KirId"
        );
    }

    #[test]
    fn transform_node_kir_id_differs_by_index_and_source_path() {
        let g = sample_graph();
        assert_ne!(
            transform_node_kir_id(&g.origin, 0),
            transform_node_kir_id(&g.origin, 1),
            "distinct node indices within one graph must not collide"
        );

        let mut other = g.origin.clone();
        other.source_path = "jobs/other.ktr".into();
        assert_ne!(
            transform_node_kir_id(&g.origin, 0),
            transform_node_kir_id(&other, 0),
            "distinct source paths must not collide even at the same node index"
        );
    }

    #[test]
    fn lower_to_kir_produces_one_object_per_node_with_evidence() {
        let graph = sample_graph();
        let kir = lower_to_kir(&graph);

        assert_eq!(kir.objects.len(), 3);
        assert_eq!(kir.evidence.len(), 3, "every node gets its own evidence");
        for obj in &kir.objects {
            assert_eq!(obj.kind, ObjectKind::Custom("TransformNode".into()));
            assert_eq!(obj.evidence.len(), 1);
        }
    }

    #[test]
    fn lower_to_kir_sets_node_type_property() {
        let graph = sample_graph();
        let kir = lower_to_kir(&graph);

        let node_types: Vec<String> = kir
            .objects
            .iter()
            .map(|o| {
                o.properties["node_type"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string()
            })
            .collect();
        assert_eq!(node_types, vec!["Source", "Filter", "Sink"]);
    }

    #[test]
    fn lower_to_kir_indexes_filter_condition_as_excerpt() {
        let graph = sample_graph();
        let kir = lower_to_kir(&graph);
        let filter_obj = &kir.objects[1];
        assert_eq!(filter_obj.indexed_content(), "status = 'active'");
    }

    #[test]
    fn lower_to_kir_produces_feeds_into_edges_matching_graph_edges() {
        let graph = sample_graph();
        let kir = lower_to_kir(&graph);

        assert_eq!(kir.relationships.len(), 2);
        for rel in &kir.relationships {
            assert_eq!(rel.kind, RelationshipKind::Custom("FeedsInto".into()));
        }
        assert_eq!(kir.relationships[0].from, kir.objects[0].id);
        assert_eq!(kir.relationships[0].to, kir.objects[1].id);
        assert_eq!(kir.relationships[1].from, kir.objects[1].id);
        assert_eq!(kir.relationships[1].to, kir.objects[2].id);
    }

    #[test]
    fn lower_to_kir_is_idempotent_across_repeated_runs() {
        let kir1 = lower_to_kir(&sample_graph());
        let kir2 = lower_to_kir(&sample_graph());

        let ids1: Vec<KirId> = kir1.objects.iter().map(|o| o.id).collect();
        let ids2: Vec<KirId> = kir2.objects.iter().map(|o| o.id).collect();
        assert_eq!(
            ids1, ids2,
            "re-lowering an unchanged TransformGraph must produce identical object ids"
        );
    }

    /// RFC 0027's "Append-only ledger fit" acceptance criterion: re-lowering
    /// an unchanged `TransformGraph` and re-appending must be recognized as
    /// "no logical change" (a no-op), while a genuine change to the same
    /// logical node (unchanged `KirId`, different content) must be recognized
    /// as a new version — never a silent duplicate, never an in-place mutation.
    #[test]
    fn transform_nodes_round_trip_through_ledger_versioning() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = ekos_ledger::Ledger::open(&dir.path().join("ledger.db")).unwrap();

        let kir1 = lower_to_kir(&sample_graph());
        for obj in &kir1.objects {
            let is_new = ledger.append_object(obj).unwrap();
            assert!(is_new, "first append of any object must be new");
        }

        // Re-lowering the identical graph must produce identical KirIds and
        // identical content, so re-appending is a no-op.
        let kir2 = lower_to_kir(&sample_graph());
        for obj in &kir2.objects {
            let is_new = ledger.append_object(obj).unwrap();
            assert!(
                !is_new,
                "re-appending an unchanged TransformNode must be a no-op, not a new version"
            );
        }

        // A genuine change (different Filter condition) at the same logical
        // node (same source_path, same node_index) must land as a new
        // version at the same KirId, never a silent duplicate.
        let mut changed = sample_graph();
        changed.nodes[1] = TransformNode::Filter {
            condition: "status = 'archived'".into(),
        };
        let kir3 = lower_to_kir(&changed);
        assert_eq!(
            kir3.objects[1].id, kir1.objects[1].id,
            "same (origin, node_index) must keep the same KirId across a content change"
        );
        let is_new = ledger.append_object(&kir3.objects[1]).unwrap();
        assert!(
            is_new,
            "a genuine content change at a stable KirId must be recorded as a new version"
        );
    }

    #[test]
    fn lower_to_kir_unmapped_node_preserves_raw_and_reason() {
        let graph = TransformGraph {
            nodes: vec![TransformNode::Unmapped {
                raw: "<step><type>WeirdStep</type></step>".into(),
                reason: "unrecognized step type".into(),
            }],
            edges: vec![],
            origin: origin(),
        };
        let kir = lower_to_kir(&graph);

        assert_eq!(kir.objects.len(), 1);
        let obj = &kir.objects[0];
        assert_eq!(obj.properties["node_type"], "Unmapped");
        assert_eq!(obj.properties["raw"], "<step><type>WeirdStep</type></step>");
        assert_eq!(obj.properties["reason"], "unrecognized step type");
    }
}

// ── The IR ───────────────────────────────────────────────────────────────────

/// Local index into a single `TransformGraph::nodes`. Not a `KirId` — graph-local
/// only, meaningless outside the `TransformGraph` it was produced by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeId(pub u32);

/// The kind of a `Join` node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
    Cross,
}

/// One aggregate expression within an `Aggregate` node, e.g. `SUM(amount) AS total`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AggExpr {
    pub output: String,
    pub func: String,
    pub arg: String,
}

/// A single node in a Transformation IR graph. Every format-specific parser
/// (SQL, Pentaho, stored-procedure embedded SQL) compiles into this shared
/// vocabulary so graphs from different source formats can be diffed against
/// each other (RFC 0027).
///
/// `Filter.condition`/`Calculate.expr` are kept as raw source text rather than
/// a typed expression AST — deliberate, see RFC 0027's "The IR" section: a
/// shared cross-format expression AST reconciling SQL/Pentaho/T-SQL/PL-pgSQL
/// grammars is a large, separate project with no immediate consumer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "node_type")]
pub enum TransformNode {
    /// A read from a table/view/file. `object_name` is the raw identifier as
    /// written in the source (e.g. `dbo.cust_mstr`) — resolving it to a
    /// concrete cross-system object is Phase 4's job (identity resolution),
    /// not this parsing layer's.
    Source {
        object_name: String,
        columns: Vec<String>,
    },
    /// A row-filtering predicate, kept as parsed source text, never evaluated.
    Filter {
        condition: String,
    },
    Join {
        left: NodeId,
        right: NodeId,
        keys: Vec<(String, String)>,
        kind: JoinKind,
    },
    Aggregate {
        group_by: Vec<String>,
        aggs: Vec<AggExpr>,
    },
    Calculate {
        output: String,
        expr: String,
    },
    /// A write to a table/view/file — the mirror of `Source`.
    Sink {
        object_name: String,
        columns: Vec<String>,
    },
    /// Deliberate, not a fallback-to-error: anything that could not be parsed
    /// or classified into the above, preserved verbatim as evidence that
    /// something is here and not yet understood.
    Unmapped {
        raw: String,
        reason: String,
    },
}

impl TransformNode {
    /// Short label used as `properties["node_type"]` once lowered into KIR,
    /// and to drive `KirEvidence::fragment` selection below.
    fn node_type(&self) -> &'static str {
        match self {
            Self::Source { .. } => "Source",
            Self::Filter { .. } => "Filter",
            Self::Join { .. } => "Join",
            Self::Aggregate { .. } => "Aggregate",
            Self::Calculate { .. } => "Calculate",
            Self::Sink { .. } => "Sink",
            Self::Unmapped { .. } => "Unmapped",
        }
    }

    /// The text this node's `KirEvidence::fragment` cites — the closest thing
    /// each variant has to "the source text this node was parsed from".
    fn evidence_fragment(&self) -> String {
        match self {
            Self::Source { object_name, .. } | Self::Sink { object_name, .. } => {
                object_name.clone()
            }
            Self::Filter { condition } => condition.clone(),
            Self::Calculate { expr, .. } => expr.clone(),
            Self::Join { keys, kind, .. } => format!("{kind:?} JOIN ON {keys:?}"),
            Self::Aggregate { group_by, aggs } => {
                format!("GROUP BY {group_by:?} {aggs:?}")
            }
            Self::Unmapped { raw, .. } => raw.clone(),
        }
    }

    /// Populates `properties` for this node's lowered `KirObject`, beyond the
    /// shared `node_type` key `lower_to_kir` always sets. `Filter.condition`/
    /// `Calculate.expr` land under `excerpt` specifically — that is the one
    /// property `KirObject::indexed_content()` reads, so a filter predicate
    /// or calculated-field formula becomes searchable via `ekos_search`/
    /// `ekos ask` for free, the same mechanism RFC 0026 relies on for
    /// `Concept` text.
    fn properties(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut props = serde_json::Map::new();
        match self {
            Self::Source {
                object_name,
                columns,
            }
            | Self::Sink {
                object_name,
                columns,
            } => {
                props.insert("object_name".into(), object_name.clone().into());
                props.insert("columns".into(), columns.clone().into());
            }
            Self::Filter { condition } => {
                props.insert("excerpt".into(), condition.clone().into());
            }
            Self::Calculate { output, expr } => {
                props.insert("output".into(), output.clone().into());
                props.insert("excerpt".into(), expr.clone().into());
            }
            Self::Join {
                left,
                right,
                keys,
                kind,
            } => {
                props.insert("left".into(), left.0.into());
                props.insert("right".into(), right.0.into());
                props.insert(
                    "keys".into(),
                    serde_json::to_value(keys).expect("keys must serialize"),
                );
                props.insert(
                    "join_kind".into(),
                    serde_json::to_value(kind).expect("kind must serialize"),
                );
            }
            Self::Aggregate { group_by, aggs } => {
                props.insert("group_by".into(), group_by.clone().into());
                props.insert(
                    "aggs".into(),
                    serde_json::to_value(aggs).expect("aggs must serialize"),
                );
            }
            Self::Unmapped { raw, reason } => {
                props.insert("raw".into(), raw.clone().into());
                props.insert("reason".into(), reason.clone().into());
            }
        }
        props
    }
}

/// Provenance shared by every node in one `TransformGraph` — which source
/// object this graph was parsed from, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformOrigin {
    /// File path or DB object identifier the graph was parsed from.
    pub source_path: String,
    /// e.g. `"pentaho-ktr"`, `"sql-select"`, `"sql-view"`, `"stored-procedure"`.
    pub source_kind: String,
    /// Intentionally part of this content's identity, unlike `ArtifactMeta::
    /// created_at` (excluded from the artifact layer's content hash): a
    /// re-parse of the same source tomorrow is a new fact, not the same fact
    /// re-observed, so this field is hashed like everything else, not stripped.
    pub extracted_at: DateTime<Utc>,
}

/// One Transformation IR graph, parsed from one source object (one Pentaho
/// step file, one SQL object, one stored-procedure body).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformGraph {
    pub nodes: Vec<TransformNode>,
    /// Data-flow edges, source-node-index -> consuming-node-index, in parse order.
    pub edges: Vec<(NodeId, NodeId)>,
    pub origin: TransformOrigin,
}

// ── Lowering into KIR ────────────────────────────────────────────────────────

/// Deterministic `KirId` for the node at `node_index` within a graph parsed
/// from `origin`. Scoped per `(source_kind, source_path, node_index)`, not per
/// node content — a node's position within its source graph is its identity,
/// its content is what versions (RFC 0027's "Append-only ledger fit" section).
/// Mirrors `local_docs_analyzer.rs`'s `section_kir_id` / RFC 0026's
/// `concept_kir_id` schemes exactly, for the same reason: stable identity
/// across re-parses is what lets the ledger's existing diff/version machinery
/// show "what changed in this Pentaho job since last week" for free.
pub fn transform_node_kir_id(origin: &TransformOrigin, node_index: usize) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!(
            "transform:{}:{}:node:{}",
            origin.source_kind, origin.source_path, node_index
        )
        .as_bytes(),
    ))
}

/// Deterministic `KirId` for the evidence record attached to the node at
/// `node_index`. Must be just as stable as `transform_node_kir_id` — a
/// `KirObject`'s `evidence: Vec<KirId>` is part of what `content_signature`
/// hashes, so a random evidence id would make every re-lowering of an
/// unchanged graph look like a content change to the ledger, even though
/// nothing logically changed.
fn transform_evidence_kir_id(origin: &TransformOrigin, node_index: usize) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!(
            "transform:{}:{}:evidence:{}",
            origin.source_kind, origin.source_path, node_index
        )
        .as_bytes(),
    ))
}

/// Lowers a `TransformGraph` into ledger-writable KIR, per RFC 0027's mapping
/// table: every node becomes one `KirObject(Custom("TransformNode"))` with one
/// `KirEvidence` citing `origin.source_path`, and every graph edge becomes a
/// `KirRelationship(Custom("FeedsInto"))`.
pub fn lower_to_kir(graph: &TransformGraph) -> KirGraph {
    let mut kir = KirGraph::new();
    let mut node_ids: Vec<KirId> = Vec::with_capacity(graph.nodes.len());

    for (index, node) in graph.nodes.iter().enumerate() {
        let id = transform_node_kir_id(&graph.origin, index);
        node_ids.push(id);

        let mut evidence = KirEvidence::new(
            SourceLocation::file(graph.origin.source_path.clone()),
            node.evidence_fragment(),
        );
        evidence.id = transform_evidence_kir_id(&graph.origin, index);
        let evidence_id = kir.add_evidence(evidence);

        let mut properties = node.properties();
        properties.insert("node_type".into(), node.node_type().into());

        let mut obj = KirObject::new(
            format!("{}:{}", graph.origin.source_path, index),
            ObjectKind::Custom("TransformNode".into()),
        );
        obj.id = id;
        obj.properties = properties.into_iter().collect();
        obj.evidence.push(evidence_id);
        kir.add_object(obj);
    }

    for (from, to) in &graph.edges {
        let from_id = node_ids[from.0 as usize];
        let to_id = node_ids[to.0 as usize];
        let rel = KirRelationship::deterministic(
            RelationshipKind::Custom("FeedsInto".into()),
            from_id,
            to_id,
            "",
        );
        kir.add_relationship(rel);
    }

    kir
}
