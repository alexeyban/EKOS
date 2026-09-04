//! Semantic compiler: resolved KIR → Canonical Knowledge Model (CKM).
//!
//! See Phase 8 in TODO.md. The CKM is the final, denormalised, validated output
//! of the compiler pipeline. Downstream consumers (Ledger, Runtime, AI) always
//! read from the CKM, never from raw KIR.

pub mod data_lineage;
pub mod rollup;
pub mod transform_ir;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_identity::{DefaultResolver, IdentityResolver, MergeProposal};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
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

    /// The distinct set of relationship `from`/`to` ids that don't resolve within this model's
    /// own object set — the same ids `validate()` reports as `"unknown from/to-id"` errors, but
    /// as real `KirId`s a caller can cross-reference against a broader "known ids" source instead
    /// of parsing `validate()`'s formatted message text.
    ///
    /// Exists because the CKM's own object set is deliberately narrower than the full, post-
    /// `commit` ledger: `File` objects are written straight to the ledger by `ekos build`, never
    /// through a `KnowledgeArtifact` this crate reads (see `SemanticCompilerPass::run`'s own
    /// comment), so any relationship pointing at a `File` — the overwhelming majority of real
    /// `DependsOn`/`Contains` edges recovery passes emit — is structurally guaranteed to be
    /// "unknown" here even though it resolves correctly once committed. `ekos compile`'s CLI
    /// layer (which does have ledger access) uses this to tell that expected, by-design gap apart
    /// from a genuinely dangling reference, instead of reporting every one as equally alarming.
    pub fn dangling_relationship_target_ids(&self) -> HashSet<KirId> {
        let object_ids: HashSet<KirId> = self.objects.iter().map(|o| o.id).collect();
        let mut missing = HashSet::new();
        for rel in &self.relationships {
            if !object_ids.contains(&rel.from) {
                missing.insert(rel.from);
            }
            if !object_ids.contains(&rel.to) {
                missing.insert(rel.to);
            }
        }
        missing
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

/// Split identity-resolution proposals into the two tiers RFC 0063 defines: proposals whose
/// group members share the exact same normalized name (`exact_name_match`) are safe to auto-apply
/// via `apply_merges` unchanged; everything else is a fuzzy, judgment-call match that must go
/// through human/agent review (`ekos_identity_review`) instead of being irreversibly merged into
/// the append-only ledger. See `MergeProposal::exact_name_match`'s doc comment for why a
/// confidence threshold alone can't make this split safely.
fn partition_proposals(proposals: Vec<MergeProposal>) -> (Vec<MergeProposal>, Vec<MergeProposal>) {
    proposals.into_iter().partition(|p| p.exact_name_match)
}

/// Build one `unconfirmed` `Custom("SameAs")` relationship + evidence pair per non-canonical
/// member of each fuzzy-match proposal (RFC 0063), in the same property shape
/// `crates/cli/src/commands/identity.rs::scan` already writes for cross-system candidates — so
/// `ekos_identity_review` handles these with no changes there. Star topology, canonical → member,
/// matching the group's existing canonical-centric shape.
fn review_candidates_for(proposals: &[MergeProposal]) -> (Vec<KirEvidence>, Vec<KirRelationship>) {
    let mut evidence = Vec::new();
    let mut relationships = Vec::new();

    for p in proposals {
        for &member_id in &p.source_ids {
            if member_id == p.canonical_id {
                continue;
            }
            let ev = KirEvidence::new(
                SourceLocation::file("ekos compile (identity resolution)"),
                format!(
                    "same-source merge candidate for '{}' ({}), confidence={:.2}",
                    p.canonical_name, p.canonical_kind, p.confidence
                ),
            )
            .with_confidence(p.confidence);
            let ev_id = ev.id;
            evidence.push(ev);

            let mut rel = KirRelationship::deterministic(
                RelationshipKind::Custom("SameAs".to_string()),
                p.canonical_id,
                member_id,
                "",
            );
            rel.properties
                .insert("status".into(), serde_json::json!("unconfirmed"));
            rel.properties
                .insert("confidence".into(), serde_json::json!(p.confidence));
            rel.evidence.push(ev_id);
            relationships.push(rel);
        }
    }

    (evidence, relationships)
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

/// RFC 0094 §"Threshold": the smallest real dependent count where "more than a pair" starts to
/// mean something — 1 or 2 dependents doesn't yet distinguish genuinely broad, coupled usage from
/// a couple of unrelated call sites. Not a tuned/calibrated number, an explicit, named floor.
const MIN_DEPENDENTS_FOR_CONCENTRATION_RISK: usize = 3;

/// Caps how many of a risk's real justifying `DependsOn` edges get cited as evidence — a
/// widely-used object can have dozens of real dependents; the point/count is already carried in
/// `dependent_count`, a handful of real citations is enough to substantiate it without an
/// unbounded evidence list.
const MAX_CONCENTRATION_RISK_EVIDENCE: usize = 5;

/// Deterministic id for one object's Observed Concentration Risk (RFC 0094), keyed by the target
/// object's own id — re-derived identically on every `compile` run, matching every other
/// deterministic-id precedent this codebase already established (RFC 0070/0071's fix for the
/// unbounded-duplicate-accumulation failure mode a non-deterministic id causes across repeated
/// runs).
fn concentration_risk_kir_id(target_id: KirId) -> KirId {
    KirId(uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        format!("risk:concentration:{target_id}").as_bytes(),
    ))
}

/// RFC 0094: one real, deterministically-derived `Custom("Risk")` object per compiled object that
/// is the target of `MIN_DEPENDENTS_FOR_CONCENTRATION_RISK` or more real `DependsOn` edges — a
/// heavy real fan-in is the structural signature of a single-point-of-failure candidate (RFC 0068
/// §29's own "tight coupling" technical-debt category). `risk_type: "observed"` only — no
/// inference, no fabricated severity score, just the real count and the real edges that produced
/// it. A `References` edge from the risk to the object it concerns makes it directly navigable.
fn concentration_risks(graph: &KirGraph) -> Vec<(KirObject, KirRelationship)> {
    let mut dependents_by_target: HashMap<KirId, Vec<&KirRelationship>> = HashMap::new();
    for rel in &graph.relationships {
        if rel.kind == RelationshipKind::DependsOn {
            dependents_by_target.entry(rel.to).or_default().push(rel);
        }
    }
    let objects_by_id: HashMap<KirId, &KirObject> =
        graph.objects.iter().map(|o| (o.id, o)).collect();

    let mut risks: Vec<(KirObject, KirRelationship)> = Vec::new();
    let mut target_ids: Vec<&KirId> = dependents_by_target.keys().collect();
    target_ids.sort_by_key(|id| id.to_string());
    for target_id in target_ids {
        let rels = &dependents_by_target[target_id];
        if rels.len() < MIN_DEPENDENTS_FOR_CONCENTRATION_RISK {
            continue;
        }
        // Honest: only a real, resolvable target ever gets a Risk object — never fabricated
        // against a dangling id.
        let Some(target) = objects_by_id.get(target_id) else {
            continue;
        };
        let count = rels.len();
        let risk_id = concentration_risk_kir_id(*target_id);
        let mut risk = KirObject::new(
            format!("Concentration risk: {}", target.name),
            ObjectKind::Custom("Risk".to_string()),
        )
        .with_property("risk_type", serde_json::json!("observed"))
        .with_property(
            "statement",
            serde_json::json!(format!(
                "'{}' has {count} real compiled dependent(s)",
                target.name
            )),
        )
        .with_property("dependent_count", serde_json::json!(count));
        risk.id = risk_id;
        for ev_id in rels
            .iter()
            .flat_map(|r| r.evidence.iter().copied())
            .take(MAX_CONCENTRATION_RISK_EVIDENCE)
        {
            risk.evidence.push(ev_id);
        }
        let rel =
            KirRelationship::deterministic(RelationshipKind::References, risk_id, *target_id, "");
        risks.push((risk, rel));
    }
    risks
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

        // RFC 0063: only exact-normalized-name groups are safe to auto-merge (irreversibly, into
        // an append-only ledger with no object-level delete/tombstone). Fuzzy groups — which RFC
        // 0060 showed cannot be safely separated from exact ones by confidence alone — become
        // `unconfirmed` `Custom("SameAs")` relationships instead, reviewable via
        // `ekos_identity_review`, same as RFC 0029's cross-system candidates.
        let (auto_merge, review) = partition_proposals(resolution.proposals);

        tracing::info!(
            proposals = resolution.stats.merges_proposed,
            auto_merged = auto_merge.len(),
            sent_to_review = review.len(),
            conflicts = resolution.stats.conflicts_detected,
            "identity resolution complete"
        );
        if !review.is_empty() {
            ctx.diagnostics.lock().unwrap().warning(
                "SEM003",
                format!(
                    "{} same-source merge candidate(s) were fuzzy name matches — sent to review \
                     as unconfirmed SameAs relationships instead of auto-merging (RFC 0063); use \
                     ekos_identity_review to confirm or reject",
                    review.len()
                ),
            );
        }

        // ── Apply merges ──────────────────────────────────────────────────────
        let mut resolved = apply_merges(combined, &auto_merge);

        // ── Review candidates ───────────────────────────────────────────────────
        let (review_evidence, review_relationships) = review_candidates_for(&review);
        resolved.evidence.extend(review_evidence);
        resolved.relationships.extend(review_relationships);

        // Hierarchical rollups (RFC 0044) intentionally do NOT run here: `File` objects — the
        // only kind rollups group by directly — are written straight to the ledger by `ekos
        // build` (`cli/src/commands/build.rs`), never through a `KnowledgeArtifact` this pass
        // reads. `combined`/`resolved` above only ever contain recovery-pass output, so rollup
        // synthesis runs in `ekos commit` instead, against the ledger's full post-commit object
        // set (which by then includes both `ekos build`'s `File` objects and this pass's own
        // CKM output) — see `cli/src/commands/commit.rs`.

        // ── Observed Concentration Risk (RFC 0094) ──────────────────────────────
        // Unlike rollups above, this only needs `DependsOn` edges + their targets — both already
        // fully present in `resolved` at this point, no `File`-object dependency to defer for.
        for (risk, rel) in concentration_risks(&resolved) {
            resolved.add_object(risk);
            resolved.add_relationship(rel);
        }

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
        KirEvidence, KirObject, KirRelationship, ObjectKind, RelationshipKind, SourceLocation,
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
    fn dangling_relationship_target_ids_returns_the_real_missing_ids() {
        let mut model = build_ckm(&two_object_graph());
        let phantom_to = KirId::new();
        let phantom_from = KirId::new();
        model.relationships.push(CkmRelationship {
            id: KirId::new(),
            kind: RelationshipKind::References,
            from: model.objects[0].id,
            to: phantom_to,
            properties: HashMap::new(),
            evidence: vec![],
        });
        model.relationships.push(CkmRelationship {
            id: KirId::new(),
            kind: RelationshipKind::References,
            from: phantom_from,
            to: model.objects[0].id,
            properties: HashMap::new(),
            evidence: vec![],
        });
        let missing = model.dangling_relationship_target_ids();
        assert_eq!(missing.len(), 2);
        assert!(missing.contains(&phantom_to));
        assert!(missing.contains(&phantom_from));
        assert!(!missing.contains(&model.objects[0].id));
    }

    #[test]
    fn dangling_relationship_target_ids_is_empty_for_a_valid_ckm() {
        let model = build_ckm(&two_object_graph());
        assert!(model.dangling_relationship_target_ids().is_empty());
    }

    /// Builds a graph with `dependent_count` real `File`-ish objects each `DependsOn` one shared
    /// `target` (a `Custom("Technology")`), each edge carrying its own real evidence — the shape
    /// `concentration_risks` is meant to detect.
    fn graph_with_fan_in(target_name: &str, dependent_count: usize) -> (KirGraph, KirId) {
        let mut g = KirGraph::new();
        // A deterministic id (not `KirObject::new`'s random default) — mirrors how a real
        // Technology object's id is actually derived in production (name-keyed, e.g.
        // `dependency_analyzer.rs`'s `technology_kir_id`), so two separately-built graphs for the
        // same real name produce the same target id, the precondition this test's own
        // determinism check needs.
        let mut target_obj =
            KirObject::new(target_name, ObjectKind::Custom("Technology".to_string()));
        target_obj.id = KirId(uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            format!("test-technology:{target_name}").as_bytes(),
        ));
        let target = g.add_object(target_obj);
        for i in 0..dependent_count {
            let ev = KirEvidence::new(
                SourceLocation::at(format!("file{i}.py"), 1),
                format!("import {target_name}"),
            );
            let ev_id = g.add_evidence(ev);
            let dep = g.add_object(KirObject::new(format!("file{i}.py"), ObjectKind::File));
            let mut rel = KirRelationship::new(RelationshipKind::DependsOn, dep, target);
            rel.evidence.push(ev_id);
            g.add_relationship(rel);
        }
        (g, target)
    }

    #[test]
    fn below_threshold_fan_in_produces_no_risk() {
        let (g, _target) =
            graph_with_fan_in("small-lib", MIN_DEPENDENTS_FOR_CONCENTRATION_RISK - 1);
        assert!(concentration_risks(&g).is_empty());
    }

    #[test]
    fn at_threshold_fan_in_produces_exactly_one_observed_risk() {
        let (g, target) = graph_with_fan_in("popular-lib", MIN_DEPENDENTS_FOR_CONCENTRATION_RISK);
        let risks = concentration_risks(&g);
        assert_eq!(risks.len(), 1);
        let (risk, rel) = &risks[0];
        assert_eq!(risk.kind, ObjectKind::Custom("Risk".to_string()));
        assert_eq!(risk.properties["risk_type"], "observed");
        assert_eq!(
            risk.properties["dependent_count"],
            MIN_DEPENDENTS_FOR_CONCENTRATION_RISK
        );
        assert!(
            risk.properties["statement"]
                .as_str()
                .unwrap()
                .contains("popular-lib")
        );
        assert_eq!(rel.kind, RelationshipKind::References);
        assert_eq!(rel.from, risk.id);
        assert_eq!(rel.to, target);
        assert!(!risk.evidence.is_empty());
    }

    #[test]
    fn concentration_risk_id_is_deterministic_across_separate_runs() {
        let (g1, _) = graph_with_fan_in("popular-lib", MIN_DEPENDENTS_FOR_CONCENTRATION_RISK);
        let (g2, _) = graph_with_fan_in("popular-lib", MIN_DEPENDENTS_FOR_CONCENTRATION_RISK);
        let r1 = concentration_risks(&g1);
        let r2 = concentration_risks(&g2);
        assert_eq!(r1[0].0.id, r2[0].0.id);
    }

    #[test]
    fn concentration_risk_evidence_is_capped() {
        let (g, _) = graph_with_fan_in("very-popular-lib", MAX_CONCENTRATION_RISK_EVIDENCE + 10);
        let risks = concentration_risks(&g);
        assert_eq!(risks.len(), 1);
        assert_eq!(risks[0].0.evidence.len(), MAX_CONCENTRATION_RISK_EVIDENCE);
        assert_eq!(
            risks[0].0.properties["dependent_count"],
            MAX_CONCENTRATION_RISK_EVIDENCE + 10
        );
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
            exact_name_match: true,
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
            exact_name_match: true,
        };

        let resolved = apply_merges(g, &[proposal]);
        assert_eq!(
            resolved.relationships.len(),
            1,
            "rels must deduplicate after remap"
        );
    }

    #[test]
    fn partition_proposals_splits_exact_from_fuzzy() {
        let exact = MergeProposal {
            canonical_id: KirId::new(),
            canonical_name: "Customer".into(),
            canonical_kind: ObjectKind::Table,
            source_ids: vec![KirId::new(), KirId::new()],
            confidence: 1.0,
            exact_name_match: true,
        };
        let fuzzy = MergeProposal {
            canonical_id: KirId::new(),
            canonical_name: "RobertJoonas".into(),
            canonical_kind: ObjectKind::Person,
            source_ids: vec![KirId::new(), KirId::new()],
            confidence: 0.93,
            exact_name_match: false,
        };

        let (auto, review) = partition_proposals(vec![exact.clone(), fuzzy.clone()]);
        assert_eq!(auto.len(), 1);
        assert_eq!(auto[0].canonical_name, "Customer");
        assert_eq!(review.len(), 1);
        assert_eq!(review[0].canonical_name, "RobertJoonas");
    }

    #[test]
    fn review_candidates_for_produces_unconfirmed_same_as_relationships() {
        let canonical = KirId::new();
        let member = KirId::new();
        let proposal = MergeProposal {
            canonical_id: canonical,
            canonical_name: "RobertJoonas".into(),
            canonical_kind: ObjectKind::Person,
            source_ids: vec![canonical, member],
            confidence: 0.93,
            exact_name_match: false,
        };

        let (evidence, relationships) = review_candidates_for(&[proposal]);
        assert_eq!(evidence.len(), 1);
        assert_eq!(relationships.len(), 1);

        let rel = &relationships[0];
        assert!(matches!(&rel.kind, RelationshipKind::Custom(k) if k == "SameAs"));
        assert_eq!(rel.from, canonical);
        assert_eq!(rel.to, member);
        assert_eq!(rel.properties["status"], "unconfirmed");
        assert_eq!(rel.properties["confidence"].as_f64().unwrap() as f32, 0.93);
        assert_eq!(rel.evidence, vec![evidence[0].id]);
    }

    #[test]
    fn fuzzy_proposals_do_not_delete_objects_but_do_add_a_review_relationship() {
        // End-to-end shape of the RFC 0063 fix: a fuzzy proposal must leave both objects in the
        // resolved graph (never irreversibly merged into an append-only ledger) while still
        // producing a reviewable SameAs relationship.
        let mut g = KirGraph::new();
        let canonical = g.add_object(KirObject::new("RobertJoonas", ObjectKind::Person));
        let member = g.add_object(KirObject::new("Robert", ObjectKind::Person));

        let proposal = MergeProposal {
            canonical_id: canonical,
            canonical_name: "RobertJoonas".into(),
            canonical_kind: ObjectKind::Person,
            source_ids: vec![canonical, member],
            confidence: 0.93,
            exact_name_match: false,
        };

        let (auto, review) = partition_proposals(vec![proposal]);
        assert!(auto.is_empty());

        let mut resolved = apply_merges(g, &auto);
        let (review_evidence, review_relationships) = review_candidates_for(&review);
        resolved.evidence.extend(review_evidence);
        resolved.relationships.extend(review_relationships);

        assert_eq!(
            resolved.objects.len(),
            2,
            "both objects must survive, unmerged"
        );
        assert_eq!(resolved.relationships.len(), 1);
        assert!(matches!(
            &resolved.relationships[0].kind,
            RelationshipKind::Custom(k) if k == "SameAs"
        ));
        assert_eq!(
            resolved.relationships[0].properties["status"],
            "unconfirmed"
        );

        // And the review relationship survives straight through build_ckm.
        let model = build_ckm(&resolved);
        assert_eq!(model.objects.len(), 2);
        assert_eq!(model.relationships.len(), 1);
        assert_eq!(model.relationships[0].properties["status"], "unconfirmed");
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

#[cfg(test)]
mod relationship_determinism_guard {
    //! RFC 0135 Part C — see the identical guard in `ekos_recovery`. Every persisted
    //! `KirRelationship` this crate emits (rollups, data-lineage links, concentration-risk edges,
    //! `SameAs` merge proposals, `FeedsInto`) must carry a deterministic id.

    fn strip_test_modules(src: &str) -> String {
        let mut out = String::new();
        let mut rest = src;
        while let Some(pos) = rest.find("#[cfg(test)]") {
            let (before, after) = rest.split_at(pos);
            out.push_str(before);
            let Some(brace) = after.find('{') else {
                out.push_str(after);
                return out;
            };
            let mut depth = 1usize;
            let mut idx = brace + 1;
            let bytes = after.as_bytes();
            while idx < bytes.len() && depth > 0 {
                match bytes[idx] {
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    _ => {}
                }
                idx += 1;
            }
            rest = &after[idx..];
        }
        out.push_str(rest);
        out
    }

    #[test]
    fn no_bare_relationship_new_in_production_code() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut bad = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let src = strip_test_modules(&std::fs::read_to_string(&path).unwrap());
            let mut from = 0;
            while let Some(rel) = src[from..].find("KirRelationship::new(") {
                let at = from + rel;
                let window = &src[at..(at + 600).min(src.len())];
                if !window.contains(".id =") && !window.contains(".id=") {
                    let line = src[..at].matches('\n').count() + 1;
                    bad.push(format!(
                        "{}:{}",
                        path.file_name().unwrap().to_string_lossy(),
                        line
                    ));
                }
                from = at + 1;
            }
        }
        assert!(
            bad.is_empty(),
            "bare `KirRelationship::new(` in production code — use `KirRelationship::deterministic` \
             (RFC 0135 Part C): {bad:?}"
        );
    }
}
