//! `GovernanceAnalyzerPass` — converts governance-proposal observation artifacts (RFC 0032) into
//! KIR: one `Custom("GovernanceProposal")` object per proposal. Pure structural mapping plus
//! deliberately conservative text extraction — no LLM.
//!
//! **Extraction is best-effort and refuses to guess.** A proposal body is prose. A recipient
//! address or an amount is recorded only when the body contains exactly one distinct candidate;
//! two addresses, or "50,000 USDC" and later "10,000 USDC", leave the field unset, because a
//! wrong `approved_amount` would make the matcher assert something the proposal never said.
//! Every transaction hash the text cites is kept (a citation is only ever evidence *for* a link).
//!
//! Property contract read by `ekos_identity::treasury`: `proposal_id`, `title`, `outcome`,
//! `approved_at` (only when `outcome == "approved"`), `approved_recipient`, `approved_amount`,
//! `approved_token`, `tx_hashes_mentioned`.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{KirEvidence, KirGraph, KirId, KirObject, ObjectKind, SourceLocation};
use regex::Regex;
use serde::Deserialize;
use uuid::Uuid;

const BODY_EXCERPT_MAX_CHARS: usize = 600;
const ZERO_ADDRESS: &str = "0x0000000000000000000000000000000000000000";

#[derive(Debug, Deserialize)]
struct ProposalData {
    space: String,
    proposal_id: String,
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    outcome: String,
    end: String,
}

pub fn proposal_kir_id(space: &str, proposal_id: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("snapshot:{space}:{}", proposal_id.to_ascii_lowercase()).as_bytes(),
    ))
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("static regex"))
}

/// The one recipient address the body names, or `None` if it names none or several.
pub fn extract_recipient(body: &str) -> Option<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    let found: BTreeSet<String> = re(&R, r"0x[0-9a-fA-F]{40}\b")
        .find_iter(body)
        .map(|m| m.as_str().to_ascii_lowercase())
        .filter(|a| a != ZERO_ADDRESS)
        .collect();
    (found.len() == 1).then(|| found.into_iter().next().unwrap())
}

/// The one `(amount, token)` the body states — `"50,000 USDC"`, `"1.5M DAI"`, `"20k OP"` — or
/// `None` if it states none or several different ones. A bare "$10k" has no token and is ignored.
pub fn extract_amount(body: &str) -> Option<(String, String)> {
    static R: OnceLock<Regex> = OnceLock::new();
    let r = re(
        &R,
        r"(?i)\b(\d{1,3}(?:,\d{3})+|\d+(?:\.\d+)?)\s*([km])?\s*(USDC|USDT|DAI|ETH|WETH|OP|ARB|MATIC|SOL)\b",
    );
    let found: BTreeSet<(String, String)> = r
        .captures_iter(body)
        .filter_map(|c| {
            let base: f64 = c[1].replace(',', "").parse().ok()?;
            let scale = match c.get(2).map(|m| m.as_str().to_ascii_lowercase()).as_deref() {
                Some("k") => 1e3,
                Some("m") => 1e6,
                _ => 1.0,
            };
            let v = format!("{:.6}", base * scale);
            let v = v.trim_end_matches('0').trim_end_matches('.').to_string();
            Some((v, c[3].to_ascii_uppercase()))
        })
        .collect();
    (found.len() == 1).then(|| found.into_iter().next().unwrap())
}

/// Every 32-byte hex hash the text cites, lowercased and de-duplicated.
pub fn extract_tx_hashes(body: &str) -> Vec<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    let set: BTreeSet<String> = re(&R, r"0x[0-9a-fA-F]{64}\b")
        .find_iter(body)
        .map(|m| m.as_str().to_ascii_lowercase())
        .collect();
    set.into_iter().collect()
}

pub struct GovernanceAnalyzerPass {
    pass_id: String,
    artifact_ids: Vec<ArtifactId>,
}

impl GovernanceAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, artifact_ids: Vec<ArtifactId>) -> Self {
        Self {
            pass_id: format!("governance-analyzer:{}", workspace_name.into()),
            artifact_ids,
        }
    }
}

#[async_trait]
impl CompilerPass for GovernanceAnalyzerPass {
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
                        .warning("GOV001", format!("cannot read artifact {artifact_id}: {e}"));
                    continue;
                }
            };
            let d: ProposalData = match serde_json::from_value(json["data"].clone()) {
                Ok(d) => d,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "GOV002",
                        format!("malformed governance payload in {artifact_id}: {e}"),
                    );
                    continue;
                }
            };
            let mut obj = KirObject::new(
                format!("{}: {}", d.space, d.title),
                ObjectKind::Custom("GovernanceProposal".into()),
            );
            obj.id = proposal_kir_id(&d.space, &d.proposal_id);
            let (amount, token) = extract_amount(&d.body).unzip();
            let approved_at = (d.outcome == "approved").then(|| d.end.clone());
            for (k, v) in [
                ("proposal_id", serde_json::json!(d.proposal_id)),
                ("space", serde_json::json!(d.space)),
                ("title", serde_json::json!(d.title)),
                ("status", serde_json::json!(d.state)),
                ("author", serde_json::json!(d.author)),
                ("outcome", serde_json::json!(d.outcome)),
                ("approved_at", serde_json::json!(approved_at)),
                (
                    "approved_recipient",
                    serde_json::json!(extract_recipient(&d.body)),
                ),
                ("approved_amount", serde_json::json!(amount)),
                ("approved_token", serde_json::json!(token)),
                (
                    "tx_hashes_mentioned",
                    serde_json::json!(extract_tx_hashes(&d.body)),
                ),
                (
                    "body_excerpt",
                    serde_json::json!(
                        d.body
                            .chars()
                            .take(BODY_EXCERPT_MAX_CHARS)
                            .collect::<String>()
                    ),
                ),
            ] {
                obj.properties.insert(k.into(), v);
            }
            let ev = KirEvidence::new(
                SourceLocation::file(format!("snapshot:{}:{}", d.space, d.proposal_id)),
                format!(
                    "proposal {} \"{}\" state={} outcome={} closed {}",
                    d.proposal_id, d.title, d.state, d.outcome, d.end
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
        tracing::info!(pass = %self.pass_id, proposals = knowledge.content.kir.objects.len(), "governance-analyzer complete");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_compiler_core::EkosConfig;
    use std::sync::Arc;

    const R1: &str = "0x1111111111111111111111111111111111111111";
    const R2: &str = "0x2222222222222222222222222222222222222222";

    #[test]
    fn extracts_a_single_recipient_and_refuses_to_pick_between_two() {
        assert_eq!(
            extract_recipient(&format!("send to {R1} please")).as_deref(),
            Some(R1)
        );
        assert_eq!(
            extract_recipient(&format!("from {R1} to {R2}")),
            None,
            "ambiguous → unset"
        );
        assert_eq!(extract_recipient("no address here"), None);
        assert_eq!(
            extract_recipient(&format!("{R1} and again {R1}")).as_deref(),
            Some(R1),
            "same address twice is still one"
        );
    }

    #[test]
    fn extracts_a_single_stated_amount_with_scaling() {
        assert_eq!(
            extract_amount("Pay 50,000 USDC to the team"),
            Some(("50000".into(), "USDC".into()))
        );
        assert_eq!(
            extract_amount("a grant of 1.5M dai"),
            Some(("1500000".into(), "DAI".into()))
        );
        assert_eq!(
            extract_amount("20k OP over the quarter"),
            Some(("20000".into(), "OP".into()))
        );
        assert_eq!(
            extract_amount("about $10k per month"),
            None,
            "no token → not a stated amount"
        );
        assert_eq!(
            extract_amount("50,000 USDC now and 10,000 USDC later"),
            None,
            "two amounts → unset"
        );
        assert_eq!(
            extract_amount("50,000 USDC total, i.e. 50000 USDC"),
            Some(("50000".into(), "USDC".into())),
            "same amount restated is fine"
        );
    }

    #[test]
    fn extracts_cited_transaction_hashes_lowercased() {
        let h = format!("0x{}", "AB".repeat(32));
        let got = extract_tx_hashes(&format!("executed in {h} and {h}"));
        assert_eq!(got, vec![h.to_ascii_lowercase()]);
        assert!(extract_tx_hashes(&format!("just an address {R1}")).is_empty());
    }

    async fn run_one(body: &str, outcome: &str) -> KirObject {
        let dir = tempfile::tempdir().unwrap();
        let mut c = PassContext::new(Arc::new(EkosConfig::default()), dir.path().to_path_buf());
        let data = serde_json::json!({
            "platform":"snapshot","space":"dao.eth","proposal_id":"0xprop","title":"Fund marketing",
            "body": body, "state":"closed","author":"0xa","outcome": outcome,
            "created":"2026-02-20T00:00:00+00:00","end":"2026-03-01T00:00:00+00:00",
        });
        let a =
            ekos_artifact::ObservationArtifact::new("governance", "snapshot:dao.eth:0xprop", data);
        c.artifact_store
            .write(&a.id, &serde_json::to_value(&a).unwrap())
            .unwrap();
        GovernanceAnalyzerPass::new("test", vec![a.id])
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
            .unwrap();
        let k: ekos_artifact::KnowledgeArtifact =
            serde_json::from_value(c.artifact_store.read(&id).unwrap().unwrap()).unwrap();
        k.content.kir.objects.into_iter().next().unwrap()
    }

    #[tokio::test]
    async fn an_approved_proposal_carries_its_approval_time_and_extracted_terms() {
        let o = run_one(&format!("Pay 50,000 USDC to {R1}"), "approved").await;
        assert!(matches!(&o.kind, ObjectKind::Custom(k) if k == "GovernanceProposal"));
        assert_eq!(o.properties["approved_at"], "2026-03-01T00:00:00+00:00");
        assert_eq!(o.properties["approved_amount"], "50000");
        assert_eq!(o.properties["approved_token"], "USDC");
        assert_eq!(o.properties["approved_recipient"], R1);
        assert_eq!(o.evidence.len(), 1);
    }

    #[tokio::test]
    async fn only_an_approved_outcome_gets_an_approval_time() {
        for outcome in ["rejected", "unknown"] {
            let o = run_one("prose only, no numbers", outcome).await;
            assert!(
                o.properties["approved_at"].is_null(),
                "{outcome} must not carry approved_at"
            );
            assert!(o.properties["approved_amount"].is_null());
            assert!(o.properties["approved_recipient"].is_null());
        }
    }

    #[test]
    fn proposal_ids_are_stable_and_case_insensitive() {
        assert_eq!(
            proposal_kir_id("dao.eth", "0xABC"),
            proposal_kir_id("dao.eth", "0xabc")
        );
        assert_ne!(
            proposal_kir_id("a.eth", "0xabc"),
            proposal_kir_id("b.eth", "0xabc")
        );
    }
}
