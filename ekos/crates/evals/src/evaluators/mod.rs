//! Evaluators (RFC 0138) — pure grading functions over a [`crate::runners::ScenarioRun`]. Every
//! score is `Option<f32>`/`Option<f64>`: `None` means "not applicable to this scenario", excluded
//! from that metric's report-wide average rather than silently counted as a pass.

pub mod answer;
pub mod completeness;
pub mod evidence;
pub mod groundedness;
pub mod retrieval;
pub mod trajectory;

use crate::runners::ScenarioRun;
use crate::schema::Scenario;
use ekos_ledger::KnowledgeStore;
use ekos_runtime::ai::TokenUsage;
use std::time::Duration;

/// Every score gathered for one scenario, plus the derived pass/fail + hallucination flag the
/// report aggregates over.
#[derive(Debug, Clone)]
pub struct EvalOutcome {
    pub scenario_id: String,
    pub answer_score: Option<f32>,
    /// Raw citation-validity ratio (`evaluators::evidence`) — not one of the report's five
    /// headline metrics (`groundedness_score` is), but kept per-scenario for `--json` output.
    pub evidence_score: Option<f32>,
    pub completeness_score: Option<f32>,
    pub retrieval_recall: Option<f64>,
    pub groundedness_score: Option<f32>,
    pub trajectory_score: Option<f32>,
    pub hallucinated: bool,
    pub tokens: Option<TokenUsage>,
    pub latency: Duration,
    pub error: Option<String>,
    pub passed: bool,
}

/// Grade one scenario's [`ScenarioRun`] against its own expectations. `ledger` is used only for
/// evidence-citation validity checks and object-name resolution — never mutated.
pub fn evaluate(
    scenario: &Scenario,
    run: &ScenarioRun,
    ledger: &dyn KnowledgeStore,
) -> EvalOutcome {
    if let Some(err) = &run.error {
        return EvalOutcome {
            scenario_id: scenario.id.clone(),
            answer_score: None,
            evidence_score: None,
            completeness_score: None,
            retrieval_recall: None,
            groundedness_score: None,
            trajectory_score: None,
            hallucinated: false,
            tokens: run.token_usage,
            latency: run.latency,
            error: Some(err.clone()),
            passed: false,
        };
    }

    let answer_text = run.answer.as_deref();
    let evidence_check = evidence::check(scenario, &run.evidence_refs, ledger);

    let answer_score = answer::score(scenario, answer_text);
    let evidence_score = evidence::score(&evidence_check);
    let completeness_score = completeness::score(scenario, answer_text, &evidence_check);
    let groundedness_score = groundedness::score(scenario, answer_text, &evidence_check);
    let trajectory_score = trajectory::score(scenario, run.planned_query_type.as_deref());
    let retrieval_recall = retrieval::recall_at_10(scenario, &run.retrieved_ids, ledger);

    let hallucinated = !evidence_check.invalid_ids.is_empty()
        || (scenario.should_refuse && groundedness_score.is_some_and(|s| s < 1.0));

    let applicable_scores: Vec<f32> = [
        answer_score,
        completeness_score,
        groundedness_score,
        trajectory_score,
    ]
    .into_iter()
    .flatten()
    .chain(retrieval_recall.map(|r| r as f32))
    .collect();
    let composite = if applicable_scores.is_empty() {
        1.0 // nothing was gradable (e.g. a bare retrieval scenario whose only signal is recall,
    // already folded in above) — don't fail a scenario for having no applicable checks
    } else {
        applicable_scores.iter().sum::<f32>() / applicable_scores.len() as f32
    };
    let passed = composite >= scenario.pass_threshold && !hallucinated;

    EvalOutcome {
        scenario_id: scenario.id.clone(),
        answer_score,
        evidence_score,
        completeness_score,
        retrieval_recall,
        groundedness_score,
        trajectory_score,
        hallucinated,
        tokens: run.token_usage,
        latency: run.latency,
        error: None,
        passed,
    }
}
