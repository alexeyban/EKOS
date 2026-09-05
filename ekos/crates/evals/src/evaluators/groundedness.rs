//! Groundedness evaluator (RFC 0138) — the report's headline "Evidence groundedness" metric.
//!
//! For a normal scenario this is the same citation-validity ratio `evaluators::evidence` computes
//! (real signal reused, not duplicated logic — see [`score`]). For a `should_refuse` scenario
//! (RFC 0043's "never fabricate" tested directly) it means something different: 1.0 iff the answer
//! actually declined — a builtin refusal-phrase check plus no fabricated citation — and 0.0 if the
//! model invented an answer to a question with no grounded answer in the ledger.

use super::evidence::EvidenceCheck;
use crate::schema::Scenario;

const DEFAULT_REFUSAL_PHRASES: &[&str] = &[
    "cannot find",
    "can't find",
    "could not find",
    "couldn't find",
    "no such",
    "does not exist",
    "doesn't exist",
    "not present",
    "no evidence",
    "i don't have",
    "i do not have",
    "unable to find",
    "no information",
    "not found in",
    "no record of",
    "cannot answer",
    "can't answer",
];

fn is_refusal(scenario: &Scenario, answer: &str) -> bool {
    let lower = answer.to_lowercase();
    DEFAULT_REFUSAL_PHRASES.iter().any(|p| lower.contains(p))
        || scenario
            .refusal_phrases
            .iter()
            .any(|p| lower.contains(&p.to_lowercase()))
}

/// `None` for a normal scenario with nothing cited (same "not applicable" rule as
/// `evaluators::evidence::score`). Always `Some` for a `should_refuse` scenario.
pub fn score(scenario: &Scenario, answer: Option<&str>, evidence: &EvidenceCheck) -> Option<f32> {
    if scenario.should_refuse {
        let refused = answer.is_some_and(|a| is_refusal(scenario, a));
        let no_fabricated_citation = evidence.cited == 0 || evidence.valid == evidence.cited;
        return Some(if refused && no_fabricated_citation {
            1.0
        } else {
            0.0
        });
    }
    if evidence.cited == 0 {
        None
    } else {
        Some(evidence.valid as f32 / evidence.cited as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Mode;

    fn scenario(should_refuse: bool) -> Scenario {
        Scenario {
            id: "t".into(),
            category: "test".into(),
            question: "q".into(),
            mode: Mode::Reason,
            difficulty: None,
            adversarial: should_refuse,
            should_refuse,
            refusal_phrases: vec![],
            expected_facts: vec![],
            expected_evidence_contains: vec![],
            expected_objects: vec![],
            expected_query_type: None,
            pass_threshold: 0.7,
        }
    }

    #[test]
    fn should_refuse_and_did_refuse_scores_one() {
        let s = scenario(true);
        let e = EvidenceCheck::default();
        assert_eq!(
            score(
                &s,
                Some("I could not find any such object in the ledger."),
                &e
            ),
            Some(1.0)
        );
    }

    #[test]
    fn should_refuse_but_fabricated_answer_scores_zero() {
        let s = scenario(true);
        let e = EvidenceCheck::default();
        assert_eq!(
            score(&s, Some("It handles the checkout flow."), &e),
            Some(0.0)
        );
    }

    #[test]
    fn should_refuse_with_refusal_text_but_fabricated_citation_scores_zero() {
        let s = scenario(true);
        let e = EvidenceCheck {
            cited: 2,
            valid: 1,
            ..Default::default()
        };
        assert_eq!(
            score(&s, Some("I could not find any such object."), &e),
            Some(0.0)
        );
    }

    #[test]
    fn normal_scenario_no_citation_not_applicable() {
        let s = scenario(false);
        let e = EvidenceCheck::default();
        assert_eq!(score(&s, Some("anything"), &e), None);
    }
}
