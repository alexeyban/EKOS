//! Completeness evaluator (RFC 0138) — did the answer cover everything the scenario asked for:
//! combines `answer`'s fact coverage and `evidence`'s expected-substring coverage into one ratio.
//! Deliberately does *not* fold in `retrieval` recall — that is graded and reported separately
//! since it's a different pipeline stage (what was found vs. what was said about it).

use super::evidence::EvidenceCheck;
use crate::evaluators::answer;
use crate::schema::Scenario;

/// `None` when the scenario has neither `expected_facts` nor `expected_evidence_contains` —
/// nothing to be complete about.
pub fn score(
    scenario: &Scenario,
    answer_text: Option<&str>,
    evidence: &EvidenceCheck,
) -> Option<f32> {
    let (matched_facts, total_facts) = answer::matched_count(scenario, answer_text);
    let total = total_facts + evidence.total_contains;
    if total == 0 {
        return None;
    }
    let matched = matched_facts + evidence.matched_contains;
    Some(matched as f32 / total as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Mode;

    fn scenario(facts: Vec<&str>, evidence_contains: Vec<&str>) -> Scenario {
        Scenario {
            id: "t".into(),
            category: "test".into(),
            question: "q".into(),
            mode: Mode::Reason,
            difficulty: None,
            adversarial: false,
            should_refuse: false,
            refusal_phrases: vec![],
            expected_facts: facts.into_iter().map(String::from).collect(),
            expected_evidence_contains: evidence_contains.into_iter().map(String::from).collect(),
            expected_objects: vec![],
            expected_query_type: None,
            pass_threshold: 0.7,
        }
    }

    #[test]
    fn not_applicable_when_nothing_expected() {
        let s = scenario(vec![], vec![]);
        let e = EvidenceCheck::default();
        assert_eq!(score(&s, Some("anything"), &e), None);
    }

    #[test]
    fn combines_facts_and_evidence_coverage() {
        let s = scenario(vec!["alpha", "beta"], vec!["gamma"]);
        let e = EvidenceCheck {
            matched_contains: 1,
            total_contains: 1,
            ..Default::default()
        };
        // 1/2 facts matched + 1/1 evidence matched = 2/3
        let got = score(&s, Some("mentions alpha only"), &e).unwrap();
        assert!((got - (2.0 / 3.0)).abs() < 1e-6, "{got}");
    }
}
