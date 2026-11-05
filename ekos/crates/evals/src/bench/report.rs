//! The benchmark run report, the tuning record and `compare` (RFC 0240 §2.3 item 5, §2.5, §2.6).
//!
//! Reports are JSON under `evals/reports/bench/<suite>/`, next to RFC 0138's reports, and are
//! listed by `ekos eval history`. A report is self-describing: the cell's whole configuration
//! and its hash, the prompt hash, each ledger fingerprint, the split and the hash of its question
//! ids, `n` with a bootstrap interval, tokens, cost and every per-question result.

use super::stats::{self, Interval};
use super::{ExperimentConfig, QuestionResult, Split};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const REPORT_VERSION: u32 = 1;

/// Accuracy over a group of questions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Breakdown {
    pub n: usize,
    pub ex: f64,
}

/// Run-level numbers. Percentages are 0–100.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    /// EX, with its bootstrap 95% interval.
    pub ex: Interval,
    pub soft_f1: Interval,
    pub rves: f64,
    pub grounding_rate: f64,
    pub executed_rate: f64,
    pub schema_recall: Option<f64>,
    pub join_recall: Option<f64>,
    pub value_hit_rate: Option<f64>,
    pub by_difficulty: BTreeMap<String, Breakdown>,
    pub by_database: BTreeMap<String, Breakdown>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub cost_known: bool,
    pub latency_mean_ms: f64,
    pub latency_p95_ms: u64,
    /// RFC 0241: questions per failure category (`correct`, `no_sql`, …).
    #[serde(default)]
    pub categories: BTreeMap<String, usize>,
    /// RFC 0241: answers with no SQL that used the whole output allowance.
    #[serde(default)]
    pub truncated: usize,
    /// RFC 0241: calls served by the LLM cache, and the tokens of calls that were billed.
    #[serde(default)]
    pub calls: usize,
    #[serde(default)]
    pub cached_calls: usize,
    #[serde(default)]
    pub billed_input_tokens: u64,
    #[serde(default)]
    pub billed_output_tokens: u64,
    /// RFC 0242: model replies per question (`loop` / `agent` cells), in total.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub turns: usize,
    /// RFC 0242: per finding code left open on the final SQL, how many of those answers were
    /// scored (correct, wrong) — a detector's precision on that run.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub open_findings: BTreeMap<String, (usize, usize)>,
}

fn is_zero(v: &usize) -> bool {
    *v == 0
}

/// One `ekos eval bench run`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchReport {
    pub report_version: u32,
    pub suite: String,
    pub suite_version: String,
    pub adapter: String,
    /// BIRD's cleaned dev split: reports are marked "New Dev".
    pub new_dev: bool,
    pub cell: String,
    pub config: ExperimentConfig,
    pub config_hash: String,
    pub prompt_hash: String,
    pub model: String,
    pub split: Split,
    pub question_ids_hash: String,
    /// The questions the run was asked to answer; `n` is how many it did.
    pub planned: usize,
    pub n: usize,
    /// The run was on the tuning split: its result may shape the configuration.
    pub tuned_on: bool,
    /// Set when a hold-out result could have shaped this configuration (see [`contamination`]).
    pub contaminated: Option<String>,
    pub few_shot_used: usize,
    /// Whether the benchmark's evidence ("oracle knowledge") was in the prompt.
    pub oracle_knowledge: bool,
    pub ledger_fingerprints: BTreeMap<String, String>,
    pub budget_stopped: Option<String>,
    pub started_at: String,
    pub finished_at: String,
    pub metrics: Metrics,
    pub questions: Vec<QuestionResult>,
}

fn mean(xs: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, n) = xs.fold((0.0, 0usize), |(s, n), x| (s + x, n + 1));
    (n > 0).then(|| sum / n as f64)
}

/// The run-level numbers of `results`.
pub fn metrics(results: &[QuestionResult], price: Option<(f64, f64)>) -> Metrics {
    let pct = |xs: Vec<f64>| {
        let i = stats::bootstrap_mean(&xs);
        Interval {
            n: i.n,
            mean: i.mean * 100.0,
            low: i.low * 100.0,
            high: i.high * 100.0,
        }
    };
    let ex = pct(results.iter().map(|r| r.score.ex as u8 as f64).collect());
    let soft_f1 = pct(results.iter().map(|r| r.score.soft_f1).collect());
    let rves = super::sqlexec::rves(
        &results
            .iter()
            .map(|r| r.score.rves_reward)
            .collect::<Vec<_>>(),
    );
    let rate = |f: &dyn Fn(&QuestionResult) -> bool| {
        mean(results.iter().map(|r| f(r) as u8 as f64)).unwrap_or(0.0) * 100.0
    };
    let mut by_difficulty: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut by_database: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for r in results {
        for (map, key) in [
            (&mut by_difficulty, &r.difficulty),
            (&mut by_database, &r.db_id),
        ] {
            let e = map.entry(key.clone()).or_default();
            e.0 += 1;
            e.1 += r.score.ex as usize;
        }
    }
    let breakdown = |m: BTreeMap<String, (usize, usize)>| {
        m.into_iter()
            .map(|(k, (n, ok))| {
                (
                    k,
                    Breakdown {
                        n,
                        ex: ok as f64 * 100.0 / n as f64,
                    },
                )
            })
            .collect()
    };
    let input_tokens = results.iter().map(|r| r.prediction.input_tokens).sum();
    let output_tokens = results.iter().map(|r| r.prediction.output_tokens).sum();
    let billed_input_tokens: u64 = results
        .iter()
        .map(|r| r.prediction.billed_input_tokens)
        .sum();
    let billed_output_tokens: u64 = results
        .iter()
        .map(|r| r.prediction.billed_output_tokens)
        .sum();
    // Cost is what reached the provider; a cache-served call is free.
    let cost_usd = price
        .map(|(i, o)| (billed_input_tokens as f64 * i + billed_output_tokens as f64 * o) / 1e6)
        .unwrap_or(0.0);
    let mut categories: BTreeMap<String, usize> = BTreeMap::new();
    for r in results {
        *categories
            .entry(r.score.category.as_str().to_string())
            .or_default() += 1;
    }
    let mut latencies: Vec<u64> = results.iter().map(|r| r.prediction.latency_ms).collect();
    latencies.sort_unstable();
    let p95 = if latencies.is_empty() {
        0
    } else {
        latencies[((latencies.len() as f64 * 0.95).ceil() as usize).clamp(1, latencies.len()) - 1]
    };
    Metrics {
        ex,
        soft_f1,
        rves,
        grounding_rate: rate(&|r| r.score.grounded),
        executed_rate: rate(&|r| r.score.executed),
        schema_recall: mean(results.iter().filter_map(|r| r.score.schema_recall))
            .map(|x| x * 100.0),
        join_recall: mean(results.iter().filter_map(|r| r.score.join_recall)).map(|x| x * 100.0),
        value_hit_rate: mean(results.iter().filter_map(|r| r.score.value_hit)).map(|x| x * 100.0),
        by_difficulty: breakdown(by_difficulty),
        by_database: breakdown(by_database),
        input_tokens,
        output_tokens,
        cost_usd,
        cost_known: price.is_some(),
        latency_mean_ms: mean(latencies.iter().map(|l| *l as f64)).unwrap_or(0.0),
        latency_p95_ms: p95,
        categories,
        truncated: results.iter().filter(|r| r.prediction.truncated).count(),
        calls: results.iter().map(|r| r.prediction.calls).sum(),
        cached_calls: results.iter().map(|r| r.prediction.cached_calls).sum(),
        billed_input_tokens,
        billed_output_tokens,
        turns: results.iter().map(|r| r.prediction.turns).sum(),
        open_findings: finding_counts(
            results
                .iter()
                .map(|r| (&r.prediction.open_findings, r.score.ex)),
        ),
    }
}

/// Per finding code: answers it was raised on that were (correct, wrong); a code counts once per
/// answer.
pub fn finding_counts<'a>(
    answers: impl Iterator<Item = (&'a Vec<String>, bool)>,
) -> BTreeMap<String, (usize, usize)> {
    let mut out: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (codes, ex) in answers {
        let unique: std::collections::BTreeSet<&String> = codes.iter().collect();
        for c in unique {
            let e = out.entry(c.clone()).or_default();
            if ex {
                e.0 += 1;
            } else {
                e.1 += 1;
            }
        }
    }
    out
}

/// The per-code table: `L004 right 3 · wrong 9 (75% of its answers wrong)`.
pub fn render_finding_counts(counts: &BTreeMap<String, (usize, usize)>) -> String {
    counts
        .iter()
        .map(|(code, (right, wrong))| {
            let share = 100.0 * *wrong as f64 / (*right + *wrong).max(1) as f64;
            format!("{code} right {right} · wrong {wrong} ({share:.0}% wrong)")
        })
        .collect::<Vec<_>>()
        .join("\n               ")
}

/// Everything about a run that is not its per-question results.
pub struct RunMeta<'a> {
    pub suite: &'a super::suite::Suite,
    pub config: &'a ExperimentConfig,
    pub split: Split,
    /// The questions the run was asked to answer.
    pub planned: &'a [super::BenchScenario],
    /// The hash of the suite's hold-out question ids, for the contamination check.
    pub hold_ids_hash: &'a str,
    pub model: String,
    pub ledger_fingerprints: BTreeMap<String, String>,
    pub started_at: String,
    pub finished_at: String,
    pub price: Option<(f64, f64)>,
    /// Reports already saved for this suite.
    pub earlier: &'a [(PathBuf, BenchReport)],
}

impl BenchReport {
    pub fn assemble(meta: RunMeta<'_>, outcome: super::RunOutcome) -> BenchReport {
        let config_hash = meta.config.hash();
        let question_ids_hash = super::split::ids_hash(meta.planned);
        let prompt_hash = outcome
            .results
            .iter()
            .map(|r| r.prediction.prompt_hash.clone())
            .find(|h| !h.is_empty())
            .unwrap_or_default();
        BenchReport {
            report_version: REPORT_VERSION,
            suite: meta.suite.name.clone(),
            suite_version: meta.suite.version.clone(),
            adapter: meta.suite.adapter.clone(),
            new_dev: meta.suite.new_dev,
            cell: meta.config.name.clone(),
            config: meta.config.clone(),
            prompt_hash,
            model: meta.model,
            split: meta.split,
            planned: meta.planned.len(),
            n: outcome.results.len(),
            tuned_on: meta.split == Split::Tune,
            contaminated: contamination(
                meta.earlier,
                &meta.suite.name,
                &meta.config.name,
                meta.split,
                meta.hold_ids_hash,
                &config_hash,
            ),
            few_shot_used: meta.config.few_shot,
            oracle_knowledge: meta.config.evidence,
            ledger_fingerprints: meta.ledger_fingerprints,
            budget_stopped: outcome.budget_stopped,
            started_at: meta.started_at,
            finished_at: meta.finished_at,
            metrics: metrics(&outcome.results, meta.price),
            questions: outcome.results,
            config_hash,
            question_ids_hash,
        }
    }

    pub fn save(&self, dir: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let stamp = self.started_at.replace([':', '-'], "");
        let stamp = stamp.split('.').next().unwrap_or(&stamp).to_string();
        let path = dir.join(format!(
            "{stamp}-{}-{}.json",
            self.cell.to_lowercase(),
            self.split
        ));
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(path)
    }

    pub fn load(path: &Path) -> Result<BenchReport> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Plain-text summary, as `ekos eval bench run` prints it.
    pub fn render(&self) -> String {
        let m = &self.metrics;
        let mut out = String::new();
        let new_dev = if self.new_dev { " (New Dev)" } else { "" };
        out.push_str(&format!(
            "EKOS BENCHMARK — {} {}{new_dev} · cell {} · split {} · {}\n",
            self.suite, self.suite_version, self.cell, self.split, self.model
        ));
        out.push_str(&format!("{}\n", "─".repeat(72)));
        out.push_str(&format!(
            "n = {} of {} planned{}\n",
            self.n,
            self.planned,
            match &self.budget_stopped {
                Some(why) => format!(" — STOPPED EARLY by the budget: {why}"),
                None => String::new(),
            }
        ));
        out.push_str(&format!(
            "EX        {:6.2}%   95% CI [{:.2}, {:.2}]\n",
            m.ex.mean, m.ex.low, m.ex.high
        ));
        out.push_str(&format!(
            "Soft-F1   {:6.2}%   95% CI [{:.2}, {:.2}]\n",
            m.soft_f1.mean, m.soft_f1.low, m.soft_f1.high
        ));
        out.push_str(&format!("R-VES     {:6.2}\n", m.rves));
        out.push_str(&format!(
            "Grounded  {:6.2}%   executed {:.2}%\n",
            m.grounding_rate, m.executed_rate
        ));
        let opt = |v: Option<f64>| v.map_or("n/a".to_string(), |v| format!("{v:.2}%"));
        out.push_str(&format!(
            "Schema-linking recall {} · join-path recall {} · value-hit rate {}\n",
            opt(m.schema_recall),
            opt(m.join_recall),
            opt(m.value_hit_rate)
        ));
        out.push_str("By difficulty: ");
        out.push_str(
            &m.by_difficulty
                .iter()
                .map(|(k, b)| format!("{k} {:.1}% (n={})", b.ex, b.n))
                .collect::<Vec<_>>()
                .join(" · "),
        );
        out.push('\n');
        out.push_str("By database:   ");
        out.push_str(
            &m.by_database
                .iter()
                .map(|(k, b)| format!("{k} {:.1}% (n={})", b.ex, b.n))
                .collect::<Vec<_>>()
                .join(" · "),
        );
        out.push('\n');
        if !m.categories.is_empty() {
            out.push_str("Outcomes:      ");
            out.push_str(
                &m.categories
                    .iter()
                    .map(|(k, n)| format!("{k} {n}"))
                    .collect::<Vec<_>>()
                    .join(" · "),
            );
            if m.truncated > 0 {
                out.push_str(&format!(
                    " ({} of the no-SQL answers hit the output cap)",
                    m.truncated
                ));
            }
            out.push('\n');
        }
        if m.turns > 0 {
            out.push_str(&format!("Replies:       {} in total\n", m.turns));
        }
        if !m.open_findings.is_empty() {
            out.push_str(&format!(
                "Open findings: {}\n",
                render_finding_counts(&m.open_findings)
            ));
        }
        out.push_str(&format!(
            "Calls {} ({} from the LLM cache) · billed tokens {} in / {} out\n",
            m.calls, m.cached_calls, m.billed_input_tokens, m.billed_output_tokens
        ));
        out.push_str(&format!(
            "Tokens {} in / {} out · cost {} · latency mean {:.0} ms, P95 {} ms\n",
            m.input_tokens,
            m.output_tokens,
            if m.cost_known {
                format!("${:.4}", m.cost_usd)
            } else {
                "unknown (no price for this model)".into()
            },
            m.latency_mean_ms,
            m.latency_p95_ms
        ));
        out.push_str(&format!(
            "Config {} · prompt {} · questions {} · oracle knowledge {} · few-shot {}\n",
            &self.config_hash[..12],
            &self.prompt_hash[..12.min(self.prompt_hash.len())],
            &self.question_ids_hash[..12],
            if self.oracle_knowledge { "yes" } else { "no" },
            self.few_shot_used
        ));
        if let Some(why) = &self.contaminated {
            out.push_str(&format!("CONTAMINATED: {why}\n"));
        }
        out
    }
}

/// Every bench report under `dir` (recursively), oldest first.
pub fn load_all(dir: &Path) -> Vec<(PathBuf, BenchReport)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "json")
                && let Ok(r) = BenchReport::load(&p)
            {
                out.push((p, r));
            }
        }
    }
    out.sort_by(|a, b| a.1.started_at.cmp(&b.1.started_at));
    out
}

/// RFC 0240 §2.3 item 5: a run of a cell on the hold-out or full set is contaminated when an
/// earlier report ran the *same cell* on the same hold-out questions with a *different*
/// configuration — the configuration changed after its hold-out result was seen. Advisory: the
/// run is marked and the mark printed, never refused (§6).
pub fn contamination(
    earlier: &[(PathBuf, BenchReport)],
    suite: &str,
    cell: &str,
    split: Split,
    hold_ids_hash: &str,
    config_hash: &str,
) -> Option<String> {
    if split == Split::Tune {
        return None;
    }
    earlier
        .iter()
        .find(|(_, r)| {
            r.suite == suite
                && r.cell == cell
                && r.split == Split::Hold
                && r.question_ids_hash == hold_ids_hash
                && r.config_hash != config_hash
        })
        .map(|(p, r)| {
            format!(
                "cell {cell} ran on the hold-out split on {} with configuration {} ({}); this \
                 run's configuration {} differs, so it may have been tuned on hold-out results",
                r.started_at,
                &r.config_hash[..12],
                p.display(),
                &config_hash[..12]
            )
        })
}

/// A paired comparison of two runs (RFC 0240 §2.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    pub a: String,
    pub b: String,
    /// Questions both runs answered.
    pub paired: usize,
    pub ex_a: f64,
    pub ex_b: f64,
    /// EX(A) − EX(B) in points over the paired questions, with its paired-bootstrap interval.
    pub diff: Interval,
    pub mcnemar_p: f64,
    /// The interval excludes zero.
    pub significant: bool,
    pub only_a_correct: Vec<String>,
    pub only_b_correct: Vec<String>,
    /// RFC 0241: `(category in A, category in B)` → questions, where they differ.
    #[serde(default)]
    pub category_moves: BTreeMap<String, usize>,
    pub warnings: Vec<String>,
}

pub fn compare(a: &BenchReport, b: &BenchReport) -> Comparison {
    let b_by_id: BTreeMap<&str, &QuestionResult> =
        b.questions.iter().map(|q| (q.id.as_str(), q)).collect();
    let mut diffs = Vec::new();
    let (mut only_a, mut only_b) = (Vec::new(), Vec::new());
    let (mut ok_a, mut ok_b) = (0usize, 0usize);
    let mut moves: BTreeMap<String, usize> = BTreeMap::new();
    for qa in &a.questions {
        let Some(qb) = b_by_id.get(qa.id.as_str()) else {
            continue;
        };
        if qa.score.category != qb.score.category {
            *moves
                .entry(format!(
                    "{} → {}",
                    qb.score.category.as_str(),
                    qa.score.category.as_str()
                ))
                .or_default() += 1;
        }
        let (x, y) = (qa.score.ex, qb.score.ex);
        ok_a += x as usize;
        ok_b += y as usize;
        diffs.push(x as u8 as f64 - y as u8 as f64);
        match (x, y) {
            (true, false) => only_a.push(qa.id.clone()),
            (false, true) => only_b.push(qa.id.clone()),
            _ => {}
        }
    }
    let i = stats::bootstrap_mean(&diffs);
    let diff = Interval {
        n: i.n,
        mean: i.mean * 100.0,
        low: i.low * 100.0,
        high: i.high * 100.0,
    };
    let paired = diffs.len();
    let mut warnings = Vec::new();
    if a.suite != b.suite {
        warnings.push(format!("different suites: {} vs {}", a.suite, b.suite));
    }
    if a.model != b.model {
        warnings.push(format!(
            "different models: {} vs {} — the cells are meant to hold the model fixed",
            a.model, b.model
        ));
    }
    for r in [a, b] {
        if r.budget_stopped.is_some() {
            warnings.push(format!("{} stopped early by its budget", r.cell));
        }
        if let Some(why) = &r.contaminated {
            warnings.push(format!("{} is marked contaminated: {why}", r.cell));
        }
    }
    if paired < a.n.max(b.n) {
        warnings.push(format!(
            "only {paired} questions are in both runs ({} and {} answered)",
            a.n, b.n
        ));
    }
    let pct = |ok: usize| {
        if paired == 0 {
            0.0
        } else {
            ok as f64 * 100.0 / paired as f64
        }
    };
    Comparison {
        a: a.cell.clone(),
        b: b.cell.clone(),
        paired,
        ex_a: pct(ok_a),
        ex_b: pct(ok_b),
        significant: paired > 0 && diff.excludes_zero(),
        diff,
        mcnemar_p: stats::mcnemar_exact(only_a.len(), only_b.len()),
        only_a_correct: only_a,
        only_b_correct: only_b,
        category_moves: moves,
        warnings,
    }
}

impl Comparison {
    pub fn render(&self) -> String {
        let mut out = format!(
            "{} vs {} — {} paired questions\n  EX {:.2}% vs {:.2}%  →  difference {:+.2} points, \
             95% CI [{:+.2}, {:+.2}], McNemar p = {:.4}\n  {}\n",
            self.a,
            self.b,
            self.paired,
            self.ex_a,
            self.ex_b,
            self.diff.mean,
            self.diff.low,
            self.diff.high,
            self.mcnemar_p,
            if self.significant {
                "A difference: the interval excludes zero."
            } else {
                "No difference shown: the interval includes zero."
            }
        );
        let list = |ids: &[String]| {
            if ids.is_empty() {
                "none".to_string()
            } else {
                ids.join(", ")
            }
        };
        out.push_str(&format!(
            "  Only {} right ({}): {}\n  Only {} right ({}): {}\n",
            self.a,
            self.only_a_correct.len(),
            list(&self.only_a_correct),
            self.b,
            self.only_b_correct.len(),
            list(&self.only_b_correct)
        ));
        if !self.category_moves.is_empty() {
            let mut moves: Vec<(&String, &usize)> = self.category_moves.iter().collect();
            moves.sort_by(|x, y| y.1.cmp(x.1).then(x.0.cmp(y.0)));
            out.push_str(&format!("  Outcome changes ({} → {}): ", self.b, self.a));
            out.push_str(
                &moves
                    .iter()
                    .map(|(k, n)| format!("{k} {n}"))
                    .collect::<Vec<_>>()
                    .join(" · "),
            );
            out.push('\n');
        }
        for w in &self.warnings {
            out.push_str(&format!("  warning: {w}\n"));
        }
        out
    }
}
