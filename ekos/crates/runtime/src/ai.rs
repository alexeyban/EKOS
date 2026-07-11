//! AI Runtime — natural-language questions answered from grounded, evidenced
//! knowledge (RFC 0009).
//!
//! `AiRuntime` sits on top of [`crate::Runtime`] and an `LlmProvider`. It never
//! touches the ledger or enterprise systems directly — only through the
//! Runtime, upholding the same read-only consumer-facing contract as RFC 0005.

use crate::{ObjectState, Runtime, RuntimeError};
use ekos_compiler_core::Diagnostic;
use ekos_kir::KirId;
use ekos_recovery::llm::{LlmError, LlmProvider, LlmRequest};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use thiserror::Error;

const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
const DEFAULT_SYSTEM_PROMPT: &str = r#"You are the EKOS Knowledge Runtime assistant. Answer only using the JSON context provided.
Every claim must be traceable to the supplied evidence. End your response with a JSON block:
{"cited_evidence": ["<id>", ...]}
If you cannot answer from the given context, say so explicitly."#;
const PROMPT_VERSION: &str = "ai-runtime-ask-v1";

#[derive(Debug, Error)]
pub enum AiError {
    #[error("runtime error: {0}")]
    Runtime(#[from] RuntimeError),
    #[error("llm error: {0}")]
    Llm(#[from] LlmError),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Tunables for the retrieve → expand → ground → ask pipeline. Backed by the
/// `[ai]` section of `ekos.toml`; every field falls back to a sensible default
/// when unset.
#[derive(Debug, Clone)]
pub struct AiRuntimeConfig {
    pub model: String,
    pub max_matches: u32,
    pub neighborhood_depth: u32,
    pub max_tokens: u32,
    pub system_prompt: String,
}

impl Default for AiRuntimeConfig {
    fn default() -> Self {
        Self {
            model: DEFAULT_MODEL.to_string(),
            max_matches: 3,
            neighborhood_depth: 1,
            max_tokens: 1024,
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
        }
    }
}

/// The result of `AiRuntime::ask`: a grounded answer plus every evidence id it
/// cites. `diagnostics` carries non-fatal issues (e.g. a missing citation
/// block) — the answer is still returned even when it's non-empty.
#[derive(Debug, Clone, Serialize)]
pub struct AiAnswer {
    pub answer: String,
    pub evidence_refs: Vec<KirId>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Deserialize)]
struct CitationBlock {
    cited_evidence: Vec<String>,
}

/// Answers natural-language questions grounded in the Knowledge Ledger.
///
/// Pipeline: retrieve candidate objects via `Runtime::find_objects`, expand
/// each into its neighbourhood via `Runtime::load_neighborhood`, ground the
/// prompt with `Runtime::reconstruct_state` (object + relationships +
/// evidence as JSON), then ask the LLM and parse a trailing citation block.
pub struct AiRuntime<'a> {
    runtime: &'a Runtime<'a>,
    llm: Arc<dyn LlmProvider>,
    config: AiRuntimeConfig,
}

impl<'a> AiRuntime<'a> {
    pub fn new(runtime: &'a Runtime<'a>, llm: Arc<dyn LlmProvider>, config: AiRuntimeConfig) -> Self {
        Self { runtime, llm, config }
    }

    pub async fn ask(&self, question: &str) -> Result<AiAnswer, AiError> {
        let contexts = self.gather_context(question)?;

        let known_evidence: HashSet<KirId> = contexts
            .iter()
            .flat_map(|s| s.evidence.iter().map(|e| e.id))
            .collect();

        let context_json = serde_json::to_string_pretty(&contexts)?;
        let user = format!("Question: {question}\n\nContext:\n{context_json}");

        let req = LlmRequest {
            system: &self.config.system_prompt,
            user: &user,
            prompt_version: PROMPT_VERSION,
            max_tokens: self.config.max_tokens,
        };
        let resp = self.llm.complete(&req).await?;

        let (answer, evidence_refs, diagnostics) =
            extract_citations(&resp.content, &known_evidence);

        Ok(AiAnswer { answer, evidence_refs, diagnostics })
    }

    /// Retrieve top-ranked object matches, expand each into its neighbourhood,
    /// and reconstruct full state (object + relationships + evidence) for
    /// every object gathered. Deduplicated by object id.
    fn gather_context(&self, question: &str) -> Result<Vec<ObjectState>, AiError> {
        let matches = self.runtime.find_objects(question)?;
        let top: Vec<KirId> = matches
            .into_iter()
            .take(self.config.max_matches as usize)
            .map(|(id, _name)| id)
            .collect();

        let mut ids: Vec<KirId> = Vec::new();
        let mut seen: HashSet<KirId> = HashSet::new();
        for id in &top {
            let graph = self.runtime.load_neighborhood(id, self.config.neighborhood_depth)?;
            for obj in graph.objects {
                if seen.insert(obj.id) {
                    ids.push(obj.id);
                }
            }
        }

        let mut contexts = Vec::new();
        for id in &ids {
            if let Some(state) = self.runtime.reconstruct_state(id)? {
                contexts.push(state);
            }
        }
        Ok(contexts)
    }
}

/// Parses a trailing `{"cited_evidence": [...]}` block from an LLM response.
/// Unknown or malformed ids are silently dropped; a missing/unparsable block
/// yields the whole response as the answer with an empty citation list and a
/// warning diagnostic — the answer is never discarded.
fn extract_citations(
    content: &str,
    known_evidence: &HashSet<KirId>,
) -> (String, Vec<KirId>, Vec<Diagnostic>) {
    if let Some(idx) = content.rfind('{') {
        let (answer_part, json_part) = content.split_at(idx);
        let json_part = json_part.trim().trim_end_matches("```").trim();
        if let Ok(block) = serde_json::from_str::<CitationBlock>(json_part) {
            let evidence_refs: Vec<KirId> = block
                .cited_evidence
                .iter()
                .filter_map(|s| s.parse::<KirId>().ok())
                .filter(|id| known_evidence.contains(id))
                .collect();
            return (answer_part.trim().to_string(), evidence_refs, Vec::new());
        }
    }

    let warning = Diagnostic::warning(
        "AI001",
        "LLM response did not include a valid cited_evidence block",
    );
    (content.trim().to_string(), Vec::new(), vec![warning])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirEvidence, KirObject, KirRelationship, ObjectKind, RelationshipKind, SourceLocation};
    use ekos_ledger::Ledger;
    use ekos_recovery::MockLlmProvider;
    use tempfile::TempDir;

    fn temp_ledger() -> (Ledger, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ledger.db");
        (Ledger::open(&path).unwrap(), dir)
    }

    fn seed(ledger: &Ledger) -> (KirId, KirId) {
        let ev = KirEvidence::new(SourceLocation::file("schema.sql"), "CREATE TABLE orders");
        let ev_id = ev.id;
        ledger.append_evidence(&ev).unwrap();

        let mut orders = KirObject::new("orders", ObjectKind::Table);
        orders.evidence.push(ev_id);
        let orders_id = orders.id;
        let customers = KirObject::new("customers", ObjectKind::Table);
        ledger.append_object(&orders).unwrap();
        ledger.append_object(&customers).unwrap();
        ledger
            .append_relationship(&KirRelationship::new(RelationshipKind::ForeignKey, orders_id, customers.id))
            .unwrap();

        (orders_id, ev_id)
    }

    #[tokio::test]
    async fn ask_sends_object_context_in_prompt() {
        let (ledger, _dir) = temp_ledger();
        let (_orders_id, _ev_id) = seed(&ledger);
        let runtime = Runtime::new(&ledger);

        let llm = Arc::new(MockLlmProvider::new(
            r#"Orders depends on customers. {"cited_evidence": []}"#,
        ));
        let ai = AiRuntime::new(&runtime, llm.clone(), AiRuntimeConfig::default());
        let answer = ai.ask("orders").await.unwrap();

        assert!(answer.answer.contains("Orders depends on customers"));
        assert!(answer.diagnostics.is_empty());
    }

    #[tokio::test]
    async fn ask_parses_valid_citation_block() {
        let (ledger, _dir) = temp_ledger();
        let (_orders_id, ev_id) = seed(&ledger);
        let runtime = Runtime::new(&ledger);

        let llm = Arc::new(MockLlmProvider::new(format!(
            r#"Orders references customers via a foreign key. {{"cited_evidence": ["{ev_id}"]}}"#
        )));
        let ai = AiRuntime::new(&runtime, llm, AiRuntimeConfig::default());
        let answer = ai.ask("orders").await.unwrap();

        assert_eq!(answer.evidence_refs, vec![ev_id]);
        assert!(answer.diagnostics.is_empty());
    }

    #[tokio::test]
    async fn ask_without_citation_block_emits_warning_but_keeps_answer() {
        let (ledger, _dir) = temp_ledger();
        seed(&ledger);
        let runtime = Runtime::new(&ledger);

        let llm = Arc::new(MockLlmProvider::new("Orders depends on customers."));
        let ai = AiRuntime::new(&runtime, llm, AiRuntimeConfig::default());
        let answer = ai.ask("orders").await.unwrap();

        assert_eq!(answer.answer, "Orders depends on customers.");
        assert!(answer.evidence_refs.is_empty());
        assert_eq!(answer.diagnostics.len(), 1);
    }

    #[tokio::test]
    async fn ask_drops_unknown_cited_ids() {
        let (ledger, _dir) = temp_ledger();
        seed(&ledger);
        let runtime = Runtime::new(&ledger);

        let bogus_id = KirId::new();
        let llm = Arc::new(MockLlmProvider::new(format!(
            r#"Answer. {{"cited_evidence": ["{bogus_id}"]}}"#
        )));
        let ai = AiRuntime::new(&runtime, llm, AiRuntimeConfig::default());
        let answer = ai.ask("orders").await.unwrap();

        assert!(answer.evidence_refs.is_empty());
    }
}
