//! `TreasuryAnalyzerPass` — converts treasury observation artifacts (RFC 0032) into KIR: one
//! `Custom("TreasuryPayment")` object per outgoing on-chain transfer, each with an evidence record
//! citing the exact transaction. Pure structural mapping — no LLM. It deliberately emits **no
//! relationships**: whether a payment was authorized is decided (as an unconfirmed candidate) by
//! `ekos treasury scan` over committed ledger objects, never by a compiler pass.
//!
//! Property contract read by `ekos_identity::treasury`: `tx_hash`, `index`, `chain_id`, `from`,
//! `to`, `value` (whole-unit decimal string), `token`, `memo`, `timestamp` (RFC 3339),
//! `block_number`.

use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{KirEvidence, KirGraph, KirId, KirObject, ObjectKind, SourceLocation};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct PaymentData {
    chain_id: u64,
    tx_hash: String,
    #[serde(default)]
    index: u32,
    from: String,
    to: String,
    value: String,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    memo: Option<String>,
    timestamp: String,
    #[serde(default)]
    block_number: u64,
}

/// Deterministic id for one on-chain transfer — stable across `ekos recover` runs, so a re-run
/// converges instead of duplicating. Case-insensitive on the hash (explorers disagree on casing).
pub fn payment_kir_id(chain_id: u64, tx_hash: &str, index: u32) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!(
            "chain:{chain_id}:tx:{}:{index}",
            tx_hash.to_ascii_lowercase()
        )
        .as_bytes(),
    ))
}

pub struct TreasuryAnalyzerPass {
    pass_id: String,
    artifact_ids: Vec<ArtifactId>,
}

impl TreasuryAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, artifact_ids: Vec<ArtifactId>) -> Self {
        Self {
            pass_id: format!("treasury-analyzer:{}", workspace_name.into()),
            artifact_ids,
        }
    }
}

#[async_trait]
impl CompilerPass for TreasuryAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.artifact_ids.iter().map(|i| i.to_string()).collect();
        ids.sort();
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut graph = KirGraph::new();
        for artifact_id in &self.artifact_ids {
            let json = match ctx.artifact_store.read(artifact_id) {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    ctx.diagnostics
                        .lock()
                        .unwrap()
                        .warning("TRS001", format!("cannot read artifact {artifact_id}: {e}"));
                    continue;
                }
            };
            let d: PaymentData = match serde_json::from_value(json["data"].clone()) {
                Ok(d) => d,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "TRS002",
                        format!("malformed treasury payload in {artifact_id}: {e}"),
                    );
                    continue;
                }
            };
            let asset = d.token.clone().unwrap_or_else(|| "native".into());
            let mut obj = KirObject::new(
                format!("{} {asset} to {} ({})", d.value, d.to, d.tx_hash),
                ObjectKind::Custom("TreasuryPayment".into()),
            );
            obj.id = payment_kir_id(d.chain_id, &d.tx_hash, d.index);
            for (k, v) in [
                ("chain_id", serde_json::json!(d.chain_id)),
                ("tx_hash", serde_json::json!(d.tx_hash)),
                ("index", serde_json::json!(d.index)),
                ("from", serde_json::json!(d.from)),
                ("to", serde_json::json!(d.to)),
                ("value", serde_json::json!(d.value)),
                ("token", serde_json::json!(d.token)),
                ("memo", serde_json::json!(d.memo)),
                ("timestamp", serde_json::json!(d.timestamp)),
                ("block_number", serde_json::json!(d.block_number)),
            ] {
                obj.properties.insert(k.into(), v);
            }
            let ev = KirEvidence::new(
                SourceLocation::file(format!("chain:{}:tx:{}", d.chain_id, d.tx_hash)),
                format!(
                    "tx {} (transfer #{}) block {}: {} {asset} from {} to {} at {}",
                    d.tx_hash, d.index, d.block_number, d.value, d.from, d.to, d.timestamp
                ),
            );
            obj.evidence.push(graph.add_evidence(ev));
            graph.objects.push(obj);
        }

        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], graph);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;
        tracing::info!(pass = %self.pass_id, payments = knowledge.content.kir.objects.len(), "treasury-analyzer complete");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_compiler_core::EkosConfig;
    use std::sync::Arc;

    fn seed(ctx: &PassContext, tx: &str, index: u32, to: &str, value: &str) -> ArtifactId {
        let data = serde_json::json!({
            "chain_id": 1, "treasury_address": "0xSAFE", "tx_hash": tx, "index": index,
            "from": "0xSAFE", "to": to, "value": value, "token": "USDC", "memo": null,
            "timestamp": "2026-03-10T12:00:00+00:00", "block_number": 100,
        });
        let a = ekos_artifact::ObservationArtifact::new(
            "treasury",
            format!("chain:1:tx:{tx}:{index}"),
            data,
        );
        ctx.artifact_store
            .write(&a.id, &serde_json::to_value(&a).unwrap())
            .unwrap();
        a.id
    }

    async fn run(seeds: &[(&str, u32, &str, &str)]) -> KirGraph {
        let dir = tempfile::tempdir().unwrap();
        let mut c = PassContext::new(Arc::new(EkosConfig::default()), dir.path().to_path_buf());
        let ids = seeds
            .iter()
            .map(|(t, i, to, v)| seed(&c, t, *i, to, v))
            .collect();
        TreasuryAnalyzerPass::new("test", ids)
            .run(&mut c)
            .await
            .unwrap();
        let id = c
            .artifact_store
            .list()
            .unwrap()
            .into_iter()
            .find(|id| {
                c.artifact_store
                    .read(id)
                    .unwrap()
                    .unwrap()
                    .get("kir")
                    .is_some()
            })
            .expect("a KnowledgeArtifact");
        let k: ekos_artifact::KnowledgeArtifact =
            serde_json::from_value(c.artifact_store.read(&id).unwrap().unwrap()).unwrap();
        k.content.kir
    }

    #[tokio::test]
    async fn one_payment_object_per_transfer_with_the_property_contract_and_evidence() {
        let g = run(&[("0xAA", 0, "0xrecipient", "50000")]).await;
        assert_eq!(g.objects.len(), 1);
        let o = &g.objects[0];
        assert!(matches!(&o.kind, ObjectKind::Custom(k) if k == "TreasuryPayment"));
        assert_eq!(o.properties["value"], "50000");
        assert_eq!(o.properties["to"], "0xrecipient");
        assert_eq!(o.properties["timestamp"], "2026-03-10T12:00:00+00:00");
        assert_eq!(o.evidence.len(), 1, "every payment cites its transaction");
        assert!(
            g.relationships.is_empty(),
            "authorization is never decided by a compiler pass"
        );
    }

    #[tokio::test]
    async fn batch_sub_transfers_are_distinct_objects_and_ids_are_stable() {
        let g = run(&[("0xBATCH", 0, "0xr1", "10"), ("0xBATCH", 1, "0xr2", "20")]).await;
        assert_eq!(g.objects.len(), 2);
        assert_ne!(g.objects[0].id, g.objects[1].id);
        assert_eq!(
            payment_kir_id(1, "0xBATCH", 1),
            payment_kir_id(1, "0xbatch", 1),
            "hash case must not matter"
        );
        assert_ne!(
            payment_kir_id(1, "0xbatch", 0),
            payment_kir_id(2, "0xbatch", 0),
            "chain matters"
        );
    }
}
