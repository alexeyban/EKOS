//! Content-addressable artifact system (Phase 2).
//!
//! Every compiler input and output is an artifact. The artifact ID is the SHA-256
//! of the canonical serialization of the artifact's *content fields* — volatile
//! metadata (timestamps, producer names) is excluded from the hash so identical
//! logical content always produces the same ID.

pub mod store;

use chrono::{DateTime, Utc};
use ekos_kir::{KirEvidence, KirGraph};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub use store::{ArtifactStore, FileSystemArtifactStore, StoreError};

// ────────────────────────────────────────────────────────────────────────────
// ArtifactId
// ────────────────────────────────────────────────────────────────────────────

/// SHA-256 content hash used as the artifact's stable identity.
///
/// Two artifacts with identical content fields always produce the same `ArtifactId`,
/// regardless of when they were created.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtifactId(pub String);

impl ArtifactId {
    /// Compute an ID from a JSON value representing the artifact's *content* fields.
    /// The value is canonicalized (keys sorted) before hashing.
    pub fn compute(content: &serde_json::Value) -> Self {
        let canonical = canonicalize(content.clone());
        let bytes = serde_json::to_vec(&canonical).expect("canonical JSON must serialize");
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        Self(hex::encode(hasher.finalize()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// First two hex characters — used as the store directory prefix.
    pub fn prefix(&self) -> &str {
        &self.0[..2]
    }
}

impl std::fmt::Display for ArtifactId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Recursively sort object keys so serialization is deterministic.
fn canonicalize(val: serde_json::Value) -> serde_json::Value {
    match val {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            let sorted = keys
                .into_iter()
                .map(|k| (k.clone(), canonicalize(map[&k].clone())))
                .collect();
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(canonicalize).collect())
        }
        other => other,
    }
}

fn compute_content_id<T: Serialize>(content: &T) -> ArtifactId {
    let value = serde_json::to_value(content).expect("content must serialize");
    ArtifactId::compute(&value)
}

// ────────────────────────────────────────────────────────────────────────────
// Common metadata (volatile — excluded from content hash)
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMeta {
    pub schema_version: u32,
    pub produced_by: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

impl ArtifactMeta {
    pub fn new(produced_by: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            produced_by: produced_by.into(),
            created_at: Utc::now(),
            tags: HashMap::new(),
        }
    }
}

impl Default for ArtifactMeta {
    fn default() -> Self {
        Self::new("ekos")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    Observation,
    Knowledge,
    Evidence,
    Diagnostic,
    Index,
}

// ────────────────────────────────────────────────────────────────────────────
// ObservationArtifact
// ────────────────────────────────────────────────────────────────────────────

/// Content fields that determine the ObservationArtifact's identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationContent {
    pub connector_name: String,
    pub target: String,
    pub data: serde_json::Value,
    #[serde(default)]
    pub input_ids: Vec<ArtifactId>,
}

/// Raw facts collected from one connector scan. ID is stable across runs for
/// identical connector + target + data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationArtifact {
    pub id: ArtifactId,
    pub artifact_type: ArtifactType,
    pub meta: ArtifactMeta,
    #[serde(flatten)]
    pub content: ObservationContent,
}

impl ObservationArtifact {
    pub fn new(
        connector_name: impl Into<String>,
        target: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        let content = ObservationContent {
            connector_name: connector_name.into(),
            target: target.into(),
            data,
            input_ids: Vec::new(),
        };
        let id = compute_content_id(&content);
        Self { id, artifact_type: ArtifactType::Observation, meta: ArtifactMeta::default(), content }
    }

    pub fn with_producer(mut self, name: impl Into<String>) -> Self {
        self.meta.produced_by = name.into();
        self
    }
}

// ────────────────────────────────────────────────────────────────────────────
// KnowledgeArtifact
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeContent {
    pub pass_name: String,
    pub input_ids: Vec<ArtifactId>,
    pub kir: KirGraph,
}

/// Compiled KIR output from one compiler pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeArtifact {
    pub id: ArtifactId,
    pub artifact_type: ArtifactType,
    pub meta: ArtifactMeta,
    #[serde(flatten)]
    pub content: KnowledgeContent,
}

impl KnowledgeArtifact {
    pub fn new(pass_name: impl Into<String>, input_ids: Vec<ArtifactId>, kir: KirGraph) -> Self {
        let content = KnowledgeContent { pass_name: pass_name.into(), input_ids, kir };
        let id = compute_content_id(&content);
        Self { id, artifact_type: ArtifactType::Knowledge, meta: ArtifactMeta::default(), content }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// EvidenceArtifact — storage wrapper for a KirEvidence node
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceContent {
    pub source_artifact_id: ArtifactId,
    pub evidence: KirEvidence,
}

/// Wrapper that stores a KirEvidence node as a content-addressable artifact.
/// `EvidenceArtifact` (storage), `KirEvidence` (KIR type), and `EvidenceRecord`
/// (Phase 8 CKM projection) are three views of the same concept — defined
/// in RFC 0003.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceArtifact {
    pub id: ArtifactId,
    pub artifact_type: ArtifactType,
    pub meta: ArtifactMeta,
    #[serde(flatten)]
    pub content: EvidenceContent,
}

impl EvidenceArtifact {
    pub fn new(source_artifact_id: ArtifactId, evidence: KirEvidence) -> Self {
        let content = EvidenceContent { source_artifact_id, evidence };
        let id = compute_content_id(&content);
        Self { id, artifact_type: ArtifactType::Evidence, meta: ArtifactMeta::default(), content }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// DiagnosticArtifact
// ────────────────────────────────────────────────────────────────────────────

/// Simplified diagnostic record (avoids pulling compiler-core into artifact).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticRecord {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticContent {
    pub build_id: String,
    pub records: Vec<DiagnosticRecord>,
}

/// Diagnostics emitted during a build run, stored for diffing across builds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticArtifact {
    pub id: ArtifactId,
    pub artifact_type: ArtifactType,
    pub meta: ArtifactMeta,
    #[serde(flatten)]
    pub content: DiagnosticContent,
}

impl DiagnosticArtifact {
    pub fn new(build_id: impl Into<String>, records: Vec<DiagnosticRecord>) -> Self {
        let content = DiagnosticContent { build_id: build_id.into(), records };
        let id = compute_content_id(&content);
        Self { id, artifact_type: ArtifactType::Diagnostic, meta: ArtifactMeta::default(), content }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// IndexArtifact
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexContent {
    pub build_id: String,
    pub entries: HashMap<String, ArtifactId>,
}

/// Named manifest mapping logical names to artifact IDs for a build run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexArtifact {
    pub id: ArtifactId,
    pub artifact_type: ArtifactType,
    pub meta: ArtifactMeta,
    #[serde(flatten)]
    pub content: IndexContent,
}

impl IndexArtifact {
    pub fn new(build_id: impl Into<String>, entries: HashMap<String, ArtifactId>) -> Self {
        let content = IndexContent { build_id: build_id.into(), entries };
        let id = compute_content_id(&content);
        Self { id, artifact_type: ArtifactType::Index, meta: ArtifactMeta::default(), content }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_content_same_id() {
        let a = ObservationArtifact::new("file", "src/main.rs", serde_json::json!({"size": 42}));
        let b = ObservationArtifact::new("file", "src/main.rs", serde_json::json!({"size": 42}));
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn different_content_different_id() {
        let a = ObservationArtifact::new("file", "src/main.rs", serde_json::json!({"size": 42}));
        let b = ObservationArtifact::new("file", "src/lib.rs", serde_json::json!({"size": 42}));
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn volatile_metadata_excluded_from_id() {
        // Two artifacts with identical content but different created_at must have the same ID.
        let a = ObservationArtifact::new("file", "src/main.rs", serde_json::json!({}));
        let mut b = ObservationArtifact::new("file", "src/main.rs", serde_json::json!({}));
        b.meta.created_at = DateTime::from_timestamp(0, 0).unwrap();
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn observation_artifact_round_trip() {
        let a = ObservationArtifact::new("git", "main", serde_json::json!({"sha": "abc"}));
        let json = serde_json::to_string(&a).unwrap();
        let back: ObservationArtifact = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, a.id);
        assert_eq!(back.content.connector_name, "git");
    }

    #[test]
    fn index_artifact_round_trip() {
        let mut entries = HashMap::new();
        entries.insert("obs/file".to_string(), ArtifactId("abc123".to_string()));
        let idx = IndexArtifact::new("build-001", entries);
        let json = serde_json::to_string(&idx).unwrap();
        let back: IndexArtifact = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, idx.id);
        assert!(back.content.entries.contains_key("obs/file"));
    }

    #[test]
    fn canonicalize_sorts_keys() {
        let unordered = serde_json::json!({"z": 1, "a": 2, "m": 3});
        let canonical = canonicalize(unordered);
        let s = serde_json::to_string(&canonical).unwrap();
        assert!(s.find("\"a\"") < s.find("\"m\""));
        assert!(s.find("\"m\"") < s.find("\"z\""));
    }
}
