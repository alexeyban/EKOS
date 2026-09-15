//! Runs `mode: retrieval` scenarios (RFC 0138) — a bare `Runtime::retrieve` call, no `AiRuntime`,
//! no LLM, no token/answer fields populated. Existing purely so recall@k can be graded on
//! scenarios that don't need (or shouldn't pay for) a real LLM call.

use super::ScenarioRun;
use crate::resource::{self, ResourceDelta};
use crate::schema::Scenario;
use ekos_runtime::{RetrievalRequest, Runtime};
use std::time::Instant;

/// Corpus-generic nouns that dilute BM25 ranking without discriminating anything (RFC
/// 0139-followup "B2" fix). Unlike `ekos_runtime`'s `QUESTION_STOPWORDS` (closed-class function
/// words stripped from every natural-language question), these are real content words that are
/// simply too frequent in *this* corpus to help a short retrieval-mode query — verified against
/// the live ledger before being added: `sql_analyzer::SqlAnalyzerPass` ranks #1 for "sql_analyzer"
/// alone but falls out of the top 50 once "pass" joins the query (nearly every compiler pass in
/// the crate is named `..Pass` and mentions "pass"), and the same dilution shape holds for
/// "function" (`ekos_common::redaction::redact` #2 for "redact" alone, absent from the top 14 for
/// "redact function") and "store" (`ekos-artifact` #1 for "artifact" alone, absent from the top 14
/// for "artifact store"). Deliberately short and scoped to this runner rather than folded into
/// the shared `QUESTION_STOPWORDS`: `mode: retrieval` scenarios are short, hand-picked queries, not
/// natural questions, and this fix must not touch how `understand()`/`search_query()` build a
/// real REASON query.
const RETRIEVAL_GENERIC_TERMS: &[&str] = &["function", "pass", "store"];

/// Strips [`RETRIEVAL_GENERIC_TERMS`] (whole words, case-insensitive) from `question`, joining
/// what remains with single spaces. Falls back to the original text when stripping would remove
/// every word, so a query made entirely of generic terms still searches for something rather than
/// becoming an empty, always-empty-result query.
fn strip_generic_terms(question: &str) -> String {
    let stripped: Vec<&str> = question
        .split_whitespace()
        .filter(|w| {
            !RETRIEVAL_GENERIC_TERMS
                .iter()
                .any(|g| w.eq_ignore_ascii_case(g))
        })
        .collect();
    if stripped.is_empty() {
        question.to_string()
    } else {
        stripped.join(" ")
    }
}

pub fn run(runtime: &Runtime<'_>, scenario: &Scenario) -> ScenarioRun {
    let resource_before = resource::sample();
    let start = Instant::now();
    let query = strip_generic_terms(&scenario.question);
    let result = runtime.retrieve(&RetrievalRequest::lexical(&query));
    let latency = start.elapsed();
    let resource_delta = ResourceDelta::between(resource_before, resource::sample());

    match result {
        Ok(results) => ScenarioRun {
            retrieved_ids: results.hits.into_iter().map(|h| h.id).collect(),
            latency,
            resource: resource_delta,
            ..Default::default()
        },
        Err(e) => ScenarioRun {
            latency,
            resource: resource_delta,
            error: Some(e.to_string()),
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_a_generic_term_from_a_two_word_query() {
        assert_eq!(strip_generic_terms("redact function"), "redact");
        assert_eq!(strip_generic_terms("sql_analyzer pass"), "sql_analyzer");
        assert_eq!(strip_generic_terms("artifact store"), "artifact");
    }

    #[test]
    fn is_case_insensitive() {
        assert_eq!(strip_generic_terms("Redact Function"), "Redact");
    }

    #[test]
    fn leaves_a_query_with_no_generic_terms_unchanged() {
        assert_eq!(strip_generic_terms("recovery crate"), "recovery crate");
        assert_eq!(strip_generic_terms("redaction"), "redaction");
    }

    #[test]
    fn falls_back_to_the_original_text_when_stripping_would_remove_everything() {
        assert_eq!(strip_generic_terms("function"), "function");
        assert_eq!(strip_generic_terms("pass store"), "pass store");
    }
}
