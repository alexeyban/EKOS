//! AI Runtime — natural-language questions answered from grounded, evidenced
//! knowledge (RFC 0009).
//!
//! `AiRuntime` sits on top of [`crate::Runtime`] and an `LlmProvider`. It never
//! touches the ledger or enterprise systems directly — only through the
//! Runtime, upholding the same read-only consumer-facing contract as RFC 0005.

use crate::{ObjectState, RetrievalRequest, Runtime, RuntimeError};
use ekos_compiler_core::Diagnostic;
use ekos_kir::KirId;
use ekos_recovery::llm::{LlmError, LlmProvider, LlmRequest, Message};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use thiserror::Error;

const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
/// Default cap on the total serialized size of gathered `ObjectState` context, in characters.
/// ~200k chars (~50k tokens at a conservative ~4 chars/token) — comfortably under the rate/context
/// limits that broad, hub-like search terms were observed to blow through in practice (RFC 0046,
/// devlog_46): a single real request against EKOS-self's ~7,500-object ledger asked for 209,852
/// tokens against a 200,000 TPM limit, with no budget check anywhere upstream to stop it.
const DEFAULT_MAX_CONTEXT_CHARS: u32 = 200_000;
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
    /// Cap on the total serialized size (characters) of gathered `ObjectState` context sent to
    /// the LLM. `max_matches`/`neighborhood_depth` bound seed count and hop depth, but not what a
    /// single hop pulls in — a hub-like object with hundreds of neighbors could still blow past
    /// any provider's context/rate limit. See [`DEFAULT_MAX_CONTEXT_CHARS`].
    pub max_context_chars: u32,
}

impl Default for AiRuntimeConfig {
    fn default() -> Self {
        Self {
            model: DEFAULT_MODEL.to_string(),
            max_matches: 3,
            neighborhood_depth: 1,
            max_tokens: 1024,
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
            max_context_chars: DEFAULT_MAX_CONTEXT_CHARS,
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

/// One prior turn in a multi-turn `ekos ask --session` conversation (RFC
/// 0099) — the clean question and citation-stripped answer, never the raw
/// grounded prompt (`"Question: ...\n\nContext:\n...json..."`) or the raw
/// LLM response (which still carries the trailing `{"cited_evidence":
/// [...]}` block) a turn was actually produced from. Keeping history clean
/// like this means a long session's prior turns don't re-inflate every
/// later prompt with retrieved-context JSON nobody needs repeated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub question: String,
    pub answer: String,
}

/// Expands `history` into the `user`/`assistant` message pairs
/// `LlmRequest::history` expects, oldest first.
fn history_messages(history: &[ConversationTurn]) -> Vec<Message<'_>> {
    let mut messages = Vec::with_capacity(history.len() * 2);
    for turn in history {
        messages.push(Message {
            role: "user",
            content: &turn.question,
        });
        messages.push(Message {
            role: "assistant",
            content: &turn.answer,
        });
    }
    messages
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
        self.ask_with_history(question, &[]).await
    }

    /// Same as [`Self::ask`], but with `history` (RFC 0099) — prior clean
    /// question/answer pairs, oldest first — inserted between the system
    /// prompt and this turn's own grounded user message. Retrieval
    /// (`gather_context`) is deliberately **not** history-aware in v1: each
    /// turn's search still runs off that turn's own `question` text alone,
    /// not the whole conversation — the simplest correct behavior, and the
    /// one documented, named limitation of this RFC (see RFC 0099's own
    /// Non-goals) rather than a second open research question folded in
    /// silently.
    pub async fn ask_with_history(
        &self,
        question: &str,
        history: &[ConversationTurn],
    ) -> Result<AiAnswer, AiError> {
        let (contexts, mut diagnostics) = self.gather_context(question)?;

        let known_evidence: HashSet<KirId> = contexts
            .iter()
            .flat_map(|s| s.evidence.iter().map(|e| e.id))
            .collect();

        let context_json = serde_json::to_string_pretty(&contexts)?;
        let user = format!("Question: {question}\n\nContext:\n{context_json}");
        let history_messages = history_messages(history);

        let req = LlmRequest {
            system: &self.config.system_prompt,
            user: &user,
            prompt_version: PROMPT_VERSION,
            max_tokens: self.config.max_tokens,
            history: &history_messages,
        };
        let resp = self.llm.complete(&req).await?;

        let (answer, evidence_refs, citation_diagnostics) =
            extract_citations(&resp.content, &known_evidence);
        diagnostics.extend(citation_diagnostics);

        Ok(AiAnswer {
            answer,
            evidence_refs,
            diagnostics,
        })
    }

    /// Same pipeline as [`Self::ask`], but calls `on_chunk` with each piece
    /// of the LLM's answer as it becomes available (RFC 0098) — retrieval
    /// (`gather_context`) is unchanged and still runs synchronously up
    /// front, only the completion call itself streams. Citation extraction
    /// (`extract_citations`) still needs the *full* response text (it looks
    /// for the trailing `{"cited_evidence": [...]}` block via `rfind('{')`,
    /// which can't be resolved mid-stream), so `AiAnswer.answer`/
    /// `evidence_refs`/`diagnostics` are only available once the stream
    /// ends — `on_chunk` is the only way to see the answer progressively.
    pub async fn ask_stream(
        &self,
        question: &str,
        on_chunk: &mut (dyn FnMut(String) + Send),
    ) -> Result<AiAnswer, AiError> {
        self.ask_stream_with_history(question, &[], on_chunk).await
    }

    /// [`Self::ask_stream`] with `history` (RFC 0099) — see
    /// [`Self::ask_with_history`] for the history-handling contract; the
    /// only difference from that method is streaming the completion call.
    pub async fn ask_stream_with_history(
        &self,
        question: &str,
        history: &[ConversationTurn],
        on_chunk: &mut (dyn FnMut(String) + Send),
    ) -> Result<AiAnswer, AiError> {
        let (contexts, mut diagnostics) = self.gather_context(question)?;

        let known_evidence: HashSet<KirId> = contexts
            .iter()
            .flat_map(|s| s.evidence.iter().map(|e| e.id))
            .collect();

        let context_json = serde_json::to_string_pretty(&contexts)?;
        let user = format!("Question: {question}\n\nContext:\n{context_json}");
        let history_messages = history_messages(history);

        let req = LlmRequest {
            system: &self.config.system_prompt,
            user: &user,
            prompt_version: PROMPT_VERSION,
            max_tokens: self.config.max_tokens,
            history: &history_messages,
        };
        let resp = self.llm.complete_stream(&req, on_chunk).await?;

        let (answer, evidence_refs, citation_diagnostics) =
            extract_citations(&resp.content, &known_evidence);
        diagnostics.extend(citation_diagnostics);

        Ok(AiAnswer {
            answer,
            evidence_refs,
            diagnostics,
        })
    }

    /// Retrieve top-ranked object matches, expand each into its neighbourhood, and reconstruct
    /// full state (object + relationships + evidence) for every object gathered, deduplicated by
    /// object id, stopping once `max_context_chars` worth of serialized state has been gathered.
    ///
    /// Without this cap, a hub-like seed object (one with hundreds of real neighbors) could pull
    /// its entire neighborhood into the prompt regardless of `max_matches`/`neighborhood_depth` —
    /// those bound seed count and hop *depth*, never what a single hop actually pulls in. Found
    /// live-testing RFC 0046 (devlog_46): broad/hub search terms against EKOS-self's ~7,500-object
    /// ledger produced real `context_length_exceeded`/`rate_limit_exceeded` provider errors, one
    /// request alone asking for 209,852 tokens against a 200,000 TPM limit. The first object is
    /// always admitted regardless of its own size, so a single oversized object can never make
    /// `ask` answer from zero context.
    fn gather_context(
        &self,
        question: &str,
    ) -> Result<(Vec<ObjectState>, Vec<Diagnostic>), AiError> {
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

        let budget = self.config.max_context_chars as usize;
        let mut contexts = Vec::new();
        let mut total_chars = 0usize;
        let mut omitted = 0usize;
        for id in &ids {
            let Some(state) = self.runtime.reconstruct_state(id)? else {
                continue;
            };
            let size = serde_json::to_string(&state)?.len();
            if !contexts.is_empty() && total_chars + size > budget {
                omitted += 1;
                continue;
            }
            total_chars += size;
            contexts.push(state);
        }

        let diagnostics = if omitted > 0 {
            vec![Diagnostic::warning(
                "AI003",
                format!(
                    "context truncated to stay under the {budget}-character budget — {omitted} \
                     neighborhood object(s) omitted, {} included (~{total_chars} chars)",
                    contexts.len()
                ),
            )]
        } else {
            Vec::new()
        };
        Ok((contexts, diagnostics))
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
        // RFC 0119: route each rung of the AND→OR→raw ladder through the retrieval seam. Phase 0
        // = BM25, byte-identical; RFC 0121 replaces the whole hand-rolled ladder with `understand`.
        let search = |q: &str| -> Result<Vec<(KirId, String)>, RuntimeError> {
            Ok(self
                .runtime
                .retrieve(&RetrievalRequest::lexical(q))?
                .into_pairs())
        };
        let terms = extract_search_terms(question);
        if !terms.is_empty() {
            let hits = search(&terms.join(" "))?;
            if !hits.is_empty() {
                return Ok(hits);
            }
            if terms.len() > 1 {
                let hits = search(&terms.join(" OR "))?;
                if !hits.is_empty() {
                    return Ok(hits);
                }
            }
        }
        Ok(search(question)?)
    }
}

/// Common English function words carrying no search-discriminating value on their own — dropped
/// before building a keyword search from a natural-language question. Deliberately conservative
/// (short, closed-class words only) so a real content word is never mistaken for a stopword.
pub(crate) const QUESTION_STOPWORDS: &[&str] = &[
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
pub(crate) fn extract_search_terms(question: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    question
        .split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() >= 2 && !QUESTION_STOPWORDS.contains(&w.as_str()))
        .filter(|w| seen.insert(w.clone()))
        .collect()
}

/// Parses a trailing `{"cited_evidence": [...]}` block from an LLM response.
/// Unknown or malformed ids are dropped; a missing/unparsable block yields the
/// whole response as the answer with an empty citation list and an `AI001`
/// warning diagnostic — the answer is never discarded.
///
/// A block that parses cleanly but whose citations don't survive filtering (an empty array, or
/// every id unknown/malformed) gets its own `AI002` diagnostic, distinct from `AI001` — found
/// live-testing RFC 0046 against real `gpt-4o-mini` responses (devlog_46): roughly half of
/// reasonable single-keyword questions returned a confident, correct-looking answer with a
/// successfully-parsed but empty `cited_evidence` array, which previously produced the exact same
/// empty-diagnostics shape as a genuinely well-cited answer — a caller (CLI/MCP/demo-server) had
/// no way to tell "this answer is ungrounded" from "this answer is well-grounded" without
/// separately checking `evidence_refs.is_empty()` itself.
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
            let diagnostics = if evidence_refs.is_empty() {
                vec![Diagnostic::warning(
                    "AI002",
                    "LLM response included a cited_evidence block, but no citations survived it \
                     (empty array, or none of the ids matched evidence actually supplied in \
                     context) — treat this answer as ungrounded even though it parsed cleanly",
                )]
            } else {
                Vec::new()
            };
            return (answer_part.trim().to_string(), evidence_refs, diagnostics);
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

    /// A "hub" object connected to `n` leaf objects — models the real broad/hub-term shape
    /// (devlog_46) that pulled hundreds of neighbors into a single-hop expansion with no size cap.
    fn seed_hub(ledger: &Ledger, n: usize) -> KirId {
        let hub = KirObject::new("hub", ObjectKind::Table);
        let hub_id = hub.id;
        ledger.append_object(&hub).unwrap();
        for i in 0..n {
            let leaf = KirObject::new(format!("hub leaf {i}"), ObjectKind::Table);
            ledger.append_object(&leaf).unwrap();
            ledger
                .append_relationship(&KirRelationship::new(
                    RelationshipKind::ForeignKey,
                    hub_id,
                    leaf.id,
                ))
                .unwrap();
        }
        hub_id
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
        // An empty `cited_evidence` array is a real, distinct AI002 diagnostic case (see
        // `extract_citations`) — the answer is kept, but it's flagged as ungrounded, not silently
        // treated the same as a genuinely well-cited answer.
        assert_eq!(answer.diagnostics.len(), 1);
        assert_eq!(answer.diagnostics[0].code, "AI002");
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

    // ── RFC 0098: ask_stream ─────────────────────────────────────────────

    #[tokio::test]
    async fn ask_stream_delivers_chunks_and_returns_the_same_grounded_answer_as_ask() {
        let (ledger, _dir) = temp_ledger();
        let (_orders_id, ev_id) = seed(&ledger);
        let runtime = Runtime::new(&ledger);

        let llm = Arc::new(MockLlmProvider::new(format!(
            r#"Orders references customers via a foreign key. {{"cited_evidence": ["{ev_id}"]}}"#
        )));
        let ai = AiRuntime::new(&runtime, llm, AiRuntimeConfig::default());

        let mut chunks = Vec::new();
        let mut on_chunk = |s: String| chunks.push(s);
        let answer = ai.ask_stream("orders", &mut on_chunk).await.unwrap();

        assert_eq!(answer.evidence_refs, vec![ev_id]);
        assert!(answer.diagnostics.is_empty());
        assert!(
            answer
                .answer
                .contains("Orders references customers via a foreign key")
        );
        // MockLlmProvider has no real incremental streaming — it uses
        // LlmProvider::complete_stream's default fallback (one chunk, the
        // whole response) — so this pins that ask_stream really does route
        // through complete_stream (not silently falling back to `ask`'s own
        // complete()) rather than proving true multi-chunk delivery, which
        // is covered live against a real provider instead (devlog_115).
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("cited_evidence"));
    }

    // ── RFC 0099: multi-turn history ─────────────────────────────────────

    /// Records the `history` it was called with (as owned strings, since
    /// `LlmRequest` borrows) instead of returning a fixed response —
    /// proves `ask_with_history` actually threads `ConversationTurn`s
    /// through to the provider, not just that it compiles.
    struct RecordingMock {
        seen_history: std::sync::Mutex<Vec<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl LlmProvider for RecordingMock {
        fn model_name(&self) -> &str {
            "recording-mock"
        }
        async fn complete(
            &self,
            req: &LlmRequest<'_>,
        ) -> Result<ekos_recovery::llm::LlmResponse, LlmError> {
            *self.seen_history.lock().unwrap() = req
                .history
                .iter()
                .map(|m| (m.role.to_string(), m.content.to_string()))
                .collect();
            Ok(ekos_recovery::llm::LlmResponse {
                content: r#"An answer. {"cited_evidence": []}"#.to_string(),
                model: "recording-mock".to_string(),
                input_tokens: 0,
                output_tokens: 0,
            })
        }
    }

    #[tokio::test]
    async fn ask_with_history_threads_prior_turns_into_the_llm_request() {
        let (ledger, _dir) = temp_ledger();
        seed(&ledger);
        let runtime = Runtime::new(&ledger);

        let mock = Arc::new(RecordingMock {
            seen_history: std::sync::Mutex::new(Vec::new()),
        });
        let ai = AiRuntime::new(&runtime, mock.clone(), AiRuntimeConfig::default());

        let history = [ConversationTurn {
            question: "what tables exist?".to_string(),
            answer: "orders and customers.".to_string(),
        }];
        ai.ask_with_history("orders", &history).await.unwrap();

        let seen = mock.seen_history.lock().unwrap();
        assert_eq!(
            *seen,
            vec![
                ("user".to_string(), "what tables exist?".to_string()),
                ("assistant".to_string(), "orders and customers.".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn ask_without_history_sends_an_empty_history() {
        let (ledger, _dir) = temp_ledger();
        seed(&ledger);
        let runtime = Runtime::new(&ledger);

        let mock = Arc::new(RecordingMock {
            seen_history: std::sync::Mutex::new(Vec::new()),
        });
        let ai = AiRuntime::new(&runtime, mock.clone(), AiRuntimeConfig::default());

        ai.ask("orders").await.unwrap();

        assert!(mock.seen_history.lock().unwrap().is_empty());
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
        // The block parsed cleanly, but nothing in it survived the known-evidence filter — same
        // AI002 "ungrounded despite a clean parse" case as an empty array.
        assert_eq!(answer.diagnostics.len(), 1);
        assert_eq!(answer.diagnostics[0].code, "AI002");
    }

    // ── Context size budget (devlog_46/devlog_64) ────────────────────────────────────────

    #[test]
    fn gather_context_admits_at_least_one_object_even_under_a_tiny_budget() {
        let (ledger, _dir) = temp_ledger();
        seed_hub(&ledger, 20);
        let runtime = Runtime::new(&ledger);
        let mut config = AiRuntimeConfig::default();
        config.max_matches = 1;
        config.neighborhood_depth = 1;
        config.max_context_chars = 1; // smaller than even one serialized object
        let ai = AiRuntime::new(&runtime, Arc::new(MockLlmProvider::new("x")), config);

        let (contexts, diagnostics) = ai.gather_context("hub").unwrap();

        assert!(!contexts.is_empty(), "first object must always be admitted");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "AI003");
    }

    #[test]
    fn gather_context_stays_under_budget_and_truncates_a_large_hub_neighborhood() {
        let (ledger, _dir) = temp_ledger();
        seed_hub(&ledger, 20);
        let runtime = Runtime::new(&ledger);
        let mut config = AiRuntimeConfig::default();
        config.max_matches = 1;
        config.neighborhood_depth = 1;
        config.max_context_chars = 500;
        let ai = AiRuntime::new(&runtime, Arc::new(MockLlmProvider::new("x")), config);

        let (contexts, diagnostics) = ai.gather_context("hub").unwrap();

        // The hub has 21 real neighborhood objects (itself + 20 leaves); a 500-char budget
        // must not admit all of them.
        assert!(
            contexts.len() < 21,
            "expected truncation, got {} objects",
            contexts.len()
        );
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("omitted"));
    }

    #[tokio::test]
    async fn ask_surfaces_context_truncation_diagnostic_alongside_citation_diagnostics() {
        let (ledger, _dir) = temp_ledger();
        seed_hub(&ledger, 20);
        let runtime = Runtime::new(&ledger);
        let mut config = AiRuntimeConfig::default();
        config.max_matches = 1;
        config.neighborhood_depth = 1;
        config.max_context_chars = 500;
        let llm = Arc::new(MockLlmProvider::new("An answer with no citation block."));
        let ai = AiRuntime::new(&runtime, llm, config);

        let answer = ai.ask("hub").await.unwrap();

        let codes: Vec<&str> = answer.diagnostics.iter().map(|d| d.code.as_str()).collect();
        assert!(codes.contains(&"AI003"), "expected AI003 in {codes:?}");
        assert!(codes.contains(&"AI001"), "expected AI001 in {codes:?}");
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
