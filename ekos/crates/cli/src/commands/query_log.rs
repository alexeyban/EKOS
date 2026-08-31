//! Query/tool-call usage log + a pre-execution heuristic cost classifier (RFC 0114) — groundwork
//! for RFC 0080's storage-plan Phase 5 (materialized views), which explicitly needs "a pass over
//! real EKL/MCP query logs to find what's actually worth materializing" and had nothing to analyze:
//! the only real query log anywhere was RFC 0056's ClickHouse audit trail, scoped to that one
//! live-external-system tool. `ekos_ekl` and the other 13 read-only MCP tools had zero persisted
//! call history.
//!
//! This deliberately does **not** reuse RFC 0056's ledger-based Evidence/Event audit pattern for
//! the general case — see RFC 0114 §"Why not extend RFC 0056's ledger-based audit pattern
//! directly" for the two concrete reasons (evidence-semantics mismatch; a writable `FactLedger`
//! open per call would reintroduce the exact lock-contention/latency regression RFC 0097 fixed).
//! Usage telemetry instead gets its own append-only local file, entirely outside the ledger.
//!
//! The cost classifier is a caching *gate*, not a scoring system — it decides whether a call is
//! worth opportunistically caching before running it, using static thresholds on the tool's own
//! arguments. It doesn't have to be accurate for the logging half to be sound: every call's real
//! measured `duration_ms` is recorded regardless of what the heuristic guessed, and that measured
//! number — not `cost_class` — is what Phase 5's eventual analysis works from.

use serde::Serialize;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CostClass {
    Cheap,
    Expensive,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub ts: chrono::DateTime<chrono::Utc>,
    pub tool: String,
    pub cost_class: CostClass,
    pub reason: String,
    pub cache_hit: bool,
    pub result_count: Option<usize>,
    pub duration_ms: u128,
}

impl LogEntry {
    pub fn new(tool: impl Into<String>, cost_class: CostClass, reason: impl Into<String>) -> Self {
        Self {
            ts: chrono::Utc::now(),
            tool: tool.into(),
            cost_class,
            reason: reason.into(),
            cache_hit: false,
            result_count: None,
            duration_ms: 0,
        }
    }
}

/// Appends one JSON line to `<ekos_dir>/query-log.jsonl`. Best-effort by design at the call
/// sites (a logging failure must never fail the query it's describing) — this function itself
/// still surfaces the real `io::Error` so a caller can choose to ignore it.
///
/// A concurrent writer (another MCP server process, or `ekos ekl` running at the same instant)
/// could in principle interleave partial lines on some filesystems — accepted for v1, same
/// "accept the small edge case, document it" posture as RFC 0113's unsealed-segment loss window.
/// This is telemetry, not a correctness-bearing store: a corrupted log line is a bad log line,
/// never a bad answer.
pub fn record(ekos_dir: &Path, entry: &LogEntry) -> std::io::Result<()> {
    std::fs::create_dir_all(ekos_dir)?;
    let path = ekos_dir.join("query-log.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let mut line = serde_json::to_string(entry).expect("LogEntry is always representable as JSON");
    line.push('\n');
    file.write_all(line.as_bytes())
}

/// Heuristic classification for one EKL query, from its already-parsed AST. `Expensive` when the
/// query has no way to bound its own scan: no predicates and no `FROM` anchor (a full entity
/// scan), or no `LIMIT`/an unusually large one.
pub fn classify_ekl(ast: &ekos_ekl::EklAst) -> (CostClass, String) {
    const LIMIT_THRESHOLD: u64 = 500;

    if ast.predicates.is_empty() && ast.from.is_none() {
        return (CostClass::Expensive, "no predicates or FROM scope".into());
    }
    match ast.limit {
        None => (CostClass::Expensive, "no LIMIT".into()),
        Some(n) if n > LIMIT_THRESHOLD => (
            CostClass::Expensive,
            format!("LIMIT {n} > {LIMIT_THRESHOLD}"),
        ),
        Some(_) => (CostClass::Cheap, "filtered, bounded".into()),
    }
}

/// Heuristic classification for an MCP tool call from its raw JSON arguments — static thresholds
/// on the same parameters each handler already reads, evaluated *before* the handler runs.
///
/// `ekos_transformation_explain`/`diff` are deliberately always `Cheap` despite taking a
/// `max_hops` cap: it bounds a real chain walk that usually terminates on its own at a handful of
/// `Source` nodes long before the cap, so the parameter is a safety limit, not a proxy for actual
/// work — guessing expense from it would misclassify more often than not.
pub fn classify_tool(name: &str, args: &serde_json::Value) -> (CostClass, String) {
    match name {
        "ekos_neighborhood" => {
            let depth = args
                .get("depth")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1);
            if depth >= 3 {
                (CostClass::Expensive, format!("depth={depth}"))
            } else {
                (CostClass::Cheap, format!("depth={depth}"))
            }
        }
        "ekos_impact" => {
            let max_hops = args
                .get("max_hops")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(5);
            if max_hops >= 4 {
                (CostClass::Expensive, format!("max_hops={max_hops}"))
            } else {
                (CostClass::Cheap, format!("max_hops={max_hops}"))
            }
        }
        "ekos_diff" | "ekos_architecture_diff" => {
            let from = args
                .get("from")
                .and_then(serde_json::Value::as_str)
                .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());
            let to = args
                .get("to")
                .and_then(serde_json::Value::as_str)
                .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());
            match (from, to) {
                (Some(_), None) => (
                    CostClass::Expensive,
                    "no `to` (open-ended window to now)".into(),
                ),
                (Some(from), Some(to)) if (to - from) > chrono::Duration::days(7) => (
                    CostClass::Expensive,
                    format!("window {} days", (to - from).num_days()),
                ),
                _ => (CostClass::Cheap, "bounded window".into()),
            }
        }
        "ekos_architecture_evaluate" | "ekos_architecture_drift" => (
            CostClass::Expensive,
            "whole-workspace scan, no arguments to vary".into(),
        ),
        _ => (CostClass::Cheap, "not classified as expensive".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_ekl::ekl_parse;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn record_appends_rather_than_overwrites() {
        let dir = tempdir().unwrap();
        record(
            dir.path(),
            &LogEntry::new("ekos_search", CostClass::Cheap, "test"),
        )
        .unwrap();
        record(
            dir.path(),
            &LogEntry::new("ekos_impact", CostClass::Expensive, "test"),
        )
        .unwrap();

        let contents = std::fs::read_to_string(dir.path().join("query-log.jsonl")).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(first["tool"], "ekos_search");
        assert_eq!(first["cost_class"], "cheap");
        assert_eq!(second["tool"], "ekos_impact");
        assert_eq!(second["cost_class"], "expensive");
    }

    #[test]
    fn classify_ekl_full_scan_is_expensive() {
        let ast = ekl_parse("FIND Object").unwrap();
        assert_eq!(classify_ekl(&ast).0, CostClass::Expensive);
    }

    #[test]
    fn classify_ekl_filtered_and_bounded_is_cheap() {
        let ast = ekl_parse("FIND Object WHERE kind = 'Table' LIMIT 10").unwrap();
        assert_eq!(classify_ekl(&ast).0, CostClass::Cheap);
    }

    #[test]
    fn classify_ekl_filtered_but_unbounded_is_expensive() {
        let ast = ekl_parse("FIND Object WHERE kind = 'Table'").unwrap();
        assert_eq!(classify_ekl(&ast).0, CostClass::Expensive);
    }

    #[test]
    fn classify_ekl_huge_limit_is_expensive() {
        let ast = ekl_parse("FIND Object WHERE kind = 'Table' LIMIT 10000").unwrap();
        assert_eq!(classify_ekl(&ast).0, CostClass::Expensive);
    }

    #[test]
    fn classify_tool_neighborhood_shallow_is_cheap_deep_is_expensive() {
        assert_eq!(
            classify_tool("ekos_neighborhood", &json!({"id": "x", "depth": 1})).0,
            CostClass::Cheap
        );
        assert_eq!(
            classify_tool("ekos_neighborhood", &json!({"id": "x", "depth": 3})).0,
            CostClass::Expensive
        );
        // default (no `depth`) matches the handler's own default of 1 → Cheap
        assert_eq!(
            classify_tool("ekos_neighborhood", &json!({"id": "x"})).0,
            CostClass::Cheap
        );
    }

    #[test]
    fn classify_tool_impact_default_max_hops_is_expensive() {
        // the handler's own default is 5, which is already >= the threshold
        assert_eq!(
            classify_tool("ekos_impact", &json!({"id": "x"})).0,
            CostClass::Expensive
        );
        assert_eq!(
            classify_tool("ekos_impact", &json!({"id": "x", "max_hops": 2})).0,
            CostClass::Cheap
        );
    }

    #[test]
    fn classify_tool_diff_open_ended_or_wide_window_is_expensive() {
        assert_eq!(
            classify_tool("ekos_diff", &json!({"from": "2026-01-01T00:00:00Z"})).0,
            CostClass::Expensive,
            "no `to` — open-ended to now"
        );
        assert_eq!(
            classify_tool(
                "ekos_diff",
                &json!({"from": "2026-01-01T00:00:00Z", "to": "2026-02-15T00:00:00Z"})
            )
            .0,
            CostClass::Expensive,
            "> 7 day window"
        );
        assert_eq!(
            classify_tool(
                "ekos_diff",
                &json!({"from": "2026-01-01T00:00:00Z", "to": "2026-01-02T00:00:00Z"})
            )
            .0,
            CostClass::Cheap
        );
    }

    #[test]
    fn classify_tool_architecture_evaluate_and_drift_always_expensive() {
        assert_eq!(
            classify_tool("ekos_architecture_evaluate", &json!({})).0,
            CostClass::Expensive
        );
        assert_eq!(
            classify_tool("ekos_architecture_drift", &json!({})).0,
            CostClass::Expensive
        );
    }

    #[test]
    fn classify_tool_unlisted_tools_default_cheap() {
        for name in [
            "ekos_search",
            "ekos_state",
            "ekos_dependents",
            "ekos_status",
            "ekos_transformation_explain",
            "ekos_transformation_diff",
            "ekos_clickhouse_query",
        ] {
            assert_eq!(
                classify_tool(name, &json!({"max_hops": 999})).0,
                CostClass::Cheap,
                "{name} must not be classified expensive by this heuristic"
            );
        }
    }
}
