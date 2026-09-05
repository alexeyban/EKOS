//! Runners (RFC 0138) — execute one [`crate::schema::Scenario`] against an already-open
//! `Runtime`/`AiRuntime` and capture the raw outcome. Runners never grade anything; grading is
//! `crate::evaluators`' job, kept separate so a runner's output is reusable across evaluators.

pub mod agent_runner;
pub mod retrieval_runner;

use crate::resource::ResourceDelta;
use ekos_kir::KirId;
use ekos_runtime::ai::TokenUsage;
use std::time::Duration;

/// The raw result of running one scenario — either an LLM answer (`mode: reason`/`ask`) or a bare
/// retrieval (`mode: retrieval`), plus whatever offline signal was gathered for the trajectory
/// evaluator. Optional fields are `None` when that scenario's `mode` didn't produce them.
#[derive(Debug, Clone, Default)]
pub struct ScenarioRun {
    pub answer: Option<String>,
    pub evidence_refs: Vec<KirId>,
    pub token_usage: Option<TokenUsage>,
    /// Ranked object ids from `Runtime::retrieve`, gathered whenever a scenario has
    /// `expected_objects` — regardless of `mode` — so recall@k can be graded on an LLM-answered
    /// scenario too, not only pure `retrieval`-mode ones.
    pub retrieved_ids: Vec<KirId>,
    /// `AiRuntime::plan(question).query_type`, as a lowercase string (`"lookup"`, `"lexical"`, …)
    /// — the one offline trajectory signal available without deeper pipeline instrumentation
    /// (RFC 0138 §2.3).
    pub planned_query_type: Option<String>,
    pub latency: Duration,
    /// `Some(true)` if this scenario's LLM call (if any) was served from `CachedLlmProvider`'s
    /// disk cache — real tokens were not spent for it. `None` for a `retrieval`-mode scenario
    /// (no LLM call at all) or when the provider isn't cached.
    pub cache_hit: Option<bool>,
    /// Process RSS/CPU delta measured around this one scenario's execution (RFC 0138) — best
    /// effort, `None` fields off-Linux. Diagnostic only, never gates pass/fail.
    pub resource: ResourceDelta,
    /// Set when the runner itself errored (LLM call failed, store error, …) — a scenario with an
    /// error is always a hard fail, never silently scored.
    pub error: Option<String>,
}
