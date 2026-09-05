//! Trajectory evaluator (RFC 0138) — the one offline signal available without deeper
//! instrumentation of `AiRuntime`'s internal retrieve→expand→ground→ask pipeline (a named, honest
//! v1 limit — see the RFC's Non-goals): does the REASON rules planner (RFC 0123) route this
//! question to the `QueryType` the scenario expects? No LLM call, no network.

use crate::schema::Scenario;

/// `None` when the scenario doesn't set `expected_query_type`, or when no plan was gathered for
/// it (e.g. a `retrieval`-mode scenario, which never calls `AiRuntime::plan`).
pub fn score(scenario: &Scenario, planned_query_type: Option<&str>) -> Option<f32> {
    let expected = scenario.expected_query_type.as_deref()?;
    let got = planned_query_type?;
    Some(if expected.eq_ignore_ascii_case(got) {
        1.0
    } else {
        0.0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Mode;

    fn scenario(expected: Option<&str>) -> Scenario {
        Scenario {
            id: "t".into(),
            category: "test".into(),
            question: "q".into(),
            mode: Mode::Reason,
            difficulty: None,
            adversarial: false,
            should_refuse: false,
            refusal_phrases: vec![],
            expected_facts: vec![],
            expected_evidence_contains: vec![],
            expected_objects: vec![],
            expected_query_type: expected.map(String::from),
            pass_threshold: 0.7,
        }
    }

    #[test]
    fn not_applicable_when_unset() {
        assert_eq!(score(&scenario(None), Some("lexical")), None);
    }

    #[test]
    fn not_applicable_when_no_plan_gathered() {
        assert_eq!(score(&scenario(Some("lexical")), None), None);
    }

    #[test]
    fn case_insensitive_match() {
        assert_eq!(
            score(&scenario(Some("Lexical")), Some("lexical")),
            Some(1.0)
        );
    }

    #[test]
    fn mismatch_scores_zero() {
        assert_eq!(
            score(&scenario(Some("structural")), Some("lexical")),
            Some(0.0)
        );
    }
}
