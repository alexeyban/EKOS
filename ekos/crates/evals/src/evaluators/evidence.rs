//! Evidence-citation evaluator (RFC 0138) — checks that every id an answer cited actually
//! resolves to a real `KnowledgeStore::get_evidence` entry (a hallucinated citation is an id the
//! ledger has never seen), and how many of `Scenario::expected_evidence_contains` show up in the
//! *valid* citations' fragment/path text.

use crate::schema::Scenario;
use ekos_kir::KirId;
use ekos_ledger::KnowledgeStore;

#[derive(Debug, Clone, Default)]
pub struct EvidenceCheck {
    pub cited: usize,
    pub valid: usize,
    pub invalid_ids: Vec<KirId>,
    pub matched_contains: usize,
    pub total_contains: usize,
}

pub fn check(
    scenario: &Scenario,
    evidence_refs: &[KirId],
    ledger: &dyn KnowledgeStore,
) -> EvidenceCheck {
    let mut valid = 0usize;
    let mut invalid_ids = Vec::new();
    let mut fragments = Vec::new();
    for id in evidence_refs {
        match ledger.get_evidence(id) {
            Ok(Some(ev)) => {
                valid += 1;
                fragments.push(format!("{} {}", ev.location.path, ev.fragment));
            }
            _ => invalid_ids.push(*id),
        }
    }

    let total_contains = scenario.expected_evidence_contains.len();
    let matched_contains = if total_contains == 0 {
        0
    } else {
        let haystack = fragments.join("\n").to_lowercase();
        scenario
            .expected_evidence_contains
            .iter()
            .filter(|s| haystack.contains(&s.to_lowercase()))
            .count()
    };

    EvidenceCheck {
        cited: evidence_refs.len(),
        valid,
        invalid_ids,
        matched_contains,
        total_contains,
    }
}

/// Fraction of cited evidence ids that are real (non-hallucinated). `None` when nothing was
/// cited — there is nothing to check, not a pass or a fail.
pub fn score(check: &EvidenceCheck) -> Option<f32> {
    if check.cited == 0 {
        None
    } else {
        Some(check.valid as f32 / check.cited as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Mode;
    use ekos_ledger::Ledger;

    fn scenario(expected_evidence_contains: Vec<&str>) -> Scenario {
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
            expected_evidence_contains: expected_evidence_contains
                .into_iter()
                .map(String::from)
                .collect(),
            expected_objects: vec![],
            expected_query_type: None,
            pass_threshold: 0.7,
        }
    }

    #[test]
    fn no_citations_is_not_applicable() {
        let dir = tempfile::tempdir().unwrap();
        let store = Ledger::open(&dir.path().join("db.sqlite")).unwrap();
        let c = check(&scenario(vec![]), &[], &store);
        assert_eq!(score(&c), None);
    }

    #[test]
    fn unknown_id_is_invalid_and_lowers_score() {
        let dir = tempfile::tempdir().unwrap();
        let store = Ledger::open(&dir.path().join("db.sqlite")).unwrap();
        let fake_id = KirId::new();
        let c = check(&scenario(vec![]), &[fake_id], &store);
        assert_eq!(c.cited, 1);
        assert_eq!(c.valid, 0);
        assert_eq!(c.invalid_ids, vec![fake_id]);
        assert_eq!(score(&c), Some(0.0));
    }
}
