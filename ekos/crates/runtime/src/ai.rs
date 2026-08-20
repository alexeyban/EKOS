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
    pub fn new(
        runtime: &'a Runtime<'a>,
        llm: Arc<dyn LlmProvider>,
        config: AiRuntimeConfig,
    ) -> Self {
        Self {
            runtime,
            llm,
            config,
        }
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

        Ok(AiAnswer {
            answer,
            evidence_refs,
            diagnostics,
        })
    }

    /// Retrieve top-ranked object matches, expand each into its neighbourhood,
    /// and reconstruct full state (object + relationships + evidence) for
    /// every object gathered. Deduplicated by object id.
    fn gather_context(&self, question: &str) -> Result<Vec<ObjectState>, AiError> {
        let matches = self.search_for_question(question)?;
        let top: Vec<KirId> = matches
            .into_iter()
            .take(self.config.max_matches as usize)
            .map(|(id, _name)| id)
            .collect();

        let mut ids: Vec<KirId> = Vec::new();
        let mut seen: HashSet<KirId> = HashSet::new();
        for id in &top {
            let graph = self
                .runtime
                .load_neighborhood(id, self.config.neighborhood_depth)?;
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

    /// Turns a natural-language `question` into a search that `Runtime::find_objects` (backed by
    /// SQLite FTS5) can actually match (RFC 0061), instead of passing the raw sentence straight
    /// through as `gather_context` used to.
    ///
    /// `Ledger::find_objects` (`crates/ledger/src/lib.rs`) treats *any* character outside
    /// `[alphanumeric, space, *]` — including ordinary sentence punctuation like `?`, `,`, `'` —
    /// as a signal to escape the *entire* query into one literal FTS5 phrase. A phrase query
    /// requires that exact text to appear contiguously in the indexed content, which a natural
    /// question never does, so every question containing punctuation silently retrieved zero
    /// context — confirmed live against a real compiled ledger (`analytics/`, devlog_60): "Who is
    /// Niklas Hambüchen and what did they contribute?" retrieved nothing, while the bare name
    /// "Niklas Hambüchen" (already alphanumeric-only, so never hit the phrase-escape path)
    /// correctly retrieved the real `Person` object — the same object, the same ledger, only the
    /// phrasing differed. MCP's own `ekos_search` tool description already tells callers to use
    /// "2-3 keywords, not natural-language questions"; `ask` is the one caller that's supposed to
    /// accept natural language and translate it, so the translation belongs here, not in
    /// `find_objects` itself (which other callers rely on for its literal-phrase-escaping
    /// behavior on deliberately-typed queries).
    ///
    /// Strategy: strip stopwords and punctuation to a keyword set, try an FTS5 **AND** query
    /// (every keyword must appear) first for precision, fall back to an **OR** query (any
    /// keyword) for recall if AND finds nothing, and fall back to the original raw question as a
    /// last resort so no previously-working query (e.g. one that was already just a bare name or
    /// a handful of keywords) can regress.
    fn search_for_question(&self, question: &str) -> Result<Vec<(KirId, String)>, AiError> {
        let terms = extract_search_terms(question);
        if !terms.is_empty() {
            let and_query = terms.join(" ");
            let hits = self.runtime.find_objects(&and_query)?;
            if !hits.is_empty() {
                return Ok(hits);
            }
            if terms.len() > 1 {
                let or_query = terms.join(" OR ");
                let hits = self.runtime.find_objects(&or_query)?;
                if !hits.is_empty() {
                    return Ok(hits);
                }
            }
        }
        Ok(self.runtime.find_objects(question)?)
    }
}

/// Common English function words carrying no search-discriminating value on their own — dropped
/// before building a keyword search from a natural-language question. Deliberately conservative
/// (short, closed-class words only) so a real content word is never mistaken for a stopword.
const QUESTION_STOPWORDS: &[&str] = &[
    "a", "an", "the", "is", "are", "was", "were", "am", "be", "been", "being", "do", "does", "did",
    "doing", "what", "who", "whom", "whose", "which", "how", "why", "where", "when", "and", "or",
    "but", "to", "of", "in", "on", "for", "with", "at", "by", "from", "about", "into", "than",
    "then", "this", "that", "these", "those", "it", "its", "their", "they", "them", "he", "she",
    "we", "you", "i", "my", "your", "our", "can", "could", "would", "should", "will", "shall",
    "have", "has", "had", "not", "no", "if", "as", "there", "here", "any", "some", "did",
];

/// Lowercased, punctuation-stripped, stopword-filtered keywords from a natural-language question
/// — the same "significant word" filtering step MCP's own `ekos_search` tool description asks
/// callers to do by hand ("Use 2-3 keywords, not natural-language questions"). Splits on `_` too
/// (not just non-alphanumeric punctuation) so `imported_browsers` becomes the two keywords
/// `imported`/`browsers` — matching FTS5's own default `unicode61` tokenizer, which already
/// treats `_` as a token separator, not part of a token. This keeps every extracted term (and the
/// query built from them) free of any character `Ledger::find_objects`'s `is_simple_term` check
/// would otherwise treat as needing literal-phrase escaping.
fn extract_search_terms(question: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    question
        .split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() >= 2 && !QUESTION_STOPWORDS.contains(&w.as_str()))
        .filter(|w| seen.insert(w.clone()))
        .collect()
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
    use ekos_kir::{
        KirEvidence, KirObject, KirRelationship, ObjectKind, RelationshipKind, SourceLocation,
    };
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
            .append_relationship(&KirRelationship::new(
                RelationshipKind::ForeignKey,
                orders_id,
                customers.id,
            ))
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

    // ── RFC 0061: natural-language question retrieval ───────────────────────────────────

    #[test]
    fn extract_search_terms_strips_stopwords_and_punctuation() {
        let terms = extract_search_terms(
            "Who is Niklas Hambüchen and what did they contribute to this repository?",
        );
        assert_eq!(
            terms,
            vec!["niklas", "hambüchen", "contribute", "repository"]
        );
    }

    #[test]
    fn extract_search_terms_splits_on_underscore_like_fts5_does() {
        let terms = extract_search_terms("What columns does imported_browsers have?");
        assert_eq!(terms, vec!["columns", "imported", "browsers"]);
    }

    #[test]
    fn extract_search_terms_dedupes_preserving_first_occurrence() {
        let terms = extract_search_terms("the orders table and the orders schema");
        assert_eq!(terms, vec!["orders", "table", "schema"]);
    }

    #[test]
    fn extract_search_terms_on_bare_keywords_is_unchanged() {
        // A caller that already passes 2-3 keywords (no stopwords, no punctuation) — the
        // pre-RFC-0060 working case — must keep working identically.
        assert_eq!(
            extract_search_terms("Niklas Hambüchen"),
            vec!["niklas", "hambüchen"]
        );
    }

    #[tokio::test]
    async fn ask_finds_context_from_a_full_sentence_question() {
        // Real bug (devlog_60): "orders" alone found the object; a full sentence containing
        // "orders" plus stopwords and a trailing "?" found nothing, because the whole sentence
        // was escaped into one unmatchable literal FTS5 phrase. Must now work identically.
        let (ledger, _dir) = temp_ledger();
        seed(&ledger);
        let runtime = Runtime::new(&ledger);

        let llm = Arc::new(MockLlmProvider::new(
            r#"Orders depends on customers. {"cited_evidence": []}"#,
        ));
        let ai = AiRuntime::new(&runtime, llm, AiRuntimeConfig::default());
        let answer = ai
            .ask("What does the orders table depend on?")
            .await
            .unwrap();

        assert!(
            answer.answer.contains("Orders depends on customers"),
            "expected real context to be retrieved from a full-sentence question, got: {:?}",
            answer.answer
        );
    }

    #[tokio::test]
    async fn ask_finds_context_from_an_underscore_named_object_via_sentence() {
        let (ledger, _dir) = temp_ledger();
        let ev = KirEvidence::new(
            SourceLocation::file("structure.sql"),
            "CREATE TABLE imported_browsers",
        );
        ledger.append_evidence(&ev).unwrap();
        let mut table = KirObject::new("imported_browsers", ObjectKind::Table);
        table.evidence.push(ev.id);
        ledger.append_object(&table).unwrap();
        let runtime = Runtime::new(&ledger);

        let llm = Arc::new(MockLlmProvider::new(
            r#"It has a browser column. {"cited_evidence": []}"#,
        ));
        let ai = AiRuntime::new(&runtime, llm, AiRuntimeConfig::default());
        let answer = ai
            .ask("What columns does imported_browsers have?")
            .await
            .unwrap();

        assert!(
            answer.answer.contains("browser column"),
            "expected the underscore-named table to be retrieved from a full-sentence question, got: {:?}",
            answer.answer
        );
    }
}
