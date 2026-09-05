//! Run history (RFC 0138) — `ekos eval run` already saves a full `Report` as
//! `evals/reports/<timestamp>-<dataset>.json` on every run (see `crates/cli/src/commands/eval.rs`);
//! this module is the reader side: load every saved report back and render a trend table, so
//! "did this get better or worse" doesn't require opening each JSON file by hand.

use crate::report::Report;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("reading {0}: {1}")]
    Io(String, std::io::Error),
    #[error("parsing {0}: {1}")]
    Json(String, serde_json::Error),
}

/// Every saved report under `reports_dir`, oldest first (sorted by `generated_at`, not filename —
/// the timestamp in the filename and the one inside the JSON should always agree, but the JSON is
/// the source of truth). Skips non-`.json` files (e.g. `.gitkeep`) silently; a `.json` file that
/// fails to parse is a real error, not silently dropped.
pub fn load_all(reports_dir: &Path) -> Result<Vec<(PathBuf, Report)>, HistoryError> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(reports_dir)
        .map_err(|e| HistoryError::Io(reports_dir.display().to_string(), e))?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| HistoryError::Io(path.display().to_string(), e))?;
        let report: Report = serde_json::from_str(&text)
            .map_err(|e| HistoryError::Json(path.display().to_string(), e))?;
        out.push((path, report));
    }
    out.sort_by_key(|(_, r)| r.generated_at);
    Ok(out)
}

fn fmt_opt_pct(v: Option<f32>) -> String {
    v.map(|v| format!("{:.1}%", v * 100.0))
        .unwrap_or_else(|| "n/a".to_string())
}

/// A compact trend table — one row per saved run, newest last so it reads top-to-bottom as a
/// timeline. Deliberately plain-text/fixed-width rather than a third rendering format: same
/// philosophy as `report::render_text`.
pub fn render_table(runs: &[(PathBuf, Report)]) -> String {
    if runs.is_empty() {
        return "No saved runs found.\n".to_string();
    }
    let mut out = String::new();
    out.push_str(
        "Timestamp             Dataset          Agent            Status  Answer   Ground   Halluc   N\n",
    );
    out.push_str(
        "─────────────────────  ───────────────  ───────────────  ──────  ───────  ───────  ───────  ───\n",
    );
    for (_, r) in runs {
        let m = &r.metrics;
        out.push_str(&format!(
            "{:<22} {:<16} {:<16} {:<7} {:<8} {:<8} {:<8} {}\n",
            r.generated_at.format("%Y-%m-%d %H:%M:%S"),
            truncate(&r.dataset, 16),
            truncate(&r.agent, 16),
            if m.status_pass { "PASS" } else { "FAIL" },
            fmt_opt_pct(m.answer_correctness),
            fmt_opt_pct(m.evidence_groundedness),
            format!("{:.1}%", m.hallucination_rate * 100.0),
            m.scenarios,
        ));
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluators::EvalOutcome;
    use crate::report::GateThresholds;
    use crate::resource::ResourceDelta;
    use std::time::Duration;

    fn outcome(id: &str) -> EvalOutcome {
        EvalOutcome {
            scenario_id: id.into(),
            answer_score: Some(1.0),
            evidence_score: None,
            completeness_score: None,
            retrieval_recall: None,
            groundedness_score: None,
            trajectory_score: None,
            hallucinated: false,
            tokens: None,
            cache_hit: None,
            resource: ResourceDelta::default(),
            latency: Duration::from_millis(10),
            error: None,
            passed: true,
        }
    }

    #[test]
    fn load_all_reads_back_saved_reports_sorted_by_time() {
        let dir = tempfile::tempdir().unwrap();
        let older = crate::report::build(
            "d1",
            "claude",
            "local",
            &[outcome("a")],
            GateThresholds::default(),
        );
        let mut newer = crate::report::build(
            "d2",
            "claude",
            "local",
            &[outcome("b")],
            GateThresholds::default(),
        );
        newer.generated_at = older.generated_at + chrono::Duration::seconds(60);

        std::fs::write(
            dir.path().join("b-newer.json"),
            serde_json::to_string(&newer).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("a-older.json"),
            serde_json::to_string(&older).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.path().join(".gitkeep"), "").unwrap();

        let runs = load_all(dir.path()).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].1.dataset, "d1");
        assert_eq!(runs[1].1.dataset, "d2");
    }

    /// Real bug, found live 2026-09-05: a report saved before the cache/RSS/CPU fields existed
    /// (RFC 0138's own initial release) failed to parse at all once those fields were added
    /// without `#[serde(default)]` — `ekos eval history` errored on its very first real run
    /// against this repo's own `evals/reports/` directory. Fixed by defaulting every field added
    /// after the first release; this test is the regression guard so the next field addition
    /// doesn't silently reintroduce it.
    #[test]
    fn load_all_tolerates_a_report_saved_before_cache_and_resource_fields_existed() {
        let dir = tempfile::tempdir().unwrap();
        let pre_rfc0138_metrics_shape = serde_json::json!({
            "dataset": "old-run",
            "agent": "claude",
            "runtime": "local",
            "generated_at": "2026-09-05T14:00:00Z",
            "gates": {
                "min_answer_correctness": 0.85,
                "min_evidence_groundedness": 0.90,
                "min_completeness": 0.80,
                "min_recall_at_10": 0.80,
                "max_hallucination_rate": 0.05
            },
            "metrics": {
                "scenarios": 1,
                "passed": 1,
                "failed": 0,
                "answer_correctness": 1.0,
                "evidence_groundedness": null,
                "completeness": null,
                "recall_at_10": null,
                "hallucination_rate": 0.0,
                "avg_tokens": null,
                "p95_latency_ms": 10.0,
                "status_pass": true
                // no cache_hits/cache_misses/tokens_saved/peak_rss_kb/total_cpu_time_ms at all
            },
            "scenarios": []
        });
        std::fs::write(
            dir.path().join("old.json"),
            serde_json::to_string(&pre_rfc0138_metrics_shape).unwrap(),
        )
        .unwrap();

        let runs = load_all(dir.path()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].1.metrics.cache_hits, 0);
        assert_eq!(runs[0].1.metrics.tokens_saved, None);
    }

    #[test]
    fn render_table_on_empty_history_is_honest() {
        assert!(render_table(&[]).contains("No saved runs"));
    }
}
