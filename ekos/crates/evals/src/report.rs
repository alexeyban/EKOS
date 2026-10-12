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
    /// RFC 0139 §1 — which layer this scenario's miss belongs to (`retrieval`/`generation`/
    /// `passed`/`n/a`). Always recorded: it is one short string and it is the whole point of the
    /// attribution pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
    /// RFC 0177 §5 — every expected fact was in the evidence the model was given. Absent for a
    /// scenario without expected facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facts_shown: Option<bool>,
    /// RFC 0139 §1 — pipeline diagnostic codes (`AI001`, `RSN001`, …). Always recorded; these are
    /// small and are the direct evidence for citation-parse failures.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
    /// The answer text, the evidence the model was shown, and the ids it cited — written only
    /// under `--save-answers`, because 101 answers plus their evidence is ~1MB of JSON and most
    /// runs don't need it. Without it a saved report cannot be re-graded offline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retrieved_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_query_type: Option<String>,
}

impl From<&EvalOutcome> for ScenarioReport {
    /// Scores plus attribution and diagnostics — the cheap fields, always written.
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
            attribution: o.transcript.attribution.map(|a| a.as_str().to_string()),
            facts_shown: o.facts_shown,
            diagnostics: o.transcript.diagnostics.clone(),
            answer: None,
            evidence_text: None,
            evidence_refs: Vec::new(),
            retrieved_ids: Vec::new(),
            planned_query_type: None,
        }
    }
}

impl ScenarioReport {
    /// The full transcript form (RFC 0139 §1) — everything [`From`] writes, plus the answer text,
    /// the evidence the model was shown, and the ids it cited/retrieved. This is what makes a saved
    /// report re-gradable offline by `ekos eval regrade`; it is opt-in via `--save-answers` because
    /// a 101-scenario suite's transcripts are roughly a megabyte of JSON.
    pub fn with_transcript(o: &EvalOutcome) -> Self {
        Self {
            answer: o.transcript.answer.clone(),
            evidence_text: o.transcript.evidence_text.clone(),
            evidence_refs: o.transcript.evidence_refs.clone(),
            retrieved_ids: o.transcript.retrieved_ids.clone(),
            planned_query_type: o.transcript.planned_query_type.clone(),
            ..Self::from(o)
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
    /// Fabrications over **all** scenarios. Kept for continuity with RFC 0138's gate, but see
    /// `fabrication_rate` — this denominator counts scenarios where fabrication isn't even
    /// definable, so it understates the real behaviour by roughly 5x (RFC 0139 §2.4).
    pub hallucination_rate: f32,
    /// Fabrications over the `should_refuse` scenarios only — the denominator on which
    /// "did it refuse when it should have?" is actually defined (RFC 0139 §2.4). `None` when the
    /// dataset has no refusal scenarios at all.
    #[serde(default)]
    pub fabrication_rate: Option<f32>,
    /// Scenarios that cited at least one unresolvable evidence id, over those that cited anything
    /// (RFC 0139 §2.4). Separated from `hallucination_rate` because a refusal miss and a fabricated
    /// citation are different defects with different fixes — and this one currently measures 0.
    #[serde(default)]
    pub invalid_citation_rate: Option<f32>,
    /// Answers that are not refusals and cite nothing (RFC 0139 §2.3). Each scores 0 for
    /// groundedness. A refusal has nothing to cite and is counted in `refusals` (RFC 0177 §1).
    #[serde(default)]
    pub uncited_answers: usize,
    /// Answers that open as a refusal or a premise rejection, on any scenario.
    #[serde(default)]
    pub refusals: usize,
    /// Scenarios whose runner returned an error. Any makes the run fail (RFC 0177 §4).
    #[serde(default)]
    pub errored: usize,
    /// Of the scenarios stating expected facts, how many had every fact in the evidence the model
    /// was given, and how many such scenarios there were (RFC 0177 §5).
    #[serde(default)]
    pub facts_shown: usize,
    #[serde(default)]
    pub facts_scenarios: usize,
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
    /// RFC 0177 §5 — no model was called: only retrieval and evidence assembly were measured.
    #[serde(default)]
    pub evidence_only: bool,
    /// RFC 0177 §3 D — objects left out of search because their text quotes a scenario question.
    #[serde(default)]
    pub question_echoes_excluded: usize,
    pub dataset: String,
    pub agent: String,
    pub runtime: String,
    pub generated_at: DateTime<Utc>,
    pub gates: GateThresholds,
    pub metrics: Metrics,
    pub scenarios: Vec<ScenarioReport>,
    /// Which grading semantics produced these numbers (RFC 0139 §1). Bumped by any change to
    /// matching, denominators, composite weights, the alias table, or the datasets — so that two
    /// reports are only ever compared directly when this matches. Without it, a ruler change and a
    /// system change are indistinguishable in the trend table.
    #[serde(default = "default_ruler_version")]
    pub ruler_version: u32,
}

/// Reports written before `ruler_version` existed were all produced by the original RFC 0138
/// ruler, which is version 1 by definition.
fn default_ruler_version() -> u32 {
    1
}

/// The current grading semantics. **Bump this whenever grading changes** — see [`Report::ruler_version`].
///
/// - **v1** — RFC 0138 as shipped: raw case-insensitive substring containment on `expected_facts`.
/// - **v2** — RFC 0139 §2.1: token matching with separator/word-ending normalisation
///   (`crate::evaluators::normalize`) and `any_of` alternates. Removes false negatives where a
///   correct answer used a different word form, without loosening what counts as a fact.
/// - **v3** — RFC 0139 §2.2/§2.3: `completeness` no longer votes twice in the composite (it
///   re-uses `answer`'s match count, so one missed keyword cost two of three slots); the composite
///   is weighted rather than an unweighted mean; a scenario with no gradable signal fails visibly
///   instead of scoring a silent 1.0; and an answered-but-uncited scenario scores 0 for
///   groundedness instead of vanishing from its denominator.
/// - **v4** — RFC 0139 §2.5/§2.6 dataset hygiene: degenerate refusal phrases removed (`adv-014`
///   accepted a bare `"not"`, which matches almost any sentence), near-free-pass `expected_facts`
///   replaced with discriminating ones, and an unresolvable `expected_objects` name now scores 0
///   instead of silently shrinking the relevant set. A dataset edit changes grading, so it gets a
///   version like any evaluator change.
/// - **v5** — RFC 0139 §2.6 metric power: `expected_objects` added to 6 more scenarios (11 → 17),
///   so recall@10 stops being an ~11-sample mean where one scenario moves it 10 points. Only
///   specific identifiers were used; generic words like `ledger` or `build` also resolve to *an*
///   object, but not to the one their question is about — adding those would have manufactured
///   coverage rather than signal.
/// - **v6** — RFC 0177: a refusal is not an "answered but uncited" row; `Premise not supported.` is
///   a refusal; recall@10 is counted per expected name (any object bearing it), and a name that
///   resolves to nothing is a miss even beside one that resolves; a run with an errored scenario
///   fails.
/// - **v7** — RFC 0177 amendment: where a scenario expects an answer, only an answer that *begins*
///   as a refusal is graded as one. A refusal phrase found anywhere used to zero the answer score
///   of a complete, cited answer.
pub const RULER_VERSION: u32 = 7;

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
    build_with_transcripts(dataset, agent, runtime, outcomes, gates, false)
}

/// [`build`], with control over whether each scenario's transcript is written into the report
/// (RFC 0139 §1 — `ekos eval run --save-answers`). Transcripts are what make a saved report
/// re-gradable offline; they are opt-in only because of their size.
pub fn build_with_transcripts(
    dataset: &str,
    agent: &str,
    runtime: &str,
    outcomes: &[EvalOutcome],
    gates: GateThresholds,
    save_transcripts: bool,
) -> Report {
    let scenarios: Vec<ScenarioReport> = outcomes
        .iter()
        .map(|o| {
            if save_transcripts {
                ScenarioReport::with_transcript(o)
            } else {
                ScenarioReport::from(o)
            }
        })
        .collect();

    let passed = outcomes.iter().filter(|o| o.passed).count();
    let failed = outcomes.len() - passed;
    let hallucinated = outcomes.iter().filter(|o| o.hallucinated).count();
    let hallucination_rate = if outcomes.is_empty() {
        0.0
    } else {
        hallucinated as f32 / outcomes.len() as f32
    };

    // RFC 0139 §2.4 — the same numerator over the denominators it is actually defined on.
    let refusal_scenarios = outcomes.iter().filter(|o| o.should_refuse).count();
    let fabrication_rate = (refusal_scenarios > 0).then(|| {
        let fabricated = outcomes
            .iter()
            .filter(|o| o.should_refuse && o.hallucinated)
            .count();
        fabricated as f32 / refusal_scenarios as f32
    });
    let citing_scenarios = outcomes.iter().filter(|o| o.cited_count > 0).count();
    let invalid_citation_rate = (citing_scenarios > 0).then(|| {
        let bad = outcomes
            .iter()
            .filter(|o| o.invalid_citation_count > 0)
            .count();
        bad as f32 / citing_scenarios as f32
    });
    let uncited_answers = outcomes.iter().filter(|o| o.answered_uncited).count();
    let refusals = outcomes.iter().filter(|o| o.refused).count();
    let errored = outcomes.iter().filter(|o| o.error.is_some()).count();
    let facts_scenarios = outcomes.iter().filter(|o| o.facts_shown.is_some()).count();
    let facts_shown = outcomes
        .iter()
        .filter(|o| o.facts_shown == Some(true))
        .count();

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

    // RFC 0177 §4 — with every scenario errored each mean is `None` and every gate below holds
    // vacuously; a run that could not be measured has not passed.
    let status_pass = errored == 0
        && answer_correctness.is_none_or(|v| v >= gates.min_answer_correctness)
        && evidence_groundedness.is_none_or(|v| v >= gates.min_evidence_groundedness)
        && completeness.is_none_or(|v| v >= gates.min_completeness)
        && recall_at_10.is_none_or(|v| v >= gates.min_recall_at_10 as f64)
        && hallucination_rate <= gates.max_hallucination_rate;

    Report {
        evidence_only: false,
        question_echoes_excluded: 0,
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
            fabrication_rate,
            invalid_citation_rate,
            uncited_answers,
            refusals,
            errored,
            facts_shown,
            facts_scenarios,
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
        ruler_version: RULER_VERSION,
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
    out.push_str(&format!("Runtime: {}\n", report.runtime));
    if report.question_echoes_excluded > 0 {
        out.push_str(&format!(
            "Left out of search: {} objects that quote a scenario question\n",
            report.question_echoes_excluded
        ));
    }
    out.push('\n');
    if report.evidence_only {
        return render_evidence_only(report, out);
    }
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
        "{}\n",
        row("Hallucination rate:", &fmt_pct(Some(m.hallucination_rate)))
    ));
    // RFC 0139 §2.4 — the same failures over the denominators they're actually defined on. Printed
    // next to the headline rate rather than replacing it, so the gate number stays visible while
    // the honest one is impossible to miss.
    out.push_str(&format!(
        "{}\n",
        row(
            "  ├─ fabrication rate:",
            &format!("{} (of should_refuse only)", fmt_pct(m.fabrication_rate))
        )
    ));
    out.push_str(&format!(
        "{}\n",
        row(
            "  └─ invalid citations:",
            &format!("{} (of scenarios citing)", fmt_pct(m.invalid_citation_rate))
        )
    ));
    out.push_str(&format!(
        "{}\n",
        row(
            "Answered but uncited:",
            &format!("{} (each scores 0 for groundedness)", m.uncited_answers)
        )
    ));
    out.push_str(&format!("{}\n", row("Refusals:", &m.refusals.to_string())));
    out.push_str(&format!(
        "{}\n\n",
        row(
            "Facts in evidence:",
            &format!(
                "{}/{} (scenarios whose expected facts all reached the model)",
                m.facts_shown, m.facts_scenarios
            )
        )
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
    // RFC 0139 §1 — where the failures live. A pass rate says how bad things are; this says which
    // layer to fix, which is the question the RFC 0138 baseline could not answer.
    let attributed = |want: &str| {
        report
            .scenarios
            .iter()
            .filter(|s| s.attribution.as_deref() == Some(want))
            .count()
    };
    let (retrieval, generation) = (attributed("retrieval"), attributed("generation"));
    if retrieval + generation > 0 {
        out.push_str("Failure attribution (scenarios with expected_facts):\n");
        out.push_str(&format!(
            "{}\n",
            row(
                "  retrieval:",
                &format!("{retrieval} (fact never reached the model)")
            )
        ));
        out.push_str(&format!(
            "{}\n\n",
            row(
                "  generation:",
                &format!("{generation} (fact was shown, answer omitted it)")
            )
        ));
    }
    if m.errored > 0 {
        out.push_str(&format!(
            "{}\n\n",
            row(
                "Errored:",
                &format!("{} (a run with an errored scenario fails)", m.errored)
            )
        ));
    }
    out.push_str(&format!(
        "Status: {}\n",
        if m.status_pass { "PASS" } else { "FAIL" }
    ));
    out
}

/// The report of a run that called no model (RFC 0177 §5): retrieval and evidence assembly only.
fn render_evidence_only(report: &Report, mut out: String) -> String {
    let m = &report.metrics;
    out.push_str(&format!(
        "{}\n",
        row("Scenarios:", &m.scenarios.to_string())
    ));
    out.push_str(&format!(
        "{}\n",
        row("Recall@10:", &fmt_pct64(m.recall_at_10))
    ));
    out.push_str(&format!(
        "{}\n",
        row(
            "Facts in evidence:",
            &format!("{}/{}", m.facts_shown, m.facts_scenarios)
        )
    ));
    let missing: Vec<&str> = report
        .scenarios
        .iter()
        .filter(|s| s.facts_shown == Some(false))
        .map(|s| s.id.as_str())
        .collect();
    if !missing.is_empty() {
        out.push_str(&format!("  not shown: {}\n", missing.join(", ")));
    }
    let low: Vec<String> = report
        .scenarios
        .iter()
        .filter_map(|s| {
            s.retrieval_recall
                .filter(|r| *r < 1.0)
                .map(|r| format!("{} ({:.0}%)", s.id, r * 100.0))
        })
        .collect();
    if !low.is_empty() {
        out.push_str(&format!("  recall below 100%: {}\n", low.join(", ")));
    }
    if m.errored > 0 {
        out.push_str(&format!("{}\n", row("Errored:", &m.errored.to_string())));
    }
    out.push_str("\nNo model was called. Answers were not graded.\n");
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
            should_refuse: false,
            cited_count: 1,
            invalid_citation_count: 0,
            answered_uncited: false,
            refused: false,
            facts_shown: None,
            not_gradable: false,
            transcript: Default::default(),
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

    /// RFC 0177 §4 — every mean is `None` when every scenario errored, so every gate held
    /// vacuously and a run that measured nothing printed `PASS`.
    #[test]
    fn a_run_with_an_errored_scenario_fails_even_when_nothing_else_was_measured() {
        let errored = EvalOutcome {
            answer_score: None,
            evidence_score: None,
            completeness_score: None,
            retrieval_recall: None,
            groundedness_score: None,
            error: Some("api error 403".into()),
            passed: false,
            ..outcome(false, false)
        };
        let report = build("t", "a", "local", &[errored], GateThresholds::default());
        assert_eq!(report.metrics.errored, 1);
        assert!(!report.metrics.status_pass);
        assert!(render_text(&report).contains("Errored:"));
    }

    /// RFC 0177 §1 — a refusal has nothing to cite.
    #[test]
    fn refusals_and_uncited_answers_are_counted_apart() {
        let refusal = EvalOutcome {
            refused: true,
            cited_count: 0,
            ..outcome(true, false)
        };
        let uncited = EvalOutcome {
            answered_uncited: true,
            cited_count: 0,
            ..outcome(false, false)
        };
        let report = build(
            "t",
            "a",
            "local",
            &[refusal, uncited],
            GateThresholds::default(),
        );
        assert_eq!(
            (report.metrics.refusals, report.metrics.uncited_answers),
            (1, 1)
        );
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
