//! `ekos-evals` — RFC 0138: end-to-end agent/answer evaluation harness.
//!
//! Loads scenario datasets (`evals/datasets/*.yaml`), runs them against an already-open
//! `Runtime`/`AiRuntime` (opening the store and building the `LlmProvider` stays the CLI
//! command's job — see `crates/cli/src/commands/eval.rs`), grades each with a deterministic
//! evaluator suite (no LLM judge), and aggregates the result into the `ekos eval run` report.
//!
//! ```text
//! schema   — Scenario/Dataset/Manifest YAML types + load_dataset
//! runners  — agent_runner (LLM-answered scenarios), retrieval_runner (bare retrieval)
//! evaluators — answer, evidence, retrieval, completeness, groundedness, trajectory
//! report   — aggregation + the text/JSON report
//! ```

pub mod echoes;
pub mod evaluators;
pub mod history;
pub mod regrade;
pub mod report;
pub mod resource;
pub mod runners;
pub mod schema;

use ekos_ledger::KnowledgeStore;
use ekos_runtime::{AiRuntime, Runtime};
use schema::{Mode, Scenario};

/// Run every scenario in `scenarios` against `ai`/`runtime` and grade each one. `runtime` and the
/// `Runtime` inside `ai` must be the same store handle (see `runners::agent_runner::run`'s doc
/// comment for why this isn't hidden behind a single parameter).
pub async fn run_all(
    ai: &AiRuntime<'_>,
    runtime: &Runtime<'_>,
    ledger: &dyn KnowledgeStore,
    scenarios: &[Scenario],
) -> Vec<evaluators::EvalOutcome> {
    run_all_with(ai, runtime, ledger, scenarios, false).await
}

/// [`run_all`]; with `evidence_only` no model is called (RFC 0177 §5).
pub async fn run_all_with(
    ai: &AiRuntime<'_>,
    runtime: &Runtime<'_>,
    ledger: &dyn KnowledgeStore,
    scenarios: &[Scenario],
    evidence_only: bool,
) -> Vec<evaluators::EvalOutcome> {
    let mut outcomes = Vec::with_capacity(scenarios.len());
    for scenario in scenarios {
        let run = match scenario.mode {
            Mode::Retrieval => runners::retrieval_runner::run(runtime, scenario),
            Mode::Reason | Mode::Ask => {
                runners::agent_runner::run_with(ai, runtime, scenario, evidence_only).await
            }
        };
        outcomes.push(evaluators::evaluate(scenario, &run, ledger));
    }
    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirEvidence, KirObject, ObjectKind, SourceLocation};
    use ekos_ledger::Ledger;
    use ekos_recovery::llm::MockLlmProvider;
    use ekos_runtime::AiRuntimeConfig;
    use std::sync::Arc;

    fn scenario(id: &str, question: &str, mode: Mode, expected_objects: Vec<&str>) -> Scenario {
        Scenario {
            id: id.into(),
            question: question.into(),
            category: "test".into(),
            mode,
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

    /// Real end-to-end wiring test — no LLM judge in the harness itself, but this exercises the
    /// harness's own use of a real `LlmProvider` (mocked, not stubbed-out) across all three
    /// `Mode`s in one seeded store, the way `ekos eval run` actually drives it.
    #[tokio::test]
    async fn run_all_drives_ask_reason_and_retrieval_modes_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let store = Ledger::open(&dir.path().join("db.sqlite")).unwrap();

        let obj = KirObject::new(
            "WidgetFactory",
            ObjectKind::Custom("RustSymbol".to_string()),
        );
        let ev = KirEvidence::new(
            SourceLocation::file("src/widget_factory.rs"),
            "WidgetFactory builds widgets from raw material",
        );
        store.append_evidence(&ev).unwrap();
        let mut obj = obj;
        obj.evidence.push(ev.id);
        store.append_object(&obj).unwrap();

        let runtime = Runtime::over(&store);
        let response = format!(
            "WidgetFactory builds widgets.\n\n{{\"cited_evidence\": [\"{}\"]}}",
            ev.id
        );
        let llm = Arc::new(MockLlmProvider::new(response));
        let ai = AiRuntime::new(&runtime, llm, AiRuntimeConfig::default());

        let scenarios = vec![
            scenario("s-ask", "WidgetFactory", Mode::Ask, vec!["WidgetFactory"]),
            scenario("s-reason", "WidgetFactory", Mode::Reason, vec![]),
            scenario(
                "s-retrieval",
                "WidgetFactory",
                Mode::Retrieval,
                vec!["WidgetFactory"],
            ),
        ];

        let outcomes = run_all(&ai, &runtime, &store, &scenarios).await;
        assert_eq!(outcomes.len(), 3);
        for outcome in &outcomes {
            assert!(
                outcome.error.is_none(),
                "{}: {:?}",
                outcome.scenario_id,
                outcome.error
            );
        }
        // Ask mode found the object via lexical search and ran a real recall@10 check against it.
        assert_eq!(outcomes[0].retrieval_recall, Some(1.0));
        // Retrieval mode never calls the LLM, so nothing token-related was gathered.
        assert!(outcomes[2].tokens.is_none());
    }

    /// RFC 0177 §5 — the evidence-only run grades retrieval and asks no model.
    #[tokio::test]
    async fn an_evidence_only_run_calls_no_model_and_still_says_whether_the_fact_was_shown() {
        let dir = tempfile::tempdir().unwrap();
        let store = Ledger::open(&dir.path().join("db.sqlite")).unwrap();
        let ev = KirEvidence::new(SourceLocation::file("src/w.rs"), "fn build_widget() {}");
        store.append_evidence(&ev).unwrap();
        let mut obj = KirObject::new("build_widget", ObjectKind::Custom("RustSymbol".to_string()))
            .with_property("description", serde_json::json!("Assembles a sprocket."));
        obj.evidence.push(ev.id);
        store.append_object(&obj).unwrap();

        let runtime = Runtime::over(&store);
        let llm = Arc::new(MockLlmProvider::new("THE MODEL WAS CALLED".to_string()));
        let ai = AiRuntime::new(&runtime, llm, AiRuntimeConfig::default());
        let mut shown = scenario("shown", "build_widget", Mode::Reason, vec![]);
        shown.expected_facts = vec!["sprocket".into()];
        let mut missing = scenario("missing", "build_widget", Mode::Reason, vec![]);
        missing.expected_facts = vec!["flange".into()];

        let outcomes = run_all_with(&ai, &runtime, &store, &[shown, missing], true).await;
        for outcome in &outcomes {
            assert!(outcome.error.is_none(), "{:?}", outcome.error);
            assert_eq!(outcome.transcript.answer, None);
            assert!(outcome.tokens.is_none());
        }
        assert_eq!(outcomes[0].facts_shown, Some(true));
        assert_eq!(outcomes[1].facts_shown, Some(false));
    }
}
