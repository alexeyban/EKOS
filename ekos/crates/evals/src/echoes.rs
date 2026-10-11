//! Question echoes (RFC 0177 §3 D).
//!
//! The corpus an eval runs against can contain the eval's own questions: an RFC or a devlog that
//! discusses a failing scenario quotes it. Such a passage matches the question word for word and
//! outranks the object that answers it, so the scenario measures the quote. `ekos eval run` finds
//! those objects once, before the run, and leaves them out of search.

use crate::schema::Scenario;
use ekos_kir::{KirId, KirObject};
use std::collections::HashSet;

/// A question shorter than this is a search query (`recovery crate`), and ordinary text contains
/// it by chance.
const MIN_QUESTION_WORDS: usize = 6;

fn normalized(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Ids of the `objects` whose name or indexed text holds a scenario's question verbatim, ignoring
/// case and line wrapping.
pub fn question_echoes(objects: &[KirObject], scenarios: &[Scenario]) -> HashSet<KirId> {
    let questions: Vec<String> = scenarios
        .iter()
        .filter(|s| s.question.split_whitespace().count() >= MIN_QUESTION_WORDS)
        .map(|s| normalized(&s.question))
        .collect();
    if questions.is_empty() {
        return HashSet::new();
    }
    objects
        .iter()
        .filter(|o| {
            let text = normalized(&format!("{} {}", o.name, o.indexed_content()));
            questions.iter().any(|q| text.contains(q.as_str()))
        })
        .map(|o| o.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Mode;
    use ekos_kir::ObjectKind;

    fn scenario(question: &str) -> Scenario {
        Scenario {
            id: "s".into(),
            question: question.into(),
            category: "t".into(),
            mode: Mode::Reason,
            difficulty: None,
            adversarial: false,
            should_refuse: false,
            refusal_phrases: vec![],
            expected_facts: vec![],
            expected_evidence_contains: vec![],
            expected_objects: vec![],
            expected_query_type: None,
            pass_threshold: 0.7,
        }
    }

    fn section(text: &str) -> KirObject {
        KirObject::new("doc § notes", ObjectKind::Custom("Section".into()))
            .with_property("excerpt", serde_json::json!(text))
    }

    #[test]
    fn a_passage_quoting_the_question_across_a_line_break_is_an_echo() {
        let quoting =
            section("Scenario code-002 asks *\"What\nfunction builds the Widget used by ask?\"*");
        let answering = section("build_widget builds the Widget that ask uses.");
        let found = question_echoes(
            &[quoting.clone(), answering],
            &[scenario("What function builds the Widget used by ask?")],
        );
        assert_eq!(found, HashSet::from([quoting.id]));
    }

    #[test]
    fn a_short_query_excludes_nothing() {
        let doc = section("The recovery crate holds every analyzer.");
        assert!(question_echoes(&[doc], &[scenario("recovery crate")]).is_empty());
    }
}
