use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Unique identifier for any KIR node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KirId(pub Uuid);

impl KirId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl Default for KirId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for KirId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for KirId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Location in a source artifact (file, line, column).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    pub path: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

impl SourceLocation {
    pub fn file(path: impl Into<String>) -> Self {
        Self { path: path.into(), line: None, column: None }
    }

    pub fn at(path: impl Into<String>, line: u32) -> Self {
        Self { path: path.into(), line: Some(line), column: None }
    }
}

/// Classification of a KirObject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ObjectKind {
    File,
    Directory,
    Table,
    Entity,
    Service,
    Api,
    BusinessRule,
    Unknown,
    #[serde(untagged)]
    Custom(String),
}

impl std::fmt::Display for ObjectKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Custom(s) => write!(f, "{s}"),
            other => write!(f, "{other:?}"),
        }
    }
}

/// Semantic type of a relationship between two objects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum RelationshipKind {
    ForeignKey,
    Calls,
    Extends,
    DependsOn,
    OwnedBy,
    Contains,
    References,
    CoupledWith,
    Unknown,
    #[serde(untagged)]
    Custom(String),
}

/// The identity of a concept in the enterprise (table, entity, service, rule…).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KirObject {
    pub id: KirId,
    pub name: String,
    pub kind: ObjectKind,
    #[serde(default)]
    pub properties: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub evidence: Vec<KirId>,
    pub created_at: DateTime<Utc>,
}

impl KirObject {
    pub fn new(name: impl Into<String>, kind: ObjectKind) -> Self {
        Self {
            id: KirId::new(),
            name: name.into(),
            kind,
            properties: HashMap::new(),
            evidence: Vec::new(),
            created_at: Utc::now(),
        }
    }

    pub fn with_property(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.properties.insert(key.into(), value);
        self
    }

    pub fn with_evidence(mut self, ev: KirId) -> Self {
        self.evidence.push(ev);
        self
    }
}

/// Provenance — links a knowledge claim back to the source fragment that justifies it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KirEvidence {
    pub id: KirId,
    pub location: SourceLocation,
    pub fragment: String,
    pub confidence: f32,
    pub created_at: DateTime<Utc>,
}

impl KirEvidence {
    pub fn new(location: SourceLocation, fragment: impl Into<String>) -> Self {
        Self {
            id: KirId::new(),
            location,
            fragment: fragment.into(),
            confidence: 1.0,
            created_at: Utc::now(),
        }
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }
}

/// Directed edge between two KirObjects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KirRelationship {
    pub id: KirId,
    pub kind: RelationshipKind,
    pub from: KirId,
    pub to: KirId,
    #[serde(default)]
    pub properties: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub evidence: Vec<KirId>,
    pub created_at: DateTime<Utc>,
}

impl KirRelationship {
    pub fn new(kind: RelationshipKind, from: KirId, to: KirId) -> Self {
        Self {
            id: KirId::new(),
            kind,
            from,
            to,
            properties: HashMap::new(),
            evidence: Vec::new(),
            created_at: Utc::now(),
        }
    }
}

/// Immutable change record — the only mechanism that mutates enterprise state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KirEvent {
    pub id: KirId,
    pub kind: EventKind,
    pub subject: KirId,
    pub payload: serde_json::Value,
    #[serde(default)]
    pub evidence: Vec<KirId>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum EventKind {
    Created,
    Modified,
    Deleted,
    Migrated,
    Deployed,
    Merged,
}

/// Container for all KIR nodes produced by one compilation run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KirGraph {
    pub objects: Vec<KirObject>,
    pub relationships: Vec<KirRelationship>,
    pub events: Vec<KirEvent>,
    pub evidence: Vec<KirEvidence>,
}

impl KirGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_object(&mut self, obj: KirObject) -> KirId {
        let id = obj.id;
        self.objects.push(obj);
        id
    }

    pub fn add_evidence(&mut self, ev: KirEvidence) -> KirId {
        let id = ev.id;
        self.evidence.push(ev);
        id
    }

    pub fn add_relationship(&mut self, rel: KirRelationship) -> KirId {
        let id = rel.id;
        self.relationships.push(rel);
        id
    }

    pub fn get_object(&self, id: &KirId) -> Option<&KirObject> {
        self.objects.iter().find(|o| &o.id == id)
    }

    pub fn get_evidence(&self, id: &KirId) -> Option<&KirEvidence> {
        self.evidence.iter().find(|e| &e.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kir_object_round_trip() {
        let obj = KirObject::new("orders", ObjectKind::Table)
            .with_property("schema", serde_json::json!("public"));
        let json = serde_json::to_string(&obj).unwrap();
        let back: KirObject = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, obj.name);
        assert_eq!(back.id, obj.id);
    }

    #[test]
    fn kir_evidence_round_trip() {
        let ev = KirEvidence::new(SourceLocation::at("schema.sql", 42), "id INT PRIMARY KEY")
            .with_confidence(0.95);
        let json = serde_json::to_string(&ev).unwrap();
        let back: KirEvidence = serde_json::from_str(&json).unwrap();
        assert!((back.confidence - 0.95).abs() < 0.001);
        assert_eq!(back.location.line, Some(42));
    }

    #[test]
    fn kir_graph_add_and_get() {
        let mut g = KirGraph::new();
        let ev = KirEvidence::new(SourceLocation::file("test.rs"), "fn main()");
        let ev_id = g.add_evidence(ev);
        let obj = KirObject::new("main", ObjectKind::Unknown).with_evidence(ev_id);
        let obj_id = g.add_object(obj);
        assert!(g.get_object(&obj_id).is_some());
        assert!(g.get_evidence(&ev_id).is_some());
    }

    // ── Phase 5: KIR full graph round-trip ──────────────────────────────────

    fn sample_graph() -> KirGraph {
        let mut g = KirGraph::new();

        let ev_id = g.add_evidence(
            KirEvidence::new(SourceLocation::at("schema.sql", 3), "FOREIGN KEY (customer_id) REFERENCES customers(id)")
                .with_confidence(0.99),
        );

        let customer_id = g.add_object(
            KirObject::new("customers", ObjectKind::Table)
                .with_property("schema", serde_json::json!("public"))
                .with_evidence(ev_id),
        );

        let order_id = g.add_object(
            KirObject::new("orders", ObjectKind::Table)
                .with_evidence(ev_id),
        );

        g.add_relationship(
            KirRelationship::new(RelationshipKind::ForeignKey, order_id, customer_id),
        );

        g.events.push(KirEvent {
            id: KirId::new(),
            kind: EventKind::Created,
            subject: customer_id,
            payload: serde_json::json!({"migration": "001"}),
            evidence: vec![ev_id],
            occurred_at: Utc::now(),
        });

        g
    }

    #[test]
    fn kir_graph_full_round_trip() {
        let g = sample_graph();
        let json = serde_json::to_string(&g).unwrap();
        let back: KirGraph = serde_json::from_str(&json).unwrap();
        assert_eq!(back.objects.len(), g.objects.len());
        assert_eq!(back.relationships.len(), g.relationships.len());
        assert_eq!(back.events.len(), g.events.len());
        assert_eq!(back.evidence.len(), g.evidence.len());
        assert_eq!(back.objects[0].name, "customers");
        assert_eq!(back.relationships[0].kind, RelationshipKind::ForeignKey);
    }

    #[test]
    fn kir_relationship_serializes_from_to() {
        let from = KirId::new();
        let to = KirId::new();
        let rel = KirRelationship::new(RelationshipKind::Calls, from, to);
        let json = serde_json::to_string(&rel).unwrap();
        let back: KirRelationship = serde_json::from_str(&json).unwrap();
        assert_eq!(back.from, from);
        assert_eq!(back.to, to);
        assert_eq!(back.kind, RelationshipKind::Calls);
    }

    #[test]
    fn kir_event_round_trip() {
        let subject = KirId::new();
        let ev = KirEvent {
            id: KirId::new(),
            kind: EventKind::Deployed,
            subject,
            payload: serde_json::json!({"env": "prod"}),
            evidence: Vec::new(),
            occurred_at: Utc::now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: KirEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.subject, subject);
        assert_eq!(back.kind, EventKind::Deployed);
        assert_eq!(back.payload["env"], "prod");
    }

    #[test]
    fn knowledge_artifact_embeds_kir_graph() {
        // Verifies the KnowledgeArtifact ↔ KirGraph boundary is stable
        // without importing ekos-artifact here (kir must stay dep-free).
        // We just confirm KirGraph serializes to a Value that can be embedded.
        let g = sample_graph();
        let value = serde_json::to_value(&g).unwrap();
        assert!(value.is_object());
        assert!(value["objects"].is_array());
        assert!(value["relationships"].is_array());
        assert!(value["evidence"].is_array());
        assert!(value["events"].is_array());
    }
}
