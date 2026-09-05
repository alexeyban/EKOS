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

pub mod evaluators;
pub mod report;
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
    let mut outcomes = Vec::with_capacity(scenarios.len());
    for scenario in scenarios {
        let run = match scenario.mode {
            Mode::Retrieval => runners::retrieval_runner::run(runtime, scenario),
            Mode::Reason | Mode::Ask => runners::agent_runner::run(ai, runtime, scenario).await,
        };
        outcomes.push(evaluators::evaluate(scenario, &run, ledger));
    }
    outcomes
}
