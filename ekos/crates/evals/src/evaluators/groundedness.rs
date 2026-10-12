//! Groundedness evaluator (RFC 0138) — the report's headline "Evidence groundedness" metric.
//!
//! For a normal scenario this is the same citation-validity ratio `evaluators::evidence` computes
//! (real signal reused, not duplicated logic — see [`score`]). For a `should_refuse` scenario
//! (RFC 0043's "never fabricate" tested directly) it means something different: 1.0 iff the answer
//! actually declined — a builtin refusal-phrase check plus no fabricated citation — and 0.0 if the
//! model invented an answer to a question with no grounded answer in the ledger.

use super::evidence::EvidenceCheck;
use crate::schema::Scenario;

/// "Insufficient evidence" is the canonical phrase this harness's Category G (unknown/adversarial)
/// scenarios are written to expect (RFC 0138 §Non-goals amendment, 2026-09-05) — the rest of the
/// list are equivalent real phrasings a grounded model reasonably uses instead, kept so a correct
/// refusal in different wording still passes.
const DEFAULT_REFUSAL_PHRASES: &[&str] = &[
    "insufficient evidence",
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
    "not enough evidence",
    "not enough information",
    "no mention",
    "not mentioned",
    "refusing",
    "no direct evidence",
    "not possible to determine",
    "claims mention",
    PREMISE_REJECTION,
];

/// The fixed opening the reason prompt asks for when a question asserts something the claims do
/// not support (RFC 0177 §2). A stated outcome, like "insufficient evidence" — not a loose phrase:
/// wording such as "does not" appears in fabricated answers too and is deliberately absent.
pub const PREMISE_REJECTION: &str = "premise not supported";

/// Whether `answer` reads as a refusal by [`DEFAULT_REFUSAL_PHRASES`] alone, with no
/// scenario-specific phrasing considered (RFC 0139-followup "D1" fix). Exposed to
/// `evaluators::answer` so a scenario that isn't `should_refuse` but got refused anyway doesn't
/// score a fact "matched" just because the refusal's own preamble echoes the fact's keyword (e.g.
/// "I looked for a claim naming `Observer`" restating exactly the entity the question named) — a
/// refusal never states a fact, regardless of what nouns it echoes while explaining what it
/// couldn't find.
pub(crate) fn looks_like_a_refusal(answer: &str) -> bool {
    let lower = answer.to_lowercase();
    DEFAULT_REFUSAL_PHRASES.iter().any(|p| lower.contains(p))
}

fn is_refusal(scenario: &Scenario, answer: &str) -> bool {
    looks_like_a_refusal(answer)
        || scenario
            .refusal_phrases
            .iter()
            .any(|p| answer.to_lowercase().contains(&p.to_lowercase()))
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
    match (evidence.cited, answer) {
        // RFC 0139 §2.3 — an answer that cites nothing is the *definition* of ungrounded, not a
        // scenario this metric doesn't apply to. Returning `None` here removed it from the
        // denominator instead of scoring it: measured on the R0 baseline, 59 of 101 scenarios
        // produced an answer and cited nothing, so the published 78.3% groundedness was a mean
        // over 46 scenarios — a metric mostly reporting on the scenarios that behaved.
        (0, Some(_)) => Some(0.0),
        // No answer at all (a runner error) genuinely has nothing to grade.
        (0, None) => None,
        (cited, _) => Some(evidence.valid as f32 / cited as f32),
    }
}

/// How far into an answer a refusal has to start for the answer to count as one when the scenario
/// expects an answer. Wide enough for a model that repeats the question first.
const REFUSAL_OPENING_CHARS: usize = 300;

/// Whether `answer` begins as a refusal: one of [`DEFAULT_REFUSAL_PHRASES`] within its first
/// [`REFUSAL_OPENING_CHARS`] characters.
///
/// Used where a scenario expects an answer. [`looks_like_a_refusal`] searches the whole text, and
/// a complete, cited answer that says "the claims show no such allowlisting" in its third
/// paragraph was scored as stating no fact at all (measured: `sec-010`, 2026-10-12).
pub(crate) fn opens_with_a_refusal(answer: &str) -> bool {
    let opening: String = answer.chars().take(REFUSAL_OPENING_CHARS).collect();
    looks_like_a_refusal(&opening)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The grader knows fixed words only, so the prompt has to state them (RFC 0139 §4.3,
    /// RFC 0177 §2): a model graded against words it was never shown is graded on luck.
    #[test]
    fn the_reason_prompt_states_both_openings_the_grader_accepts() {
        let prompt = ekos_runtime::AiRuntimeConfig::default()
            .reason_system_prompt
            .to_lowercase();
        assert!(prompt.contains("insufficient evidence."));
        assert!(prompt.contains(&format!("{PREMISE_REJECTION}.")));
    }

    /// Each opening is shown once, alone on its line. A second spelling of either would give the
    /// model a choice, and the grader accepts one.
    #[test]
    fn the_reason_prompt_shows_each_opening_once_on_its_own_line() {
        let prompt = ekos_runtime::AiRuntimeConfig::default().reason_system_prompt;
        for opening in ["Insufficient evidence.", "Premise not supported."] {
            let lines = prompt.lines().filter(|l| l.trim() == opening).count();
            assert_eq!(lines, 1, "{opening}");
            assert_eq!(prompt.matches(opening).count(), 1, "{opening}");
        }
    }

    #[test]
    fn a_refusal_phrase_deep_in_an_answer_does_not_make_it_open_as_a_refusal() {
        let answer = format!(
            "{} The claims show no such allowlisting.",
            "The `[security]` section can only extend the baseline. ".repeat(8)
        );
        assert!(looks_like_a_refusal(&answer));
        assert!(!opens_with_a_refusal(&answer));
    }

    #[test]
    fn a_refusal_after_the_repeated_question_still_opens_as_one() {
        assert!(opens_with_a_refusal(
            "What does the FooBarNonexistentAnalyzer do?\n\nInsufficient evidence. I looked for it."
        ));
    }

    #[test]
    fn a_premise_rejection_is_a_refusal() {
        assert!(looks_like_a_refusal(
            "Premise not supported. The claims show the vector index is EKOS's own module."
        ));
    }

    #[test]
    fn a_denial_in_loose_wording_is_not_a_refusal() {
        // "does not" reads like a denial and is in plenty of invented answers too.
        assert!(!looks_like_a_refusal(
            "The Enterprise Edition does not charge per seat; the fee is $4,000 a year."
        ));
    }
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

    /// RFC 0139 §2.3 changed this deliberately. An answered scenario that cites nothing used to
    /// return `None` — "not applicable" — which quietly removed it from groundedness's denominator
    /// instead of scoring it. On the R0 baseline that was 59 of 101 scenarios, so a published
    /// 78.3% was really a mean over the 46 that behaved. Citing nothing *is* ungrounded.
    #[test]
    fn an_answered_scenario_that_cites_nothing_scores_zero_not_not_applicable() {
        let s = scenario(false);
        let e = EvidenceCheck::default();
        assert_eq!(score(&s, Some("anything"), &e), Some(0.0));
    }

    #[test]
    fn a_scenario_with_no_answer_at_all_remains_not_applicable() {
        // A runner error genuinely has nothing to grade — distinct from an answer that chose not
        // to cite.
        let s = scenario(false);
        let e = EvidenceCheck::default();
        assert_eq!(score(&s, None, &e), None);
    }
}
