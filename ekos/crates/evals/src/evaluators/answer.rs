//! Answer-correctness evaluator (RFC 0138) — deterministic, no LLM judge: fraction of
//! `Scenario::expected_facts` present as a case-insensitive substring of the answer text.

use crate::schema::Scenario;

/// `(matched, total)` — exposed separately from [`score`] so `evaluators::completeness` can
/// combine this with `evaluators::evidence`'s match count without re-scanning the answer text.
pub fn matched_count(scenario: &Scenario, answer: Option<&str>) -> (usize, usize) {
    let total = scenario.expected_facts.len();
    if total == 0 {
        return (0, 0);
    }
    let matched = match answer {
        None => 0,
        Some(text) => {
            let lower = text.to_lowercase();
            scenario
                .expected_facts
                .iter()
                .filter(|f| lower.contains(&f.to_lowercase()))
                .count()
        }
    };
    (matched, total)
}

/// `None` when the scenario has no `expected_facts` (nothing to grade — excluded from the
/// report's average rather than silently scored as a pass).
pub fn score(scenario: &Scenario, answer: Option<&str>) -> Option<f32> {
    let (matched, total) = matched_count(scenario, answer);
    if total == 0 {
        None
    } else {
        Some(matched as f32 / total as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Mode;

    fn scenario(expected_facts: Vec<&str>) -> Scenario {
        Scenario {
            id: "t".into(),
            category: "test".into(),
            question: "q".into(),
            mode: Mode::Reason,
            difficulty: None,
            adversarial: false,
            should_refuse: false,
            refusal_phrases: vec![],
            expected_facts: expected_facts.into_iter().map(String::from).collect(),
            expected_evidence_contains: vec![],
            expected_objects: vec![],
            expected_query_type: None,
            pass_threshold: 0.7,
        }
    }

    #[test]
    fn not_applicable_when_no_expected_facts() {
        let s = scenario(vec![]);
        assert_eq!(score(&s, Some("anything")), None);
    }

    #[test]
    fn full_match() {
        let s = scenario(vec!["sql_analyzer", "recovery"]);
        assert_eq!(
            score(
                &s,
                Some("Implemented by sql_analyzer inside the recovery crate.")
            ),
            Some(1.0)
        );
    }

    #[test]
    fn partial_match_is_case_insensitive() {
        let s = scenario(vec!["SQL_Analyzer", "missing_term"]);
        assert_eq!(score(&s, Some("uses sql_analyzer only")), Some(0.5));
    }

    #[test]
    fn no_answer_scores_zero_not_none() {
        let s = scenario(vec!["x"]);
        assert_eq!(score(&s, None), Some(0.0));
    }
}
