//! Report aggregation + rendering (RFC 0138) — turns a run's [`EvalOutcome`]s into the five
//! headline metrics, a PASS/FAIL gate decision, the `ekos eval run` text report, and a
//! `Serialize`-able form for `--json` / the saved `evals/reports/<ts>-<dataset>.json` file.

use crate::evaluators::EvalOutcome;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

fn cpu_time_ms(d: Option<Duration>) -> Option<f64> {
    d.map(|d| d.as_secs_f64() * 1000.0)
}

/// Gate thresholds the report's `Status` line is decided against. A metric with no applicable
/// scenarios (`None`) never blocks the gate — you can't fail a bar nothing was measured against.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GateThresholds {
    pub min_answer_correctness: f32,
    pub min_evidence_groundedness: f32,
    pub min_completeness: f32,
    pub min_recall_at_10: f32,
    pub max_hallucination_rate: f32,
}

impl Default for GateThresholds {
    fn default() -> Self {
        Self {
            min_answer_correctness: 0.85,
            min_evidence_groundedness: 0.90,
            min_completeness: 0.80,
            min_recall_at_10: 0.80,
            max_hallucination_rate: 0.05,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioReport {
    pub id: String,
    pub passed: bool,
    pub hallucinated: bool,
    pub answer_score: Option<f32>,
    pub evidence_score: Option<f32>,
    pub completeness_score: Option<f32>,
    pub retrieval_recall: Option<f64>,
    pub groundedness_score: Option<f32>,
    pub trajectory_score: Option<f32>,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    /// `Some(true)` when served from the LLM provider's disk cache — no fresh tokens spent.
    /// `#[serde(default)]`: absent in a report saved before this field existed (RFC 0138's own
    /// report schema evolves — `ekos eval history` reads old and new reports side by side, so
    /// every field added after the first release needs to tolerate a missing key, not error).
    #[serde(default)]
    pub cache_hit: Option<bool>,
    #[serde(default)]
    pub rss_kb_end: Option<u64>,
    #[serde(default)]
    pub cpu_time_ms: Option<f64>,
    pub latency_ms: f64,
    pub error: Option<String>,
}

impl From<&EvalOutcome> for ScenarioReport {
    fn from(o: &EvalOutcome) -> Self {
        Self {
            id: o.scenario_id.clone(),
            passed: o.passed,
            hallucinated: o.hallucinated,
            answer_score: o.answer_score,
            evidence_score: o.evidence_score,
            completeness_score: o.completeness_score,
            retrieval_recall: o.retrieval_recall,
            groundedness_score: o.groundedness_score,
            trajectory_score: o.trajectory_score,
            input_tokens: o.tokens.map(|t| t.input_tokens),
            output_tokens: o.tokens.map(|t| t.output_tokens),
            cache_hit: o.cache_hit,
            rss_kb_end: o.resource.rss_kb_end,
            cpu_time_ms: cpu_time_ms(o.resource.cpu_time),
            latency_ms: o.latency.as_secs_f64() * 1000.0,
            error: o.error.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metrics {
    pub scenarios: usize,
    pub passed: usize,
    pub failed: usize,
    pub answer_correctness: Option<f32>,
    pub evidence_groundedness: Option<f32>,
    pub completeness: Option<f32>,
    pub recall_at_10: Option<f64>,
    pub hallucination_rate: f32,
    pub avg_tokens: Option<f64>,
    pub p95_latency_ms: f64,
    /// Scenarios whose LLM call was served from the disk cache — no fresh network call.
    /// `#[serde(default)]` on this and the rest of this struct's cache/resource fields: absent in
    /// a report saved before they existed — see `ScenarioReport::cache_hit`'s doc comment.
    #[serde(default)]
    pub cache_hits: usize,
    /// Scenarios whose LLM call was a genuine fresh network call.
    #[serde(default)]
    pub cache_misses: usize,
    /// Sum of `tokens` over cache-hit scenarios — real content that would have cost tokens again
    /// had the cache not existed, but didn't this run (RFC 0138's "tokens saved" metric).
    #[serde(default)]
    pub tokens_saved: Option<f64>,
    /// Highest RSS reading (KB) seen across every scenario — `None` off-Linux.
    #[serde(default)]
    pub peak_rss_kb: Option<u64>,
    /// Sum of per-scenario CPU time deltas — `None` off-Linux, or when no delta was measurable.
    #[serde(default)]
    pub total_cpu_time_ms: Option<f64>,
    pub status_pass: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub dataset: String,
    pub agent: String,
    pub runtime: String,
    pub generated_at: DateTime<Utc>,
    pub gates: GateThresholds,
    pub metrics: Metrics,
    pub scenarios: Vec<ScenarioReport>,
}

fn mean_f32(values: impl Iterator<Item = f32>) -> Option<f32> {
    let (sum, n) = values.fold((0.0f32, 0usize), |(s, n), v| (s + v, n + 1));
    (n > 0).then_some(sum / n as f32)
}

fn mean_f64(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, n) = values.fold((0.0f64, 0usize), |(s, n), v| (s + v, n + 1));
    (n > 0).then_some(sum / n as f64)
}

fn p95(mut latencies: Vec<Duration>) -> Duration {
    if latencies.is_empty() {
        return Duration::ZERO;
    }
    latencies.sort();
    let idx = ((latencies.len() as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(latencies.len() - 1);
    latencies[idx]
}

/// Build the full [`Report`] from graded scenarios. `runtime` is a human label for where the
/// store was opened from — always `"local"` in v1 (RFC 0138 §3 Non-goals: no distributed harness).
pub fn build(
    dataset: &str,
    agent: &str,
    runtime: &str,
    outcomes: &[EvalOutcome],
    gates: GateThresholds,
) -> Report {
    let scenarios: Vec<ScenarioReport> = outcomes.iter().map(ScenarioReport::from).collect();

    let passed = outcomes.iter().filter(|o| o.passed).count();
    let failed = outcomes.len() - passed;
    let hallucinated = outcomes.iter().filter(|o| o.hallucinated).count();
    let hallucination_rate = if outcomes.is_empty() {
        0.0
    } else {
        hallucinated as f32 / outcomes.len() as f32
    };

    let answer_correctness = mean_f32(outcomes.iter().filter_map(|o| o.answer_score));
    let evidence_groundedness = mean_f32(outcomes.iter().filter_map(|o| o.groundedness_score));
    let completeness = mean_f32(outcomes.iter().filter_map(|o| o.completeness_score));
    let recall_at_10 = mean_f64(outcomes.iter().filter_map(|o| o.retrieval_recall));
    let avg_tokens = mean_f64(
        outcomes
            .iter()
            .filter_map(|o| o.tokens.map(|t| (t.input_tokens + t.output_tokens) as f64)),
    );
    let p95_latency_ms = p95(outcomes.iter().map(|o| o.latency).collect()).as_secs_f64() * 1000.0;

    let cache_hits = outcomes
        .iter()
        .filter(|o| o.cache_hit == Some(true))
        .count();
    let cache_misses = outcomes
        .iter()
        .filter(|o| o.cache_hit == Some(false))
        .count();
    let tokens_saved: f64 = outcomes
        .iter()
        .filter(|o| o.cache_hit == Some(true))
        .filter_map(|o| o.tokens.map(|t| (t.input_tokens + t.output_tokens) as f64))
        .sum();
    let tokens_saved = (cache_hits > 0).then_some(tokens_saved);
    let peak_rss_kb = outcomes.iter().filter_map(|o| o.resource.rss_kb_end).max();
    let total_cpu_time_ms = {
        let (sum, n) = outcomes
            .iter()
            .filter_map(|o| o.resource.cpu_time)
            .fold((Duration::ZERO, 0usize), |(s, n), d| (s + d, n + 1));
        (n > 0).then_some(sum.as_secs_f64() * 1000.0)
    };

    let status_pass = answer_correctness.is_none_or(|v| v >= gates.min_answer_correctness)
        && evidence_groundedness.is_none_or(|v| v >= gates.min_evidence_groundedness)
        && completeness.is_none_or(|v| v >= gates.min_completeness)
        && recall_at_10.is_none_or(|v| v >= gates.min_recall_at_10 as f64)
        && hallucination_rate <= gates.max_hallucination_rate;

    Report {
        dataset: dataset.to_string(),
        agent: agent.to_string(),
        runtime: runtime.to_string(),
        generated_at: Utc::now(),
        gates,
        metrics: Metrics {
            scenarios: outcomes.len(),
            passed,
            failed,
            answer_correctness,
            evidence_groundedness,
            completeness,
            recall_at_10,
            hallucination_rate,
            avg_tokens,
            p95_latency_ms,
            cache_hits,
            cache_misses,
            tokens_saved,
            peak_rss_kb,
            total_cpu_time_ms,
            status_pass,
        },
        scenarios,
    }
}

fn fmt_pct(v: Option<f32>) -> String {
    match v {
        Some(v) => format!("{:.1}%", v * 100.0),
        None => "n/a".to_string(),
    }
}

fn fmt_pct64(v: Option<f64>) -> String {
    match v {
        Some(v) => format!("{:.1}%", v * 100.0),
        None => "n/a".to_string(),
    }
}

fn fmt_tokens(v: Option<f64>) -> String {
    match v {
        Some(v) => {
            let n = v.round() as i64;
            // Thousands separator — small, no dependency needed for a value this size.
            let s = n.to_string();
            let mut out = String::new();
            for (i, c) in s.chars().rev().enumerate() {
                if i > 0 && i % 3 == 0 {
                    out.push(',');
                }
                out.push(c);
            }
            out.chars().rev().collect()
        }
        None => "n/a".to_string(),
    }
}

fn fmt_latency(ms: f64) -> String {
    if ms >= 1000.0 {
        format!("{:.1}s", ms / 1000.0)
    } else {
        format!("{:.0}ms", ms)
    }
}

fn fmt_duration_opt(ms: Option<f64>) -> String {
    match ms {
        Some(ms) => fmt_latency(ms),
        None => "n/a".to_string(),
    }
}

fn fmt_rss(kb: Option<u64>) -> String {
    match kb {
        Some(kb) if kb >= 1024 => format!("{:.1} MB", kb as f64 / 1024.0),
        Some(kb) => format!("{kb} KB"),
        None => "n/a".to_string(),
    }
}

fn fmt_cache(hits: usize, misses: usize) -> String {
    let total = hits + misses;
    if total == 0 {
        "n/a".to_string()
    } else {
        format!("{hits}/{total}")
    }
}

/// `label` and `value` in independent fixed-width fields, so the value column lines up
/// regardless of how long any one row's label is (the bug an "adjust padding by label length"
/// version has: the right edge drifts per row instead of staying a straight column).
fn row(label: &str, value: &str) -> String {
    const LABEL_WIDTH: usize = 23;
    const VALUE_WIDTH: usize = 8;
    if label.len() >= LABEL_WIDTH {
        format!("{label} {value:>VALUE_WIDTH$}")
    } else {
        format!("{label:<LABEL_WIDTH$}{value:>VALUE_WIDTH$}")
    }
}

/// The text form of the worked example in `ekos/docs/rfcs/0138-eval-harness.md`.
pub fn render_text(report: &Report) -> String {
    let m = &report.metrics;
    let mut out = String::new();
    out.push_str("EKOS EVALUATION\n");
    out.push_str("─────────────────────────────\n\n");
    out.push_str(&format!("Dataset: {}\n", report.dataset));
    out.push_str(&format!("Agent: {}\n", report.agent));
    out.push_str(&format!("Runtime: {}\n\n", report.runtime));
    out.push_str(&format!(
        "{}\n",
        row("Scenarios:", &m.scenarios.to_string())
    ));
    out.push_str(&format!("{}\n", row("Passed:", &m.passed.to_string())));
    out.push_str(&format!("{}\n\n", row("Failed:", &m.failed.to_string())));
    out.push_str(&format!(
        "{}\n",
        row("Answer correctness:", &fmt_pct(m.answer_correctness))
    ));
    out.push_str(&format!(
        "{}\n",
        row("Evidence groundedness:", &fmt_pct(m.evidence_groundedness))
    ));
    out.push_str(&format!(
        "{}\n",
        row("Completeness:", &fmt_pct(m.completeness))
    ));
    out.push_str(&format!(
        "{}\n",
        row("Recall@10:", &fmt_pct64(m.recall_at_10))
    ));
    out.push_str(&format!(
        "{}\n\n",
        row("Hallucination rate:", &fmt_pct(Some(m.hallucination_rate)))
    ));
    out.push_str(&format!(
        "{}\n",
        row("Avg tokens:", &fmt_tokens(m.avg_tokens))
    ));
    out.push_str(&format!(
        "{}\n\n",
        row("P95 latency:", &fmt_latency(m.p95_latency_ms))
    ));
    out.push_str(&format!(
        "{}\n",
        row("Cache hits:", &fmt_cache(m.cache_hits, m.cache_misses))
    ));
    out.push_str(&format!(
        "{}\n",
        row("Tokens saved:", &fmt_tokens(m.tokens_saved))
    ));
    out.push_str(&format!("{}\n", row("Peak RSS:", &fmt_rss(m.peak_rss_kb))));
    out.push_str(&format!(
        "{}\n\n",
        row("CPU time:", &fmt_duration_opt(m.total_cpu_time_ms))
    ));
    out.push_str(&format!(
        "Status: {}\n",
        if m.status_pass { "PASS" } else { "FAIL" }
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluators::EvalOutcome;
    use crate::resource::ResourceDelta;
    use ekos_runtime::ai::TokenUsage;

    fn outcome(passed: bool, hallucinated: bool) -> EvalOutcome {
        EvalOutcome {
            scenario_id: "s".into(),
            answer_score: Some(0.9),
            evidence_score: Some(1.0),
            completeness_score: Some(0.9),
            retrieval_recall: Some(1.0),
            groundedness_score: Some(0.95),
            trajectory_score: None,
            hallucinated,
            tokens: Some(TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
            }),
            cache_hit: Some(false),
            resource: ResourceDelta::default(),
            latency: Duration::from_millis(500),
            error: None,
            passed,
        }
    }

    #[test]
    fn status_fail_when_gate_missed() {
        let outcomes = vec![outcome(false, true)];
        let report = build("t", "claude", "local", &outcomes, GateThresholds::default());
        assert!(!report.metrics.status_pass);
        assert_eq!(report.metrics.failed, 1);
    }

    #[test]
    fn status_pass_can_tolerate_some_scenario_failures() {
        // 19 clean passes + 1 hallucination: hallucination_rate 5% is exactly at the default gate
        // (<=0.05), everything else stays high — this mirrors the RFC's worked example (some
        // individual failures, still an overall PASS).
        let mut outcomes = vec![outcome(true, false); 19];
        outcomes.push(outcome(false, false));
        let report = build("t", "claude", "local", &outcomes, GateThresholds::default());
        assert_eq!(report.metrics.scenarios, 20);
        assert_eq!(report.metrics.failed, 1);
        assert!(report.metrics.status_pass);
    }

    #[test]
    fn render_text_contains_headline_metrics() {
        let outcomes = vec![outcome(true, false), outcome(true, false)];
        let report = build(
            "ekos-2",
            "claude",
            "local",
            &outcomes,
            GateThresholds::default(),
        );
        let text = render_text(&report);
        assert!(text.contains("EKOS EVALUATION"));
        assert!(text.contains("Dataset: ekos-2"));
        assert!(text.contains("Answer correctness:"));
        assert!(text.contains("Status: PASS"));
    }
}
