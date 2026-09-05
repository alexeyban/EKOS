//! Runs `mode: retrieval` scenarios (RFC 0138) — a bare `Runtime::retrieve` call, no `AiRuntime`,
//! no LLM, no token/answer fields populated. Existing purely so recall@k can be graded on
//! scenarios that don't need (or shouldn't pay for) a real LLM call.

use super::ScenarioRun;
use crate::schema::Scenario;
use ekos_runtime::{RetrievalRequest, Runtime};
use std::time::Instant;

pub fn run(runtime: &Runtime<'_>, scenario: &Scenario) -> ScenarioRun {
    let start = Instant::now();
    let result = runtime.retrieve(&RetrievalRequest::lexical(&scenario.question));
    let latency = start.elapsed();

    match result {
        Ok(results) => ScenarioRun {
            retrieved_ids: results.hits.into_iter().map(|h| h.id).collect(),
            latency,
            ..Default::default()
        },
        Err(e) => ScenarioRun {
            latency,
            error: Some(e.to_string()),
            ..Default::default()
        },
    }
}
