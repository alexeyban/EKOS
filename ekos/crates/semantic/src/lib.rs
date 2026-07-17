//! Semantic compiler: resolved KIR → Canonical Knowledge Model (CKM).
//!
//! See Phase 8 in TODO.md. The CKM is the final, denormalised, validated output
//! of the compiler pipeline. Downstream consumers (Ledger, Runtime, AI) always
//! read from the CKM, never from raw KIR.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_identity::{DefaultResolver, IdentityResolver, MergeProposal};
use ekos_kir::{KirGraph, KirId, KirRelationship, ObjectKind, RelationshipKind};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

// ── CKM Types ──────────────────────────────────────────────────────────────────

/// Flattened provenance record embedded inside a `CkmObject` or `CkmRelationship`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub id: KirId,
    pub source: String,
    pub fragment: String,
    pub confidence: f32,
}

/// Canonical, denormalised view of one resolved enterprise concept.
///
/// Unlike `KirObject`, all related evidence is embedded (no forward references).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CkmObject {
    pub id: KirId,
    pub name: String,
    pub kind: ObjectKind,
    #[serde(default)]
    pub properties: HashMap<String, serde_json::Value>,
    /// Best single evidence fragment; `None` if there is no evidence.
    pub primary_description: Option<String>,
    /// Evidence sorted by confidence descending.
    #[serde(default)]
    pub evidence: Vec<EvidenceRecord>,
}

/// Canonical, deduplicated relationship between two `CkmObject`s.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CkmRelationship {
    pub id: KirId,
    pub kind: RelationshipKind,
    pub from: KirId,
    pub to: KirId,
    #[serde(default)]
    pub properties: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub evidence: Vec<EvidenceRecord>,
}

/// The Canonical Knowledge Model — the final output of one compilation run.
///
/// Schema version 1. Written to `.ekos/ckm/model.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CkModel {
    pub version: u32,
    pub compiled_at: DateTime<Utc>,
    pub objects: Vec<CkmObject>,
    pub relationships: Vec<CkmRelationship>,
    /// All evidence records keyed by `KirId.to_string()`, for O(1) lookup.
    pub evidence_index: HashMap<String, EvidenceRecord>,
}

impl CkModel {
    /// Validate structural invariants. Returns a list of error descriptions.
    ///
    /// Checks:
    /// - No duplicate object IDs.
    /// - Every relationship `from` and `to` references an existing object.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        let mut seen_ids: HashSet<KirId> = HashSet::new();
        for obj in &self.objects {
            if !seen_ids.insert(obj.id) {
                errors.push(format!("duplicate object id: {}", obj.id));
            }
        }

        let object_ids: HashSet<KirId> = self.objects.iter().map(|o| o.id).collect();
        for rel in &self.relationships {
            if !object_ids.contains(&rel.from) {
                errors.push(format!(
                    "relationship {} has unknown from-id {}",
                    rel.id, rel.from
                ));
            }
            if !object_ids.contains(&rel.to) {
                errors.push(format!(
                    "relationship {} has unknown to-id {}",
                    rel.id, rel.to
                ));
            }
        }

        errors
    }
}

// ── Graph utilities ────────────────────────────────────────────────────────────

/// Append all nodes from `src` into `dst`.
pub fn merge_graphs(dst: &mut KirGraph, src: KirGraph) {
    dst.objects.extend(src.objects);
    dst.relationships.extend(src.relationships);
    dst.events.extend(src.events);
    dst.evidence.extend(src.evidence);
}

/// Remap non-canonical object IDs according to identity resolution proposals.
///
/// - Updates `from`/`to` on every relationship.
/// - Updates `subject` on every event.
/// - Removes non-canonical objects from `graph.objects`.
pub fn apply_merges(mut graph: KirGraph, proposals: &[MergeProposal]) -> KirGraph {
    let mut id_map: HashMap<KirId, KirId> = HashMap::new();

    for p in proposals {
        for &sid in &p.source_ids {
            if sid != p.canonical_id {
                id_map.insert(sid, p.canonical_id);
            }
        }
    }

    for rel in &mut graph.relationships {
        if let Some(&cid) = id_map.get(&rel.from) {
            rel.from = cid;
        }
        if let Some(&cid) = id_map.get(&rel.to) {
            rel.to = cid;
        }
    }

    for ev in &mut graph.events {
        if let Some(&cid) = id_map.get(&ev.subject) {
            ev.subject = cid;
        }
    }

    let non_canonical: HashSet<KirId> = id_map.keys().copied().collect();
    graph.objects.retain(|o| !non_canonical.contains(&o.id));

    graph.relationships = dedup_relationships(graph.relationships);

    graph
}

/// Deduplicate relationships by `(from, to, kind)`, merging evidence lists.
pub fn dedup_relationships(rels: Vec<KirRelationship>) -> Vec<KirRelationship> {
    let mut index: HashMap<(KirId, KirId, String), usize> = HashMap::new();
    let mut result: Vec<KirRelationship> = Vec::new();

    for rel in rels {
        let key = (rel.from, rel.to, format!("{:?}", rel.kind));
        if let Some(&idx) = index.get(&key) {
            for ev_id in &rel.evidence {
                if !result[idx].evidence.contains(ev_id) {
                    result[idx].evidence.push(*ev_id);
                }
            }
        } else {
            index.insert(key, result.len());
            result.push(rel);
        }
    }

    result
}

/// Build a `CkModel` from a resolved `KirGraph`.
pub fn build_ckm(graph: &KirGraph) -> CkModel {
    // Build evidence_index from graph.evidence
    let mut evidence_index: HashMap<String, EvidenceRecord> = HashMap::new();
    for ev in &graph.evidence {
        evidence_index.insert(
            ev.id.to_string(),
            EvidenceRecord {
                id: ev.id,
                source: ev.location.path.clone(),
                fragment: ev.fragment.clone(),
                confidence: ev.confidence,
            },
        );
    }

    let objects: Vec<CkmObject> = graph
        .objects
        .iter()
        .map(|obj| {
            let mut ev_records: Vec<EvidenceRecord> = obj
                .evidence
                .iter()
                .filter_map(|ev_id| evidence_index.get(&ev_id.to_string()).cloned())
                .collect();
            ev_records.sort_by(|a, b| {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let primary_description = ev_records.first().map(|e| e.fragment.clone());
            CkmObject {
                id: obj.id,
                name: obj.name.clone(),
                kind: obj.kind.clone(),
                properties: obj.properties.clone(),
                primary_description,
                evidence: ev_records,
            }
        })
        .collect();

    let relationships: Vec<CkmRelationship> = graph
        .relationships
        .iter()
        .map(|rel| {
            let ev_records: Vec<EvidenceRecord> = rel
                .evidence
                .iter()
                .filter_map(|ev_id| evidence_index.get(&ev_id.to_string()).cloned())
                .collect();
            CkmRelationship {
                id: rel.id,
                kind: rel.kind.clone(),
                from: rel.from,
                to: rel.to,
                properties: rel.properties.clone(),
                evidence: ev_records,
            }
        })
        .collect();

    CkModel {
        version: 1,
        compiled_at: Utc::now(),
        objects,
        relationships,
        evidence_index,
    }
}

// ── SemanticCompilerPass ───────────────────────────────────────────────────────

/// Compiler pass: loads all KnowledgeArtifacts, resolves identities, builds and
/// validates the CKM, and writes it to `<output_dir>/model.json`.
pub struct SemanticCompilerPass {
    output_dir: PathBuf,
    /// Sorted ids of the knowledge artifacts this pass consumes — the Phase 13
    /// cache key. Without them the pass cached on `{version, config}` alone
    /// and silently reused a stale CKM after any recover re-run (devlog 14).
    cache_inputs: Vec<String>,
}

impl SemanticCompilerPass {
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
            cache_inputs: Vec::new(),
        }
    }

    /// Declare the knowledge-artifact ids this pass will consume, so the cache
    /// invalidates when recover output changes.
    pub fn with_cache_inputs(mut self, mut ids: Vec<String>) -> Self {
        ids.sort();
        self.cache_inputs = ids;
        self
    }
}

#[async_trait]
impl CompilerPass for SemanticCompilerPass {
    fn name(&self) -> &str {
        "semantic-compiler"
    }

    fn cache_inputs(&self) -> Vec<String> {
        self.cache_inputs.clone()
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        // ── Load all KnowledgeArtifacts ───────────────────────────────────────
        let ids = ctx
            .artifact_store
            .list()
            .map_err(|e| PassError::failed(format!("artifact store list failed: {e}")))?;

        let mut combined = KirGraph::new();
        let mut ka_count = 0usize;

        for id in &ids {
            let json = match ctx.artifact_store.read(id) {
                Ok(Some(j)) => j,
                _ => continue,
            };
            if json["artifact_type"].as_str() != Some("knowledge") {
                continue;
            }
            match serde_json::from_value::<KirGraph>(json["kir"].clone()) {
                Ok(graph) => {
                    merge_graphs(&mut combined, graph);
                    ka_count += 1;
                }
                Err(e) => ctx
                    .diagnostics
                    .lock()
                    .unwrap()
                    .warning("SEM000", format!("skipping artifact {id}: {e}")),
            }
        }

        if ka_count == 0 {
            ctx.diagnostics.lock().unwrap().warning(
                "SEM000",
                "no knowledge artifacts found — run `ekos recover` first",
            );
        }

        // ── Identity resolution ───────────────────────────────────────────────
        let resolution = DefaultResolver::new().resolve(&combined);

        for conflict in &resolution.conflicts {
            ctx.diagnostics
                .lock()
                .unwrap()
                .warning("SEM001", conflict.description.clone());
        }

        tracing::info!(
            proposals = resolution.stats.merges_proposed,
            conflicts = resolution.stats.conflicts_detected,
            "identity resolution complete"
        );

        // ── Apply merges ──────────────────────────────────────────────────────
        let resolved = apply_merges(combined, &resolution.proposals);

        // ── Build CKM ────────────────────────────────────────────────────────
        let model = build_ckm(&resolved);

        // ── Validate ─────────────────────────────────────────────────────────
        let validation_errors = model.validate();
        for e in &validation_errors {
            ctx.diagnostics.lock().unwrap().warning("SEM002", e.clone());
        }

        // ── Write to disk ─────────────────────────────────────────────────────
        std::fs::create_dir_all(&self.output_dir)
            .map_err(|e| PassError::failed(format!("cannot create ckm dir: {e}")))?;

        // RFC 0015: compact JSON in a zstd frame (`model.json.zst`); a stale
        // pre-0015 plain `model.json` must not shadow the fresh model for
        // readers that fall back to it, so it is removed.
        let plain_path = self.output_dir.join("model.json");
        let model_path = ekos_common::compress::zst_sibling(&plain_path);
        ekos_common::compress::write_json_zst(&model_path, &model)
            .map_err(|e| PassError::failed(format!("cannot write CKM: {e}")))?;
        std::fs::remove_file(&plain_path).ok();

        tracing::info!(
            objects = model.objects.len(),
            relationships = model.relationships.len(),
            path = %model_path.display(),
            "CKM written"
        );

        Ok(())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{
        EventKind, KirEvent, KirEvidence, KirObject, KirRelationship, ObjectKind, RelationshipKind,
        SourceLocation,
    };
    use tempfile::TempDir;

    /// Regression (devlog 14): without declared cache inputs, the pass cached
    /// on `{version, config}` alone and silently reused a stale CKM after a
    /// recover re-run. The declared inputs must round-trip, sorted.
    #[test]
    fn cache_inputs_are_declared_and_sorted() {
        let pass = SemanticCompilerPass::new("/tmp/out")
            .with_cache_inputs(vec!["bbb".into(), "aaa".into()]);
        assert_eq!(
            ekos_compiler_core::pass::CompilerPass::cache_inputs(&pass),
            vec!["aaa", "bbb"]
        );
        assert!(
            ekos_compiler_core::pass::CompilerPass::cache_inputs(&SemanticCompilerPass::new(
                "/tmp/out"
            ))
            .is_empty()
        );
    }

    fn two_object_graph() -> KirGraph {
        let mut g = KirGraph::new();

        let ev = KirEvidence::new(SourceLocation::at("schema.sql", 1), "CREATE TABLE orders");
        let ev_id = g.add_evidence(ev);

        let cust =
            g.add_object(KirObject::new("customers", ObjectKind::Table).with_evidence(ev_id));
        let ord = g.add_object(KirObject::new("orders", ObjectKind::Table).with_evidence(ev_id));

        g.add_relationship(KirRelationship::new(
            RelationshipKind::ForeignKey,
            ord,
            cust,
        ));

        g
    }

    #[test]
    fn build_ckm_produces_correct_counts() {
        let graph = two_object_graph();
        let model = build_ckm(&graph);
        assert_eq!(model.version, 1);
        assert_eq!(model.objects.len(), 2);
        assert_eq!(model.relationships.len(), 1);
        assert_eq!(model.evidence_index.len(), 1);
    }

    #[test]
    fn build_ckm_embeds_evidence_in_objects() {
        let graph = two_object_graph();
        let model = build_ckm(&graph);
        let cust = model
            .objects
            .iter()
            .find(|o| o.name == "customers")
            .unwrap();
        assert_eq!(cust.evidence.len(), 1);
        assert_eq!(cust.evidence[0].fragment, "CREATE TABLE orders");
        assert!(cust.primary_description.is_some());
    }

    #[test]
    fn validate_passes_on_valid_ckm() {
        let model = build_ckm(&two_object_graph());
        assert!(model.validate().is_empty());
    }

    #[test]
    fn validate_catches_dangling_relationship() {
        let mut model = build_ckm(&two_object_graph());
        // Inject a relationship pointing to a non-existent object.
        let phantom = KirId::new();
        model.relationships.push(CkmRelationship {
            id: KirId::new(),
            kind: RelationshipKind::References,
            from: model.objects[0].id,
            to: phantom,
            properties: HashMap::new(),
            evidence: vec![],
        });
        let errors = model.validate();
        assert!(errors.iter().any(|e| e.contains("unknown to-id")));
    }

    #[test]
    fn dedup_relationships_merges_duplicate() {
        let a = KirId::new();
        let b = KirId::new();
        let ev1 = KirId::new();
        let ev2 = KirId::new();

        let rel1 = KirRelationship::new(RelationshipKind::ForeignKey, a, b);
        let mut rel2 = KirRelationship::new(RelationshipKind::ForeignKey, a, b);
        // Manually give them different evidence so we can count the merge.
        let mut r1 = rel1;
        r1.evidence.push(ev1);
        rel2.evidence.push(ev2);

        let deduped = dedup_relationships(vec![r1, rel2]);
        assert_eq!(
            deduped.len(),
            1,
            "two identical FK rels must deduplicate to one"
        );
        assert_eq!(deduped[0].evidence.len(), 2, "evidence must be merged");
    }

    #[test]
    fn apply_merges_remaps_relationship_ids() {
        let mut g = KirGraph::new();
        let old = g.add_object(KirObject::new("customer", ObjectKind::Table));
        let canonical = g.add_object(KirObject::new("Customer", ObjectKind::Table));
        let other = g.add_object(KirObject::new("orders", ObjectKind::Table));
        g.add_relationship(KirRelationship::new(
            RelationshipKind::ForeignKey,
            other,
            old,
        ));

        let proposal = MergeProposal {
            canonical_id: canonical,
            canonical_name: "Customer".into(),
            canonical_kind: ObjectKind::Table,
            source_ids: vec![canonical, old],
            confidence: 1.0,
        };

        let resolved = apply_merges(g, &[proposal]);

        // Non-canonical object removed.
        assert!(!resolved.objects.iter().any(|o| o.id == old));
        // Relationship remapped to canonical.
        assert_eq!(resolved.relationships[0].to, canonical);
    }

    #[test]
    fn apply_merges_deduplicates_relationships() {
        let mut g = KirGraph::new();
        let a = g.add_object(KirObject::new("a", ObjectKind::Table));
        let b_old = g.add_object(KirObject::new("b_old", ObjectKind::Table));
        let b_new = g.add_object(KirObject::new("b", ObjectKind::Table));

        // Two FK rels pointing to old and new IDs of b; after remap both point to b_new.
        g.add_relationship(KirRelationship::new(RelationshipKind::ForeignKey, a, b_old));
        g.add_relationship(KirRelationship::new(RelationshipKind::ForeignKey, a, b_new));

        let proposal = MergeProposal {
            canonical_id: b_new,
            canonical_name: "b".into(),
            canonical_kind: ObjectKind::Table,
            source_ids: vec![b_new, b_old],
            confidence: 0.97,
        };

        let resolved = apply_merges(g, &[proposal]);
        assert_eq!(
            resolved.relationships.len(),
            1,
            "rels must deduplicate after remap"
        );
    }

    #[test]
    fn ckm_is_serializable() {
        let model = build_ckm(&two_object_graph());
        let json = serde_json::to_string_pretty(&model).unwrap();
        let back: CkModel = serde_json::from_str(&json).unwrap();
        assert_eq!(back.objects.len(), model.objects.len());
        assert_eq!(back.version, 1);
    }

    #[tokio::test]
    async fn semantic_compiler_pass_runs_on_empty_store() {
        use ekos_artifact::FileSystemArtifactStore;
        use ekos_compiler_core::{EkosConfig, pass::PassContext};
        use std::sync::Arc;

        let dir = TempDir::new().unwrap();
        let ckm_dir = dir.path().join("ckm");
        let store_dir = dir.path().join("artifacts");

        let config = Arc::new(EkosConfig::default());
        let store = Arc::new(FileSystemArtifactStore::new(&store_dir));
        let mut ctx = PassContext::new(config, dir.path().to_path_buf()).with_artifact_store(store);

        let mut pass = SemanticCompilerPass::new(&ckm_dir);
        pass.run(&mut ctx).await.unwrap();

        let model_path = ckm_dir.join("model.json.zst");
        assert!(
            model_path.exists(),
            "model.json.zst must be written (RFC 0015)"
        );

        let model: CkModel =
            ekos_common::compress::read_json_auto(&ckm_dir.join("model.json")).unwrap();
        assert_eq!(model.version, 1);
        assert!(model.objects.is_empty());
    }
}
