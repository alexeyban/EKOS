//! Runs `mode: reason`/`ask` scenarios (RFC 0138) through an already-constructed `AiRuntime` —
//! opening the store and building the `LlmProvider` is the CLI command's job (`ask.rs`'s own
//! pattern), so this module never touches configuration or credentials.

use super::ScenarioRun;
use crate::resource::{self, ResourceDelta};
use crate::schema::{Mode, Scenario};
use ekos_runtime::retrieval::understand;
use ekos_runtime::{AiRuntime, RetrievalRequest, Runtime, search_query};
use std::time::Instant;

/// Run one scenario. `runtime` is the same store handle `ai` was built over — the CLI command
/// already holds both (mirrors `ask.rs`'s own `Runtime::over` + `AiRuntime::new` pair), so this
/// avoids adding a `runtime()` getter to `AiRuntime`'s public surface just for this crate.
///
/// `mode: retrieval` scenarios are rejected — route those through
/// [`super::retrieval_runner::run`] instead, since they need no `AiRuntime` at all.
pub async fn run(ai: &AiRuntime<'_>, runtime: &Runtime<'_>, scenario: &Scenario) -> ScenarioRun {
    debug_assert_ne!(
        scenario.mode,
        Mode::Retrieval,
        "agent_runner::run called on a retrieval-mode scenario"
    );

    let cache_before = ai.cache_stats();
    let resource_before = resource::sample();
    let start = Instant::now();
    let answer_result = match scenario.mode {
        Mode::Ask => ai.ask(&scenario.question).await,
        Mode::Reason | Mode::Retrieval => ai.reason(&scenario.question).await,
    };
    let latency = start.elapsed();
    let resource_delta = ResourceDelta::between(resource_before, resource::sample());
    // A miss-count that grew means this specific call actually hit the network; anything else
    // (hit-count grew, or the provider isn't cached at all) means it didn't spend fresh tokens.
    let cache_hit = match (cache_before, ai.cache_stats()) {
        (Some((_, misses_before)), Some((_, misses_after))) => Some(misses_after == misses_before),
        _ => None,
    };

    let mut run = match answer_result {
        Ok(answer) => ScenarioRun {
            answer: Some(answer.answer),
            evidence_refs: answer.evidence_refs,
            token_usage: Some(answer.token_usage),
            // RFC 0139 §1: keep the pipeline's own diagnostics (AI001 "no valid cited_evidence
            // block", RSN001 "evidence set truncated", …) instead of discarding them — without
            // these, an uncited answer is indistinguishable from a citation-parse failure.
            diagnostics: answer
                .diagnostics
                .iter()
                .map(|d| format!("{}: {}", d.code, d.message))
                .collect(),
            latency,
            cache_hit,
            resource: resource_delta,
            ..Default::default()
        },
        Err(e) => ScenarioRun {
            latency,
            cache_hit,
            resource: resource_delta,
            error: Some(e.to_string()),
            ..Default::default()
        },
    };

    // Trajectory signal — offline, cheap, independent of whether the LLM call above succeeded.
    if let Ok(plan) = ai.plan(&scenario.question) {
        run.planned_query_type = Some(format!("{:?}", plan.query_type).to_lowercase());
    }

    // RFC 0139 §1: capture what the model was *shown*. `gather_evidence` is the same offline
    // `plan` + `execute` pair `reason` runs internally (no LLM call, deterministic), so this
    // reproduces the evidence set the answer above was generated from. Without it, a failure
    // cannot be attributed to retrieval versus generation.
    if let Ok(evidence) = ai.gather_evidence(&scenario.question) {
        let mut text = String::new();
        for item in &evidence.items {
            text.push_str(&item.claim);
            if !item.location.is_empty() {
                text.push_str(" [");
                text.push_str(&item.location);
                text.push(']');
            }
            text.push('\n');
        }
        run.evidence_text = Some(text);
    }

    // Recall@k needs a ranked id list even for an LLM-answered scenario (RFC 0138 §2.2).
    //
    // Captured unconditionally, not just when the scenario currently declares `expected_objects`
    // (RFC 0139 §2.6). Gating on that made transcripts un-regradable against a *later* dataset
    // change: adding `expected_objects` to six scenarios scored them 0.0 on re-grade — not
    // because retrieval missed, but because no ranked list had been recorded. Retrieval in fact
    // ranked the expected object first or second. A saved transcript has to hold everything a
    // future ruler might ask about, or `regrade` quietly manufactures failures.
    //
    // RFC 0139 Phase 2's last open item: this used to search with the raw `scenario.question` —
    // a full natural-language sentence — while `reason::plan()` (what `ai.reason()` above just
    // ran) searches with `search_query(&understand(question, ..))`, a keyword-only string with
    // stopwords/punctuation already stripped. The two queries can rank differently, so the
    // recorded ranked list didn't always match what the model was actually shown. Falls back to
    // the raw question only if `understand` itself errors — recall is still worth measuring on
    // *something* rather than left absent (RFC 0139 §2.6's own reasoning against a `None`).
    let recall_query = understand(&scenario.question, runtime)
        .map(|u| search_query(&u))
        .unwrap_or_else(|_| scenario.question.clone());
    if let Ok(results) = runtime.retrieve(&RetrievalRequest::lexical(&recall_query)) {
        run.retrieved_ids = results.hits.into_iter().map(|h| h.id).collect();
    }

    run
}
