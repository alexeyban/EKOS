//! Offline re-grading of a saved report (RFC 0139 §1/§2).
//!
//! Changing how answers are graded makes new numbers non-comparable to old ones, which is how a
//! ruler change quietly gets read as a system improvement. This module is the guard against that:
//! given a report saved with `ekos eval run --save-answers`, it rebuilds each scenario's
//! [`ScenarioRun`] from the stored transcript and re-runs the *current* evaluators over it —
//! **no LLM calls, no retrieval, nothing non-deterministic**.
//!
//! So a grading change can always be published as a pair: the old ruler and the new one, scored
//! over identical answers. Whatever moves between those two columns is the ruler, and nothing else.

use crate::evaluators::{self, EvalOutcome};
use crate::report::{GateThresholds, Report, ScenarioReport};
use crate::runners::ScenarioRun;
use crate::schema::Scenario;
use ekos_kir::KirId;
use ekos_ledger::KnowledgeStore;
use std::collections::HashMap;
use std::time::Duration;

/// Why a saved report can't be re-graded.
#[derive(Debug, thiserror::Error)]
pub enum RegradeError {
    #[error("report has no saved transcripts — re-run with `--save-answers` to make it regradable")]
    NoTranscripts,
    #[error("scenario {0} is in the report but not in the loaded dataset")]
    UnknownScenario(String),
}

/// Rebuild the run a saved scenario report came from.
///
/// Only the fields the evaluators actually read are reconstructed; timing and resource figures are
/// carried through unchanged so the re-graded report keeps the original run's cost profile rather
/// than implying this offline pass measured anything.
fn run_from(saved: &ScenarioReport) -> ScenarioRun {
    ScenarioRun {
        answer: saved.answer.clone(),
        evidence_refs: saved
            .evidence_refs
            .iter()
            .filter_map(|s| s.parse::<KirId>().ok())
            .collect(),
        retrieved_ids: saved
            .retrieved_ids
            .iter()
            .filter_map(|s| s.parse::<KirId>().ok())
            .collect(),
        planned_query_type: saved.planned_query_type.clone(),
        evidence_text: saved.evidence_text.clone(),
        diagnostics: saved.diagnostics.clone(),
        token_usage: None,
        latency: Duration::from_secs_f64(saved.latency_ms / 1000.0),
        cache_hit: saved.cache_hit,
        resource: Default::default(),
        error: saved.error.clone(),
    }
}

/// Re-grade every scenario in `saved` under the current evaluators.
///
/// `scenarios` supplies the expectations to grade against — pass the dataset as it is *now* to see
/// the combined effect of evaluator and dataset changes, which is what `ruler_version` covers.
pub fn regrade(
    saved: &Report,
    scenarios: &[Scenario],
    ledger: &dyn KnowledgeStore,
) -> Result<Report, RegradeError> {
    if saved.scenarios.iter().all(|s| s.answer.is_none()) {
        return Err(RegradeError::NoTranscripts);
    }
    let by_id: HashMap<&str, &Scenario> = scenarios.iter().map(|s| (s.id.as_str(), s)).collect();

    let mut outcomes: Vec<EvalOutcome> = Vec::with_capacity(saved.scenarios.len());
    for entry in &saved.scenarios {
        let scenario = by_id
            .get(entry.id.as_str())
            .ok_or_else(|| RegradeError::UnknownScenario(entry.id.clone()))?;
        outcomes.push(evaluators::evaluate(scenario, &run_from(entry), ledger));
    }

    Ok(crate::report::build_with_transcripts(
        &saved.dataset,
        &saved.agent,
        &saved.runtime,
        &outcomes,
        GateThresholds::default(),
        true,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build_with_transcripts;
    use crate::schema::Mode;

    fn scenario(id: &str, facts: &[&str]) -> Scenario {
        Scenario {
            id: id.into(),
            category: "t".into(),
            question: "q".into(),
            mode: Mode::Reason,
            difficulty: None,
            adversarial: false,
            should_refuse: false,
            refusal_phrases: vec![],
            expected_facts: facts.iter().map(|s| s.to_string()).collect(),
            expected_evidence_contains: vec![],
            expected_objects: vec![],
            expected_query_type: None,
            pass_threshold: 0.7,
        }
    }

    fn temp_ledger() -> (ekos_ledger::Ledger, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let ledger = ekos_ledger::Ledger::open(&dir.path().join("db.sqlite")).unwrap();
        (ledger, dir)
    }

    fn saved_report(answer: &str) -> Report {
        let s = scenario("t-1", &["widget"]);
        let run = ScenarioRun {
            answer: Some(answer.into()),
            ..Default::default()
        };
        let (ledger, _d) = temp_ledger();
        let outcome = evaluators::evaluate(&s, &run, &ledger);
        build_with_transcripts(
            "d",
            "a",
            "local",
            &[outcome],
            GateThresholds::default(),
            true,
        )
    }

    #[test]
    fn regrading_the_same_answers_reproduces_the_same_scores() {
        // The property the audit trail depends on: regrade is a pure re-scoring, so an unchanged
        // ruler over unchanged answers must land on exactly the original numbers. If this drifts,
        // no v1-vs-v2 comparison built on it means anything.
        let saved = saved_report("the widget is here");
        let (ledger, _d) = temp_ledger();
        let again = regrade(&saved, &[scenario("t-1", &["widget"])], &ledger).unwrap();
        assert_eq!(
            again.metrics.answer_correctness,
            saved.metrics.answer_correctness
        );
        assert_eq!(again.metrics.passed, saved.metrics.passed);
    }

    #[test]
    fn regrading_under_changed_expectations_moves_the_score() {
        // The other half: when the dataset's expectations change, regrade must reflect it — that
        // is the whole point of being able to re-score old answers.
        let saved = saved_report("the widget is here");
        let (ledger, _d) = temp_ledger();
        let stricter = regrade(&saved, &[scenario("t-1", &["nonexistent-term"])], &ledger).unwrap();
        assert_eq!(stricter.metrics.answer_correctness, Some(0.0));
    }

    #[test]
    fn a_report_without_transcripts_is_refused_rather_than_silently_mis_scored() {
        let mut saved = saved_report("the widget is here");
        for s in &mut saved.scenarios {
            s.answer = None;
        }
        let (ledger, _d) = temp_ledger();
        assert!(matches!(
            regrade(&saved, &[scenario("t-1", &["widget"])], &ledger),
            Err(RegradeError::NoTranscripts)
        ));
    }
}
