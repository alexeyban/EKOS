//! Evaluators (RFC 0138) — pure grading functions over a [`crate::runners::ScenarioRun`]. Every
//! score is `Option<f32>`/`Option<f64>`: `None` means "not applicable to this scenario", excluded
//! from that metric's report-wide average rather than silently counted as a pass.

pub mod answer;
pub mod completeness;
pub mod evidence;
pub mod groundedness;
pub mod normalize;
pub mod retrieval;
pub mod trajectory;

use crate::resource::ResourceDelta;
use crate::runners::ScenarioRun;
use crate::schema::Scenario;
use ekos_ledger::KnowledgeStore;
use ekos_runtime::ai::TokenUsage;
use std::time::Duration;

/// Every score gathered for one scenario, plus the derived pass/fail + hallucination flag the
/// report aggregates over.
#[derive(Debug, Clone)]
pub struct EvalOutcome {
    pub scenario_id: String,
    pub answer_score: Option<f32>,
    /// Raw citation-validity ratio (`evaluators::evidence`) — not one of the report's five
    /// headline metrics (`groundedness_score` is), but kept per-scenario for `--json` output.
    pub evidence_score: Option<f32>,
    pub completeness_score: Option<f32>,
    pub retrieval_recall: Option<f64>,
    pub groundedness_score: Option<f32>,
    pub trajectory_score: Option<f32>,
    pub hallucinated: bool,
    pub tokens: Option<TokenUsage>,
    /// `Some(true)` when this scenario's answer was served from the LLM provider's disk cache —
    /// no fresh tokens were actually spent (RFC 0138's "tokens saved" metric).
    pub cache_hit: Option<bool>,
    pub resource: ResourceDelta,
    pub latency: Duration,
    pub error: Option<String>,
    pub passed: bool,
    /// RFC 0139 §2.4 — the facts the report needs to compute honest denominators: fabrication is
    /// only definable on `should_refuse` scenarios, and an invalid citation is only definable on
    /// scenarios that cited something. Carried here so `report::build` doesn't need the `Scenario`.
    pub should_refuse: bool,
    pub cited_count: usize,
    pub invalid_citation_count: usize,
    /// An answer was produced but cited nothing — the population `groundedness` silently excludes
    /// (RFC 0139 §2.3).
    pub answered_uncited: bool,
    /// RFC 0139 §2.2 — the scenario carried no applicable check at all. Previously such a scenario
    /// scored a silent 1.0 and passed; now it fails visibly, because a suite that cannot grade a
    /// question should say so rather than count it as a success.
    pub not_gradable: bool,
    /// RFC 0139 §1 — the transcript, carried through so a saved report can be re-graded offline
    /// (`ekos eval regrade`) and so a failure can be attributed to retrieval, generation, or the
    /// ruler without re-running the scenario by hand.
    pub transcript: Transcript,
}

/// What the model was actually shown and actually said (RFC 0139 §1). Kept out of `EvalOutcome`'s
/// score fields so grading logic can't accidentally depend on it — evaluators take a
/// [`ScenarioRun`], not this.
#[derive(Debug, Clone, Default)]
pub struct Transcript {
    pub answer: Option<String>,
    pub evidence_text: Option<String>,
    pub evidence_refs: Vec<String>,
    pub retrieved_ids: Vec<String>,
    pub planned_query_type: Option<String>,
    pub diagnostics: Vec<String>,
    /// Which layer this scenario's failure is attributable to — see [`attribute`].
    pub attribution: Option<Attribution>,
}

/// Where a scenario's missed `expected_facts` went missing (RFC 0139 §1). Deterministic: computed
/// only from the answer text and the ids/claims already captured, never from a second LLM call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attribution {
    /// Every expected fact appeared in the answer — nothing to attribute.
    Passed,
    /// The fact reached the model (it is present in the cited evidence) but not the answer.
    Generation,
    /// The fact is absent from both the answer and the evidence the model was given.
    Retrieval,
    /// The scenario has no `expected_facts`, so this axis says nothing about it.
    NotApplicable,
}

impl Attribution {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Generation => "generation",
            Self::Retrieval => "retrieval",
            Self::NotApplicable => "n/a",
        }
    }
}

/// Attribute a scenario's outcome to the layer responsible (RFC 0139 §1).
///
/// The question this answers is the one the RFC 0138 baseline could not: when a scenario scores
/// 0.0, was the fact missing from what retrieval found, or present and ignored by the model? Those
/// need opposite fixes, and a score alone cannot tell them apart.
///
/// Deterministic and offline — plain substring containment over text already captured, matching
/// [`answer::matched_count`]'s own semantics so attribution and grading never disagree about what
/// "present" means.
pub fn attribute(scenario: &Scenario, run: &ScenarioRun) -> Attribution {
    if scenario.expected_facts.is_empty() {
        return Attribution::NotApplicable;
    }
    let (matched, total) = answer::matched_count(scenario, run.answer.as_deref());
    if matched == total {
        return Attribution::Passed;
    }
    // A fact the model was shown but did not state is a generation failure; one that never
    // appeared in the evidence at all is a retrieval failure. When both kinds are present, the
    // retrieval gap is the more fundamental one — report it.
    // Same normalisation the ruler uses, so attribution and grading never disagree about whether
    // a fact is "present" (RFC 0139 §2.1).
    let evidence_tokens = normalize::tokens(run.evidence_text.as_deref().unwrap_or(""));
    let answer_tokens = normalize::tokens(run.answer.as_deref().unwrap_or(""));
    let present = |tokens: &[String], fact: &crate::schema::ExpectedFact| {
        fact.alternates()
            .iter()
            .any(|alt| normalize::contains_tokens(tokens, &normalize::tokens(alt)))
    };
    let missing_from_evidence = scenario
        .expected_facts
        .iter()
        .filter(|f| !present(&answer_tokens, f))
        .any(|f| !present(&evidence_tokens, f));
    if missing_from_evidence {
        Attribution::Retrieval
    } else {
        Attribution::Generation
    }
}

/// Grade one scenario's [`ScenarioRun`] against its own expectations. `ledger` is used only for
/// evidence-citation validity checks and object-name resolution — never mutated.
pub fn evaluate(
    scenario: &Scenario,
    run: &ScenarioRun,
    ledger: &dyn KnowledgeStore,
) -> EvalOutcome {
    if let Some(err) = &run.error {
        return EvalOutcome {
            scenario_id: scenario.id.clone(),
            answer_score: None,
            evidence_score: None,
            completeness_score: None,
            retrieval_recall: None,
            groundedness_score: None,
            trajectory_score: None,
            hallucinated: false,
            tokens: run.token_usage,
            cache_hit: run.cache_hit,
            resource: run.resource,
            latency: run.latency,
            error: Some(err.clone()),
            passed: false,
            should_refuse: scenario.should_refuse,
            cited_count: 0,
            invalid_citation_count: 0,
            answered_uncited: false,
            not_gradable: false,
            transcript: transcript_of(run, Attribution::NotApplicable),
        };
    }

    let answer_text = run.answer.as_deref();
    let evidence_check = evidence::check(scenario, &run.evidence_refs, ledger);

    let answer_score = answer::score(scenario, answer_text);
    let evidence_score = evidence::score(&evidence_check);
    let completeness_score = completeness::score(scenario, answer_text, &evidence_check);
    let groundedness_score = groundedness::score(scenario, answer_text, &evidence_check);
    let trajectory_score = trajectory::score(scenario, run.planned_query_type.as_deref());
    let retrieval_recall = retrieval::recall_at_10(scenario, &run.retrieved_ids, ledger);

    let hallucinated = !evidence_check.invalid_ids.is_empty()
        || (scenario.should_refuse && groundedness_score.is_some_and(|s| s < 1.0));

    // RFC 0139 §2.2 — a weighted composite over the axes that carry independent information.
    //
    // `completeness_score` is deliberately absent. It reuses `answer::matched_count`, so for the
    // 55 scenarios with `expected_facts` but no `expected_evidence_contains` it is a near-copy of
    // `answer_score` — and in an unweighted mean both slots moved together, meaning one missed
    // keyword cost *two of three* slots. A typical reason scenario therefore landed at
    // (0 + 0 + 1.0)/3 = 0.33 against a 0.7 threshold and failed on a single substring miss.
    // Completeness is still computed, reported and gated; it just no longer votes twice.
    let weighted: Vec<(f32, f32)> = [
        (answer_score, 0.45),
        (groundedness_score, 0.30),
        (retrieval_recall.map(|r| r as f32), 0.15),
        (trajectory_score, 0.10),
    ]
    .into_iter()
    .filter_map(|(score, weight)| score.map(|s| (s, weight)))
    .collect();
    // Renormalise over whichever axes applied, so a scenario is judged only on what it actually
    // measures rather than being penalised for the checks it doesn't carry.
    let total_weight: f32 = weighted.iter().map(|(_, w)| w).sum();
    let composite = (total_weight > 0.0)
        .then(|| weighted.iter().map(|(s, w)| s * w).sum::<f32>() / total_weight);
    // RFC 0139 §2.2 — no gradable signal used to mean `composite = 1.0`, i.e. a free pass. The
    // dataset test at `schema.rs` should make this unreachable, which is exactly why it must be
    // loud rather than silently inflate the pass rate if it ever happens.
    let not_gradable = composite.is_none();
    let passed = composite.is_some_and(|c| c >= scenario.pass_threshold) && !hallucinated;

    EvalOutcome {
        scenario_id: scenario.id.clone(),
        answer_score,
        evidence_score,
        completeness_score,
        retrieval_recall,
        groundedness_score,
        trajectory_score,
        hallucinated,
        tokens: run.token_usage,
        cache_hit: run.cache_hit,
        resource: run.resource,
        latency: run.latency,
        error: None,
        passed,
        should_refuse: scenario.should_refuse,
        cited_count: evidence_check.cited,
        invalid_citation_count: evidence_check.invalid_ids.len(),
        answered_uncited: run.answer.is_some() && evidence_check.cited == 0,
        not_gradable,
        transcript: transcript_of(run, attribute(scenario, run)),
    }
}

/// Copy the captured transcript out of a [`ScenarioRun`] for the report (RFC 0139 §1). Ids are
/// stringified here rather than in `report.rs` so the report layer stays free of `KirId`.
fn transcript_of(run: &ScenarioRun, attribution: Attribution) -> Transcript {
    Transcript {
        answer: run.answer.clone(),
        evidence_text: run.evidence_text.clone(),
        evidence_refs: run.evidence_refs.iter().map(|id| id.to_string()).collect(),
        retrieved_ids: run.retrieved_ids.iter().map(|id| id.to_string()).collect(),
        planned_query_type: run.planned_query_type.clone(),
        diagnostics: run.diagnostics.clone(),
        attribution: Some(attribution),
    }
}

#[cfg(test)]
mod attribution_tests {
    use super::*;
    use crate::schema::{Mode, Scenario};

    fn scenario(expected_facts: &[&str]) -> Scenario {
        Scenario {
            id: "t".into(),
            category: "test".into(),
            question: "q".into(),
            mode: Mode::Reason,
            difficulty: None,
            adversarial: false,
            should_refuse: false,
            refusal_phrases: vec![],
            expected_facts: expected_facts.iter().map(|s| (*s).into()).collect(),
            expected_evidence_contains: vec![],
            expected_objects: vec![],
            expected_query_type: None,
            pass_threshold: 0.7,
        }
    }

    fn run(answer: &str, evidence: &str) -> ScenarioRun {
        ScenarioRun {
            answer: Some(answer.into()),
            evidence_text: Some(evidence.into()),
            ..Default::default()
        }
    }

    #[test]
    fn matched_fact_is_attributed_to_nothing() {
        let s = scenario(&["sql_analyzer"]);
        let r = run("It lives in sql_analyzer.", "sql_analyzer [x.rs]");
        assert_eq!(attribute(&s, &r), Attribution::Passed);
    }

    #[test]
    fn fact_shown_but_not_stated_is_a_generation_failure() {
        let s = scenario(&["sql_analyzer"]);
        // The evidence names it; the answer paraphrases around it.
        let r = run(
            "It is handled by the recovery crate.",
            "sql_analyzer [x.rs]",
        );
        assert_eq!(attribute(&s, &r), Attribution::Generation);
    }

    #[test]
    fn fact_absent_from_evidence_is_a_retrieval_failure() {
        let s = scenario(&["sql_analyzer"]);
        let r = run("It is handled by the recovery crate.", "something else");
        assert_eq!(attribute(&s, &r), Attribution::Retrieval);
    }

    #[test]
    fn a_scenario_without_expected_facts_is_not_attributable() {
        let s = scenario(&[]);
        let r = run("anything", "anything");
        assert_eq!(attribute(&s, &r), Attribution::NotApplicable);
    }

    #[test]
    fn missing_evidence_text_does_not_masquerade_as_generation() {
        // No captured evidence (e.g. an older report, or gather_evidence failed) must not be read
        // as "the model was shown the fact" — that would silently blame the wrong layer.
        let s = scenario(&["sql_analyzer"]);
        let r = ScenarioRun {
            answer: Some("the recovery crate".into()),
            evidence_text: None,
            ..Default::default()
        };
        assert_eq!(attribute(&s, &r), Attribution::Retrieval);
    }

    #[test]
    fn a_partial_miss_reports_the_retrieval_gap_over_the_generation_one() {
        let s = scenario(&["shown_fact", "missing_fact"]);
        let r = run("neither stated here", "shown_fact [x.rs]");
        assert_eq!(attribute(&s, &r), Attribution::Retrieval);
    }
}
