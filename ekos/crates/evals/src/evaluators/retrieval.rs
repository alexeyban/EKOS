//! Retrieval evaluator (RFC 0138) — recall@10 of `Scenario::expected_objects` against a ranked
//! id list. Deliberately reuses `ekos_runtime::retrieval_eval::recall_at_k` (RFC 0126) rather than
//! reimplementing rank-metric math a second time.

use crate::schema::Scenario;
use ekos_kir::KirId;
use ekos_ledger::KnowledgeStore;
use ekos_runtime::retrieval_eval::recall_at_k;
use std::collections::HashMap;

/// Resolve `Scenario::expected_objects` (names) to ids and score `ranked_ids`' recall@10 against
/// them. `None` when the scenario names no expected objects. Name resolution mirrors RFC 0126's
/// own `evaluate()`: exact, case-insensitive match against every object currently in the ledger —
/// names are stable identifiers a human writes in a dataset file, ids aren't.
pub fn recall_at_10(
    scenario: &Scenario,
    ranked_ids: &[KirId],
    ledger: &dyn KnowledgeStore,
) -> Option<f64> {
    if scenario.expected_objects.is_empty() {
        return None;
    }
    let name_to_id: HashMap<String, KirId> = ledger
        .all_objects()
        .unwrap_or_default()
        .into_iter()
        .map(|o| (o.name.to_lowercase(), o.id))
        .collect();
    let relevant: Vec<KirId> = scenario
        .expected_objects
        .iter()
        .filter_map(|n| name_to_id.get(&n.to_lowercase()).copied())
        .collect();
    Some(recall_at_k(ranked_ids, &relevant, 10))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Mode;
    use ekos_kir::{KirObject, ObjectKind};
    use ekos_ledger::Ledger;

    fn scenario(expected_objects: Vec<&str>) -> Scenario {
        Scenario {
            id: "t".into(),
            category: "test".into(),
            question: "q".into(),
            mode: Mode::Retrieval,
            difficulty: None,
            adversarial: false,
            should_refuse: false,
            refusal_phrases: vec![],
            expected_facts: vec![],
            expected_evidence_contains: vec![],
            expected_objects: expected_objects.into_iter().map(String::from).collect(),
            expected_query_type: None,
            pass_threshold: 0.7,
        }
    }

    #[test]
    fn not_applicable_when_no_expected_objects() {
        let dir = tempfile::tempdir().unwrap();
        let store = Ledger::open(&dir.path().join("db.sqlite")).unwrap();
        assert_eq!(recall_at_10(&scenario(vec![]), &[], &store), None);
    }

    #[test]
    fn perfect_recall_when_object_is_top_ranked() {
        let dir = tempfile::tempdir().unwrap();
        let store = Ledger::open(&dir.path().join("db.sqlite")).unwrap();
        let obj = KirObject::new(
            "sql_analyzer::SqlAnalyzerPass",
            ObjectKind::Custom("RustSymbol".to_string()),
        );
        store.append_object(&obj).unwrap();

        let s = scenario(vec!["sql_analyzer::SqlAnalyzerPass"]);
        let got = recall_at_10(&s, &[obj.id], &store).unwrap();
        assert_eq!(got, 1.0);
    }

    #[test]
    fn zero_recall_when_object_missing_from_ranking() {
        let dir = tempfile::tempdir().unwrap();
        let store = Ledger::open(&dir.path().join("db.sqlite")).unwrap();
        let obj = KirObject::new(
            "sql_analyzer::SqlAnalyzerPass",
            ObjectKind::Custom("RustSymbol".to_string()),
        );
        store.append_object(&obj).unwrap();
        let other = KirId::new();

        let s = scenario(vec!["sql_analyzer::SqlAnalyzerPass"]);
        let got = recall_at_10(&s, &[other], &store).unwrap();
        assert_eq!(got, 0.0);
    }
}
