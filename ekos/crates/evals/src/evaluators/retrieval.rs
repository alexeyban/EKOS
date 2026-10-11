//! Retrieval evaluator (RFC 0138) — recall@10 of `Scenario::expected_objects` against a ranked
//! id list. Recall is counted per expected *name* (RFC 0177): any object bearing the name, in the top
//! ten, finds it.

use crate::schema::Scenario;
use ekos_kir::KirId;
use ekos_ledger::KnowledgeStore;
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
    // No ranked list was recorded at all — the scenario cannot be scored on retrieval, which is
    // different from retrieval having found nothing. Scoring it 0.0 would invent a failure; this
    // is exactly the "missing data graded as a failed claim" mistake RFC 0139 §2.3 fixed in
    // groundedness, and it shows up here whenever an older transcript is re-graded against a
    // dataset that has since gained `expected_objects`.
    if ranked_ids.is_empty() {
        return None;
    }
    // RFC 0177 — a name is not an id. Several objects can carry one name (a file and the document
    // read from it; a function and the import path that names it), and a map from name to a
    // single id kept whichever the ledger listed last: a query that ranked the file first scored
    // 0 when the map happened to hold its twin. An expected name is found when any object bearing
    // it is in the top ten.
    let mut name_to_ids: HashMap<String, Vec<KirId>> = HashMap::new();
    for object in ledger.all_objects().unwrap_or_default() {
        name_to_ids
            .entry(object.name.to_lowercase())
            .or_default()
            .push(object.id);
    }
    let top: &[KirId] = &ranked_ids[..ranked_ids.len().min(10)];
    // RFC 0139 §2.6 — a name that resolves to nothing is a miss, not a smaller relevant set: a
    // typo'd or renamed object must not make recall look better.
    let found = scenario
        .expected_objects
        .iter()
        .filter(|name| {
            name_to_ids
                .get(&name.to_lowercase())
                .is_some_and(|ids| ids.iter().any(|id| top.contains(id)))
        })
        .count();
    Some(found as f64 / scenario.expected_objects.len() as f64)
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

    /// RFC 0177 — a file and the document read from it carry one name.
    #[test]
    fn a_name_borne_by_two_objects_is_found_when_either_is_ranked() {
        let dir = tempfile::tempdir().unwrap();
        let store = Ledger::open(&dir.path().join("db.sqlite")).unwrap();
        let file = KirObject::new("docs/a.md", ObjectKind::File);
        let document = KirObject::new("docs/a.md", ObjectKind::Custom("Document".into()));
        store.append_object(&file).unwrap();
        store.append_object(&document).unwrap();
        let s = scenario(vec!["docs/a.md"]);
        assert_eq!(recall_at_10(&s, &[file.id], &store), Some(1.0));
        assert_eq!(recall_at_10(&s, &[document.id], &store), Some(1.0));
    }

    #[test]
    fn a_name_that_resolves_to_nothing_is_a_miss_beside_one_that_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let store = Ledger::open(&dir.path().join("db.sqlite")).unwrap();
        let found = KirObject::new("real", ObjectKind::File);
        store.append_object(&found).unwrap();
        let s = scenario(vec!["real", "renamed_away"]);
        assert_eq!(recall_at_10(&s, &[found.id], &store), Some(0.5));
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

    /// RFC 0139 §2.6 — an expectation naming an object that doesn't exist (a typo, or an object
    /// renamed since the scenario was written) must not quietly make recall easier.
    #[test]
    fn an_unresolvable_expected_object_scores_zero_rather_than_vanishing() {
        let dir = tempfile::tempdir().unwrap();
        let store = ekos_ledger::Ledger::open(&dir.path().join("db.sqlite")).unwrap();
        let mut s = scenario(vec!["no-such-object-anywhere"]);
        s.expected_objects = vec!["no-such-object-anywhere".into()];
        assert_eq!(recall_at_10(&s, &[KirId::new()], &store), Some(0.0));
    }
}
