//! `CryptoAnalyzerPass` — converts DeFi Sentinel crypto export observation
//! artifacts (RFC 0017) into KIR. Unlike `SqlAnalyzerPass`, there is no LLM
//! enrichment step: the producer already assigns kind/name, so this pass is
//! pure structural mapping (entity_id/relationship kind -> KIR, verbatim).

use std::collections::HashMap;

use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use uuid::Uuid;

/// Namespace for deterministic KirIds derived from the export's own stable
/// entity_id/evidence_id strings — the same wallet/token appearing across many
/// 15-minute batches must resolve to the same KirObject, not near-duplicates.
const NAMESPACE: Uuid = Uuid::NAMESPACE_URL;

pub struct CryptoAnalyzerPass {
    pass_id: String,
    /// Crypto ObservationArtifact IDs to process (one per export batch).
    batch_artifact_ids: Vec<ArtifactId>,
}

impl CryptoAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, batch_artifact_ids: Vec<ArtifactId>) -> Self {
        let workspace_name = workspace_name.into();
        Self {
            pass_id: format!("crypto-analyzer:{workspace_name}"),
            batch_artifact_ids,
        }
    }
}

#[async_trait]
impl CompilerPass for CryptoAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .batch_artifact_ids
            .iter()
            .map(|id| id.to_string())
            .collect();
        ids.sort();
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut graph = KirGraph::new();
        // Export-side id -> KirId, so relationships (processed after all
        // entities/evidence in a batch) can resolve their endpoints.
        let mut entity_kir_ids: HashMap<String, KirId> = HashMap::new();
        let mut evidence_kir_ids: HashMap<String, KirId> = HashMap::new();

        for artifact_id in &self.batch_artifact_ids {
            let json = match ctx.artifact_store.read(artifact_id) {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "CRYPTO001",
                        format!("cannot read artifact {artifact_id}: {e}"),
                    );
                    continue;
                }
            };

            let batch: BatchData = match serde_json::from_value(json["data"].clone()) {
                Ok(b) => b,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "CRYPTO002",
                        format!("malformed crypto batch payload in {artifact_id}: {e}"),
                    );
                    continue;
                }
            };

            // Evidence first — relationships reference it below.
            for ev in &batch.evidence {
                if evidence_kir_ids.contains_key(&ev.evidence_id) {
                    continue;
                }
                let kir_id = deterministic_id("crypto-evidence", &ev.evidence_id);
                let mut kir_ev = KirEvidence::new(
                    SourceLocation::file(ev.reference.clone()),
                    format!("{}: {}", ev.evidence_type, ev.payload),
                );
                kir_ev.id = kir_id;
                evidence_kir_ids.insert(ev.evidence_id.clone(), kir_id);
                graph.add_evidence(kir_ev);
            }

            // Objects.
            for ent in &batch.entities {
                if entity_kir_ids.contains_key(&ent.entity_id) {
                    continue;
                }
                let kir_id = deterministic_id("crypto-entity", &ent.entity_id);
                let mut obj = KirObject::new(&ent.name, ObjectKind::Custom(ent.kind.clone()));
                obj.id = kir_id;
                obj.properties = parse_attrs(&ent.attrs);
                obj.properties.insert(
                    "entity_id".into(),
                    serde_json::Value::String(ent.entity_id.clone()),
                );
                entity_kir_ids.insert(ent.entity_id.clone(), kir_id);
                graph.add_object(obj);
            }

            // Relationships — skip (with a diagnostic) if either endpoint
            // wasn't in this batch's entities; exports are batch-scoped and
            // an entity can in principle land in a different batch.
            for rel in &batch.relationships {
                let (Some(&from), Some(&to)) = (
                    entity_kir_ids.get(&rel.src_entity_id),
                    entity_kir_ids.get(&rel.dst_entity_id),
                ) else {
                    ctx.diagnostics.lock().unwrap().warning(
                        "CRYPTO003",
                        format!(
                            "relationship {} -> {} ({}) references an entity outside this batch; skipped",
                            rel.src_entity_id, rel.dst_entity_id, rel.kind
                        ),
                    );
                    continue;
                };

                let mut kir_rel =
                    KirRelationship::new(RelationshipKind::Custom(rel.kind.clone()), from, to);
                kir_rel.properties = parse_attrs(&rel.attrs);
                for ev_id in &rel.evidence_ids {
                    if let Some(&kid) = evidence_kir_ids.get(ev_id) {
                        kir_rel.evidence.push(kid);
                    }
                }
                if kir_rel.evidence.is_empty() {
                    // The export contract requires non-empty evidence_ids; if none
                    // resolved, the payload disagreed with its own manifest — worth
                    // a diagnostic, but not worth dropping the relationship over.
                    ctx.diagnostics.lock().unwrap().warning(
                        "CRYPTO004",
                        format!(
                            "relationship {} -> {} ({}) has no resolvable evidence",
                            rel.src_entity_id, rel.dst_entity_id, rel.kind
                        ),
                    );
                }
                graph.add_relationship(kir_rel);
            }
        }

        let knowledge = ekos_artifact::KnowledgeArtifact::new(
            &self.pass_id,
            self.batch_artifact_ids.clone(),
            graph,
        );
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            objects = knowledge.content.kir.objects.len(),
            relationships = knowledge.content.kir.relationships.len(),
            evidence = knowledge.content.kir.evidence.len(),
            "crypto-analyzer complete"
        );
        Ok(())
    }
}

fn deterministic_id(kind: &str, seed: &str) -> KirId {
    KirId(Uuid::new_v5(
        &NAMESPACE,
        format!("{kind}:{seed}").as_bytes(),
    ))
}

fn parse_attrs(attrs: &str) -> HashMap<String, serde_json::Value> {
    match serde_json::from_str::<serde_json::Value>(attrs) {
        Ok(serde_json::Value::Object(map)) => map.into_iter().collect(),
        _ => HashMap::new(),
    }
}

// ── Export payload shapes (mirrors ekos-plugin-crypto's EntityRecord/etc.,
// duplicated rather than depended-on: recovery reads artifact JSON generically,
// the same way GitAnalyzerPass never imports ekos-plugin-git's types) ─────────

#[derive(serde::Deserialize)]
struct BatchData {
    #[allow(dead_code)]
    batch_id: String,
    entities: Vec<EntityRow>,
    relationships: Vec<RelationshipRow>,
    evidence: Vec<EvidenceRow>,
}

#[derive(serde::Deserialize)]
struct EntityRow {
    entity_id: String,
    kind: String,
    name: String,
    attrs: String,
}

#[derive(serde::Deserialize)]
struct RelationshipRow {
    src_entity_id: String,
    dst_entity_id: String,
    kind: String,
    attrs: String,
    evidence_ids: Vec<String>,
}

#[derive(serde::Deserialize)]
struct EvidenceRow {
    evidence_id: String,
    evidence_type: String,
    reference: String,
    payload: String,
    #[allow(dead_code)]
    source: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_artifact::{ArtifactStore, ObservationArtifact, PackArtifactStore};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn make_ctx(dir: &TempDir) -> PassContext {
        let mut config = ekos_compiler_core::EkosConfig::default();
        config.observe.ignore_patterns = vec![];
        let cwd = dir.path().to_path_buf();
        std::fs::create_dir_all(cwd.join(".ekos/artifacts")).unwrap();
        PassContext::new(Arc::new(config), cwd)
    }

    fn seed_batch_artifact(ctx: &PassContext, batch_json: serde_json::Value) -> ArtifactId {
        let artifact = ObservationArtifact::new("crypto", "20260720T1200", batch_json);
        let json = serde_json::to_value(&artifact).unwrap();
        ctx.artifact_store.write(&artifact.id, &json).unwrap();
        artifact.id
    }

    fn sample_batch_json() -> serde_json::Value {
        serde_json::json!({
            "batch_id": "20260720T1200",
            "entities": [
                {"entity_id": "wallet:solana:Dep1", "kind": "Wallet", "name": "Dep1", "attrs": "{}"},
                {"entity_id": "token:solana:Mint1", "kind": "Token", "name": "Fixture", "attrs": "{\"risk\": 65}"},
            ],
            "relationships": [
                {
                    "src_entity_id": "wallet:solana:Dep1",
                    "dst_entity_id": "token:solana:Mint1",
                    "kind": "DEPLOYED",
                    "attrs": "{}",
                    "evidence_ids": ["ev1"],
                },
            ],
            "evidence": [
                {
                    "evidence_id": "ev1",
                    "evidence_type": "api_response",
                    "reference": "birdeye:defi/token_creation_info",
                    "payload": "{\"deployer_wallet\": \"Dep1\"}",
                    "source": "birdeye",
                },
            ],
        })
    }

    #[tokio::test]
    async fn maps_entities_relationships_and_evidence_to_kir() {
        let dir = TempDir::new().unwrap();
        let mut ctx = make_ctx(&dir);
        let artifact_id = seed_batch_artifact(&ctx, sample_batch_json());

        let mut pass = CryptoAnalyzerPass::new("test", vec![artifact_id]);
        pass.run(&mut ctx).await.unwrap();
        assert!(!ctx.diagnostics.lock().unwrap().has_errors());

        // Re-read the KnowledgeArtifact this pass wrote.
        let store: Arc<dyn ArtifactStore> =
            Arc::new(PackArtifactStore::open(ctx.config.artifact_dir(&ctx.cwd)).unwrap());
        let ids = store.list().unwrap();
        let knowledge_json = ids
            .iter()
            .find_map(|id| {
                let j = store.read(id).ok()??;
                (j["artifact_type"] == "knowledge").then_some(j)
            })
            .expect("a KnowledgeArtifact should have been written");

        let objects = knowledge_json["kir"]["objects"].as_array().unwrap();
        assert_eq!(objects.len(), 2);
        let token_obj = objects
            .iter()
            .find(|o| {
                o["kind"] == "token:solana:Mint1"
                    || o["properties"]["entity_id"] == "token:solana:Mint1"
            })
            .expect("token object present");
        assert_eq!(token_obj["kind"], "Token");
        assert_eq!(token_obj["properties"]["risk"], 65);

        let rels = knowledge_json["kir"]["relationships"].as_array().unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0]["kind"], "DEPLOYED");
        assert_eq!(rels[0]["evidence"].as_array().unwrap().len(), 1);

        let evidence = knowledge_json["kir"]["evidence"].as_array().unwrap();
        assert_eq!(evidence.len(), 1);
    }

    #[tokio::test]
    async fn relationship_referencing_missing_entity_is_skipped_not_fatal() {
        let dir = TempDir::new().unwrap();
        let mut ctx = make_ctx(&dir);
        let batch = serde_json::json!({
            "batch_id": "b1",
            "entities": [
                {"entity_id": "token:solana:Mint1", "kind": "Token", "name": "Fixture", "attrs": "{}"},
            ],
            "relationships": [
                {
                    "src_entity_id": "wallet:solana:Unknown",
                    "dst_entity_id": "token:solana:Mint1",
                    "kind": "DEPLOYED",
                    "attrs": "{}",
                    "evidence_ids": [],
                },
            ],
            "evidence": [],
        });
        let artifact_id = seed_batch_artifact(&ctx, batch);

        let mut pass = CryptoAnalyzerPass::new("test", vec![artifact_id]);
        pass.run(&mut ctx).await.unwrap();

        assert!(!ctx.diagnostics.lock().unwrap().has_errors());
        assert!(ctx.diagnostics.lock().unwrap().has_warnings());
    }

    #[tokio::test]
    async fn same_entity_across_two_batches_gets_same_kir_id() {
        let dir = TempDir::new().unwrap();
        let mut ctx = make_ctx(&dir);
        let batch = serde_json::json!({
            "batch_id": "b1",
            "entities": [
                {"entity_id": "token:solana:Mint1", "kind": "Token", "name": "Fixture", "attrs": "{}"},
            ],
            "relationships": [],
            "evidence": [],
        });
        let id1 = deterministic_id("crypto-entity", "token:solana:Mint1");
        let artifact_a = seed_batch_artifact(&ctx, batch.clone());
        let artifact_b = seed_batch_artifact(&ctx, batch);

        let mut pass = CryptoAnalyzerPass::new("test", vec![artifact_a, artifact_b]);
        pass.run(&mut ctx).await.unwrap();

        let store: Arc<dyn ArtifactStore> =
            Arc::new(PackArtifactStore::open(ctx.config.artifact_dir(&ctx.cwd)).unwrap());
        let ids = store.list().unwrap();
        let knowledge_json = ids
            .iter()
            .find_map(|id| {
                let j = store.read(id).ok()??;
                (j["artifact_type"] == "knowledge").then_some(j)
            })
            .unwrap();
        let objects = knowledge_json["kir"]["objects"].as_array().unwrap();
        // Deduped across both batches — one object, not two — and its id is the
        // deterministic one derived from the entity_id.
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0]["id"], id1.to_string());
    }
}
