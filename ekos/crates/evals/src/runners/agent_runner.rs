//! Runs `mode: reason`/`ask` scenarios (RFC 0138) through an already-constructed `AiRuntime` —
//! opening the store and building the `LlmProvider` is the CLI command's job (`ask.rs`'s own
//! pattern), so this module never touches configuration or credentials.

use super::ScenarioRun;
use crate::resource::{self, ResourceDelta};
use crate::schema::{Mode, Scenario};
use ekos_runtime::{AiRuntime, RetrievalRequest, Runtime};
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

    // Recall@k needs a ranked id list even for an LLM-answered scenario (RFC 0138 §2.2) — reuse
    // the same lexical retrieval a pure `retrieval`-mode scenario would run.
    if !scenario.expected_objects.is_empty()
        && let Ok(results) = runtime.retrieve(&RetrievalRequest::lexical(&scenario.question))
    {
        run.retrieved_ids = results.hits.into_iter().map(|h| h.id).collect();
    }

    run
}
