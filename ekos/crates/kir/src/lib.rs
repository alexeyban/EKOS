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
        Self {
            path: path.into(),
            line: None,
            column: None,
        }
    }

    pub fn at(path: impl Into<String>, line: u32) -> Self {
        Self {
            path: path.into(),
            line: Some(line),
            column: None,
        }
    }
}

/// Classification of a KirObject.
///
/// Adding a variant here is safe and low-risk by construction: no code in this
/// workspace exhaustively `match`es over `ObjectKind` (every use is either direct
/// construction or an equality comparison), the enum is externally-tagged as a
/// plain JSON string (`#[serde(rename_all = "PascalCase")]`) with `Custom(String)`
/// as an untagged fallback, and EKL's `WHERE kind = '...'` predicate plus CLI
/// display output both work automatically off the `Display` impl below — no
/// query-language or formatting changes are needed when a variant is added here.
/// What *is* required is wiring up a construction site somewhere (a connector or
/// compiler pass that actually classifies something as this kind) — several
/// variants below (e.g. `Dataset`, `Pipeline`, `Model`) exist ahead of any
/// connector that emits them yet, same as `Directory`/`Service`/`Api` did before
/// this taxonomy was expanded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ObjectKind {
    File,
    Directory,
    Table,
    Entity,
    Service,
    Api,
    /// A constraint or policy (e.g. "orders must have a customer") — distinct
    /// from `BusinessConcept`, a named business idea or term.
    BusinessRule,
    /// A named business concept or term (e.g. "Customer Lifetime Value") —
    /// distinct from `BusinessRule` (a constraint) and `Entity` (a concrete
    /// business object like a specific customer record).
    BusinessConcept,
    /// A logical collection of data (warehouse schema, data product) — coarser
    /// grained than `Table`.
    Dataset,
    /// A single column/field within a `Table` or `Dataset`.
    Column,
    /// A data or CI/CD pipeline (dbt model, ETL job, build pipeline).
    Pipeline,
    /// A BI dashboard or report.
    Dashboard,
    /// A human contributor or stakeholder (e.g. a git commit author).
    Person,
    /// A trained ML/AI model artifact.
    Model,
    /// A versioned LLM prompt template.
    Prompt,
    /// An autonomous AI agent definition.
    Agent,
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

/// Same contract as `ObjectKind`'s `Display`: the single source of truth for
/// how a relationship kind renders anywhere a human or agent sees it
/// (CLI output, MCP tool results).
impl std::fmt::Display for RelationshipKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Custom(s) => write!(f, "{s}"),
            other => write!(f, "{other:?}"),
        }
    }
}

/// Parses the same names `Display` renders (case-insensitively for the
/// built-in variants); anything unrecognized becomes `Custom(s)` — this
/// never fails, matching the taxonomy's own escape hatch. Shared by EKL's
/// `VIA <kind>` clause and the `ekos_impact` MCP tool's `kinds` filter
/// (RFC 0018) so both parse relationship-kind names identically.
impl std::str::FromStr for RelationshipKind {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(if s.eq_ignore_ascii_case("ForeignKey") {
            Self::ForeignKey
        } else if s.eq_ignore_ascii_case("Calls") {
            Self::Calls
        } else if s.eq_ignore_ascii_case("Extends") {
            Self::Extends
        } else if s.eq_ignore_ascii_case("DependsOn") {
            Self::DependsOn
        } else if s.eq_ignore_ascii_case("OwnedBy") {
            Self::OwnedBy
        } else if s.eq_ignore_ascii_case("Contains") {
            Self::Contains
        } else if s.eq_ignore_ascii_case("References") {
            Self::References
        } else if s.eq_ignore_ascii_case("CoupledWith") {
            Self::CoupledWith
        } else if s.eq_ignore_ascii_case("Unknown") {
            Self::Unknown
        } else {
            Self::Custom(s.to_string())
        })
    }
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

    /// Text fed to full-text search (RFC 0014's `excerpt`, extended by RFC
    /// 0019's `symbols` and RFC 0024's `ocr_text`): the file-opening
    /// excerpt, any harvested declaration-line symbol names, and any
    /// OCR'd image text, space-joined. Shared by both ledger backends so
    /// `ekos_search` finds a symbol or OCR'd phrase whether or not it
    /// happens to fall within the excerpt's leading window. Before RFC
    /// 0024, `ocr_text` rode on an object's properties but was never
    /// actually searchable — an object's scanned-page text was only
    /// reachable if its id was already known through some other path.
    pub fn indexed_content(&self) -> String {
        let excerpt = self
            .properties
            .get("excerpt")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let symbols = self
            .properties
            .get("symbols")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let ocr_text = self
            .properties
            .get("ocr_text")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        [excerpt, symbols.as_str(), ocr_text]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
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
    /// Domain-time validity window (RFC 0047) — when this relationship was/is
    /// *true*, independent of when the ledger *observed* it. Distinct from
    /// `object_at`/`relationships_at`'s point-in-time reconstruction, which
    /// answers "what did the ledger know as of T" (an observation-time cut
    /// over append history), not "was this fact true during T". `None` on
    /// either bound means unbounded in that direction — today's implicit
    /// always-valid behavior, made explicit. `#[serde(default)]` so every
    /// relationship already persisted in a live ledger deserializes unchanged.
    #[serde(default)]
    pub valid_from: Option<DateTime<Utc>>,
    #[serde(default)]
    pub valid_until: Option<DateTime<Utc>>,
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
            valid_from: None,
            valid_until: None,
        }
    }

    /// Set the domain-time validity window (RFC 0047). See the field docs on
    /// `valid_from`/`valid_until` for how this differs from the ledger's
    /// observation-time history.
    pub fn with_validity(
        mut self,
        from: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Self {
        self.valid_from = from;
        self.valid_until = until;
        self
    }

    /// True for a claim that isn't a confirmed fact — a hypothesis (or a
    /// reviewed-and-rejected candidate), never silently treated as an
    /// observed one. Originally scoped to cross-system identity candidates
    /// only (RFC 0029, `Custom("SameAs")`, whose `properties["status"]` is
    /// always exactly `"unconfirmed"`, `"confirmed"`, or `"rejected"` —
    /// `ekos_identity_review`'s only two valid `decision` values plus the
    /// initial state); generalized (RFC 0047) to any relationship kind that
    /// adopts the same `status` convention, so an arbitrary claim
    /// (`Custom("Opposes")`, `Custom("Believes")`, ...) gets the same
    /// treatment without its own special case. A relationship carrying no
    /// `status` property at all — every compiler-derived fact today
    /// (`ForeignKey`, `Custom("FeedsInto")`, etc., none of which have ever
    /// touched this property) — always passes, unaffected by this
    /// generalization. Graph traversal (`ekos_dependents`, `ekos_impact`,
    /// `ekos_neighborhood`, EKL's `FROM` anchor) must exclude anything this
    /// returns `true` for: RFC 0029 requires an unconfirmed match to stay
    /// "structurally distinguishable from an observed fact, never
    /// indistinguishable" — silently walking it as if it were a real edge
    /// breaks that guarantee, and the same reasoning applies to a rejected
    /// candidate (still not a fact) and to any other kind of unconfirmed
    /// claim.
    pub fn is_pending_review(&self) -> bool {
        match self.properties.get("status").and_then(|v| v.as_str()) {
            None => false,
            Some(status) => status != "confirmed",
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
    /// Escape hatch (RFC 0048), same pattern as `ObjectKind`/`RelationshipKind`
    /// (`:113-114`, `:139-140`) — for event shapes the fixed variants above
    /// don't cover, e.g. a simulated action (`Custom("Accuse")`,
    /// `Custom("PostMessage")`) that isn't a compiler-observed lifecycle
    /// transition. No code in this workspace exhaustively matches over
    /// `EventKind` today (confirmed by direct search), so adding this variant
    /// can't silently break an existing `match`.
    #[serde(untagged)]
    Custom(String),
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
    fn indexed_content_includes_ocr_text() {
        let obj = KirObject::new("scan.pdf", ObjectKind::Custom("Document".into()))
            .with_property("ocr_text", serde_json::json!("hello from a scanned page"));
        assert_eq!(obj.indexed_content(), "hello from a scanned page");
    }

    #[test]
    fn indexed_content_concatenates_excerpt_symbols_and_ocr_text() {
        let obj = KirObject::new("mixed", ObjectKind::File)
            .with_property("excerpt", serde_json::json!("an excerpt"))
            .with_property("symbols", serde_json::json!(["foo", "bar"]))
            .with_property("ocr_text", serde_json::json!("scanned words"));
        let content = obj.indexed_content();
        assert!(content.contains("an excerpt"));
        assert!(content.contains("foo bar"));
        assert!(content.contains("scanned words"));
    }

    #[test]
    fn indexed_content_empty_when_no_relevant_properties() {
        let obj = KirObject::new("bare", ObjectKind::File);
        assert_eq!(obj.indexed_content(), "");
    }

    #[test]
    fn object_kind_taxonomy_round_trips() {
        // Every named ObjectKind variant must serialize to a plain PascalCase JSON
        // string and deserialize back to the identical variant — including the
        // ones added for AD-001 with no construction site yet.
        let kinds = [
            ObjectKind::File,
            ObjectKind::Directory,
            ObjectKind::Table,
            ObjectKind::Entity,
            ObjectKind::Service,
            ObjectKind::Api,
            ObjectKind::BusinessRule,
            ObjectKind::BusinessConcept,
            ObjectKind::Dataset,
            ObjectKind::Column,
            ObjectKind::Pipeline,
            ObjectKind::Dashboard,
            ObjectKind::Person,
            ObjectKind::Model,
            ObjectKind::Prompt,
            ObjectKind::Agent,
            ObjectKind::Unknown,
        ];
        for kind in &kinds {
            let json = serde_json::to_string(kind).unwrap();
            let back: ObjectKind = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, kind, "round-trip mismatch for {kind}");
        }
    }

    #[test]
    fn object_kind_custom_fallback_still_works() {
        // An unrecognized string must still fall back to Custom, not fail to parse.
        let back: ObjectKind = serde_json::from_str("\"SomethingNotYetEnumerated\"").unwrap();
        assert_eq!(
            back,
            ObjectKind::Custom("SomethingNotYetEnumerated".to_string())
        );
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
            KirEvidence::new(
                SourceLocation::at("schema.sql", 3),
                "FOREIGN KEY (customer_id) REFERENCES customers(id)",
            )
            .with_confidence(0.99),
        );

        let customer_id = g.add_object(
            KirObject::new("customers", ObjectKind::Table)
                .with_property("schema", serde_json::json!("public"))
                .with_evidence(ev_id),
        );

        let order_id =
            g.add_object(KirObject::new("orders", ObjectKind::Table).with_evidence(ev_id));

        g.add_relationship(KirRelationship::new(
            RelationshipKind::ForeignKey,
            order_id,
            customer_id,
        ));

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

    // ── RFC 0047: valid_from/valid_until + generalized is_pending_review ──────

    #[test]
    fn valid_from_until_default_to_none_and_round_trip() {
        let rel = KirRelationship::new(RelationshipKind::DependsOn, KirId::new(), KirId::new());
        assert_eq!(rel.valid_from, None);
        assert_eq!(rel.valid_until, None);

        let from = Utc::now();
        let until = from + chrono::Duration::days(30);
        let rel = rel.with_validity(Some(from), Some(until));
        let json = serde_json::to_string(&rel).unwrap();
        let back: KirRelationship = serde_json::from_str(&json).unwrap();
        assert_eq!(back.valid_from, Some(from));
        assert_eq!(back.valid_until, Some(until));
    }

    #[test]
    fn relationship_json_predating_rfc_0047_still_deserializes() {
        // A relationship persisted before valid_from/valid_until existed —
        // #[serde(default)] must let it load as None/None, not fail to parse.
        let from = KirId::new();
        let to = KirId::new();
        let old_json = serde_json::json!({
            "id": KirId::new(),
            "kind": "DependsOn",
            "from": from,
            "to": to,
            "properties": {},
            "evidence": [],
            "created_at": Utc::now(),
        });
        let back: KirRelationship = serde_json::from_value(old_json).unwrap();
        assert_eq!(back.valid_from, None);
        assert_eq!(back.valid_until, None);
    }

    #[test]
    fn is_pending_review_regression_same_as_unconfirmed_and_confirmed() {
        // RFC 0029's own behavior, pinned exactly: unconfirmed excluded,
        // confirmed passes.
        let mut rel = KirRelationship::new(
            RelationshipKind::Custom("SameAs".to_string()),
            KirId::new(),
            KirId::new(),
        );
        rel.properties
            .insert("status".into(), serde_json::json!("unconfirmed"));
        assert!(rel.is_pending_review());

        rel.properties
            .insert("status".into(), serde_json::json!("confirmed"));
        assert!(!rel.is_pending_review());
    }

    #[test]
    fn is_pending_review_excludes_rejected_not_just_unconfirmed() {
        // A reviewed-and-rejected candidate must stay excluded from traversal
        // too — narrowing the check to only "unconfirmed" would silently let
        // a rejected identity match leak back in as if it were a real edge.
        let mut rel = KirRelationship::new(
            RelationshipKind::Custom("SameAs".to_string()),
            KirId::new(),
            KirId::new(),
        );
        rel.properties
            .insert("status".into(), serde_json::json!("rejected"));
        assert!(rel.is_pending_review());
    }

    #[test]
    fn is_pending_review_generalizes_beyond_same_as() {
        // RFC 0047: any kind adopting the same status convention gets the
        // same hypothesis/fact distinction, not just RFC 0029's SameAs.
        let mut rel = KirRelationship::new(
            RelationshipKind::Custom("Opposes".to_string()),
            KirId::new(),
            KirId::new(),
        );
        rel.properties
            .insert("status".into(), serde_json::json!("unconfirmed"));
        assert!(rel.is_pending_review());

        rel.properties
            .insert("status".into(), serde_json::json!("confirmed"));
        assert!(!rel.is_pending_review());
    }

    #[test]
    fn is_pending_review_false_when_no_status_property() {
        // Every compiler-derived fact today (ForeignKey, Contains, DependsOn,
        // ...) never touches "status" at all — must be entirely unaffected.
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, KirId::new(), KirId::new());
        assert!(!rel.is_pending_review());
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
    fn event_kind_custom_round_trips_alongside_named_variants() {
        // RFC 0048: a simulation-shaped action doesn't fit any fixed variant.
        let ev = KirEvent {
            id: KirId::new(),
            kind: EventKind::Custom("Accuse".to_string()),
            subject: KirId::new(),
            payload: serde_json::json!({"target": "bob"}),
            evidence: Vec::new(),
            occurred_at: Utc::now(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: KirEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, EventKind::Custom("Accuse".to_string()));

        // Named variants are unaffected by the new variant's presence.
        for kind in [
            EventKind::Created,
            EventKind::Modified,
            EventKind::Deleted,
            EventKind::Migrated,
            EventKind::Deployed,
            EventKind::Merged,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: EventKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
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
