//! RFC 0159 — target design: engine, `ORDER BY`, partitioning, codecs.
//!
//! Every choice here is derived from evidence and carries it. "Why is this table ordered that way"
//! has to be answerable a year later by somebody who was not there, and the only way that works is
//! if the answer is a fact rather than a memory.
//!
//! Where there is **no** evidence, the design says so and proposes the primary key with an explicit
//! note — rather than presenting a guess as a derivation, which is the same failure as a validation
//! tier reporting green with no controls.

use serde::{Deserialize, Serialize};

/// Which ClickHouse table engine, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "engine")]
pub enum Engine {
    /// Append-only.
    MergeTree,
    /// The source updates rows in place. **Dedup is eventual**: a `SELECT` before a merge still
    /// returns duplicates, which surprises people badly and changes how RFC 0156 must read the
    /// table.
    ReplacingMergeTree { version_column: Option<String> },
}

impl Engine {
    pub fn render(&self) -> String {
        match self {
            Self::MergeTree => "MergeTree".into(),
            Self::ReplacingMergeTree {
                version_column: Some(v),
            } => format!("ReplacingMergeTree({v})"),
            Self::ReplacingMergeTree {
                version_column: None,
            } => "ReplacingMergeTree".into(),
        }
    }
}

/// What is known about the source table when designing its target.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableEvidence {
    pub row_count: i64,
    /// `n_tup_upd + n_tup_del > 0` — the source updates rows in place.
    pub has_updates: bool,
    /// A column that advances on every update, for `ReplacingMergeTree`'s version. Absent is a
    /// finding, never an invented `now()`.
    pub update_time_column: Option<String>,
    pub primary_key: Vec<String>,
    /// Columns appearing in `WHERE`/`JOIN` predicates, most-frequent first, with how many calls
    /// each was seen in. From `pg_stat_statements` and from the compiled CKM.
    pub filter_columns: Vec<(String, u64)>,
    /// A low-cardinality time column, if any, and its approximate distinct months.
    pub time_column: Option<(String, u64)>,
}

/// One column, as the design sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct DesignColumn {
    pub name: String,
    pub target_type: String,
    pub distinct: Option<f64>,
    /// Monotonic over the sample — a `Delta` codec candidate.
    pub monotonic: Option<bool>,
}

/// A complete target design, with the evidence for each decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetDesign {
    pub table: String,
    pub engine: Engine,
    pub engine_rationale: String,
    pub order_by: Vec<String>,
    pub order_by_rationale: String,
    pub partition_by: Option<String>,
    pub partition_rationale: String,
    /// `column -> codec`.
    pub codecs: Vec<(String, String)>,
    /// Decisions that need a human before this design is used.
    pub findings: Vec<String>,
}

/// Over-partitioning is the most common self-inflicted ClickHouse wound, so a projected partition
/// count above this is refused rather than emitted.
pub const MAX_PARTITIONS: u64 = 1000;

/// Below this, partitioning costs more than it saves.
pub const MIN_ROWS_TO_PARTITION: i64 = 10_000_000;

/// Choose an engine.
pub fn choose_engine(ev: &TableEvidence) -> (Engine, String, Vec<String>) {
    let mut findings = Vec::new();
    if !ev.has_updates {
        return (
            Engine::MergeTree,
            "no updates or deletes recorded against the source, so the table is append-only".into(),
            findings,
        );
    }
    match &ev.update_time_column {
        Some(v) => (
            Engine::ReplacingMergeTree {
                version_column: Some(v.clone()),
            },
            format!(
                "the source updates rows in place; {v} advances on update and serves as the version"
            ),
            {
                findings.push(
                    "ReplacingMergeTree deduplicates only eventually — a SELECT before a merge \
                     still returns duplicates. Validation must read dedup-aware (RFC 0156), and \
                     anything reading this table directly must use FINAL or accept duplicates."
                        .into(),
                );
                findings
            },
        ),
        None => (
            Engine::ReplacingMergeTree {
                version_column: None,
            },
            "the source updates rows in place, but no column advances on update".into(),
            {
                findings.push(
                    "No version column: without one ClickHouse keeps an arbitrary row per key on \
                     merge. Inventing a now() would make the load non-deterministic and \
                     unvalidatable, so this needs a decision — add an updated_at to the source, or \
                     accept last-writer-wins by insertion order."
                        .into(),
                );
                findings
            },
        ),
    }
}

/// Derive `ORDER BY`.
///
/// Priority: observed filter predicates first, most-frequent first, then the primary key for
/// uniqueness. The PK is *last* on purpose — in ClickHouse it is frequently the **worst** ordering,
/// because it is unique and therefore useless for skipping granules on the filters people actually
/// run.
pub fn choose_order_by(ev: &TableEvidence, columns: &[DesignColumn]) -> (Vec<String>, String) {
    let known: std::collections::BTreeSet<&str> = columns.iter().map(|c| c.name.as_str()).collect();

    let mut ranked: Vec<(String, u64)> = ev
        .filter_columns
        .iter()
        .filter(|(c, _)| known.contains(c.as_str()))
        .cloned()
        .collect();
    // Frequency first; then lowest cardinality, because a low-distinct leading column skips more.
    ranked.sort_by(|a, b| {
        b.1.cmp(&a.1).then_with(|| {
            let d = |n: &str| {
                columns
                    .iter()
                    .find(|c| c.name == n)
                    .and_then(|c| c.distinct)
                    .unwrap_or(f64::MAX)
            };
            d(&a.0)
                .partial_cmp(&d(&b.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });

    let mut out: Vec<String> = Vec::new();
    for (c, _) in ranked.iter().take(3) {
        if !out.contains(c) {
            out.push(c.clone());
        }
    }

    if out.is_empty() {
        let pk: Vec<String> = ev
            .primary_key
            .iter()
            .filter(|c| known.contains(c.as_str()))
            .cloned()
            .collect();
        return (
            pk,
            "no query-shape evidence available for this table, so this is the primary key rather \
             than a derivation. Populate pg_stat_statements, or compile the application's queries, \
             and re-map before relying on it."
                .into(),
        );
    }

    let rationale = format!(
        "derived from observed filter predicates ({}); the primary key follows for uniqueness",
        ranked
            .iter()
            .take(3)
            .map(|(c, n)| format!("{c} in {n} call(s)"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    for c in &ev.primary_key {
        if known.contains(c.as_str()) && !out.contains(c) {
            out.push(c.clone());
        }
    }
    (out, rationale)
}

/// Choose partitioning, or refuse.
pub fn choose_partition(ev: &TableEvidence) -> (Option<String>, String, Vec<String>) {
    let mut findings = Vec::new();
    let Some((col, months)) = &ev.time_column else {
        return (
            None,
            "no low-cardinality time column; an unpartitioned table is the right default".into(),
            findings,
        );
    };
    if ev.row_count < MIN_ROWS_TO_PARTITION {
        return (
            None,
            format!(
                "{} rows is below the {MIN_ROWS_TO_PARTITION}-row threshold where partitioning \
                 pays for itself",
                ev.row_count
            ),
            findings,
        );
    }
    if *months > MAX_PARTITIONS {
        findings.push(format!(
            "partitioning by month of {col} projects {months} partitions, above the \
             {MAX_PARTITIONS} limit. Over-partitioning is the most common self-inflicted \
             ClickHouse wound, so no partitioning is proposed — widen the granularity (by year) or \
             accept an unpartitioned table."
        ));
        return (
            None,
            format!("refused: {months} projected partitions exceeds {MAX_PARTITIONS}"),
            findings,
        );
    }
    (
        Some(format!("toYYYYMM({col})")),
        format!(
            "{} rows with a time column spanning about {months} months — within the partition \
             budget",
            ev.row_count
        ),
        findings,
    )
}

/// Pick codecs from what the profile measured.
pub fn choose_codecs(columns: &[DesignColumn]) -> Vec<(String, String)> {
    columns
        .iter()
        .filter_map(|c| {
            let t = c.target_type.as_str();
            let codec = if c.monotonic == Some(true)
                && (t.contains("Int") || t.contains("DateTime") || t.contains("Date"))
            {
                // A monotonic sequence compresses enormously as deltas.
                "CODEC(Delta, ZSTD(1))"
            } else if t.contains("String") {
                "CODEC(ZSTD(1))"
            } else {
                return None;
            };
            Some((c.name.clone(), codec.to_string()))
        })
        .collect()
}

/// Assemble a full design.
pub fn design(table: &str, ev: &TableEvidence, columns: &[DesignColumn]) -> TargetDesign {
    let (engine, engine_rationale, mut findings) = choose_engine(ev);
    let (order_by, order_by_rationale) = choose_order_by(ev, columns);
    let (partition_by, partition_rationale, partition_findings) = choose_partition(ev);
    findings.extend(partition_findings);

    if order_by.is_empty() {
        findings.push(format!(
            "{table} has no primary key and no observed filter predicates, so there is nothing to \
             order by. ClickHouse requires an ORDER BY; choose one before generating DDL."
        ));
    }

    TargetDesign {
        table: table.to_string(),
        engine,
        engine_rationale,
        order_by,
        order_by_rationale,
        partition_by,
        partition_rationale,
        codecs: choose_codecs(columns),
        findings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str, distinct: Option<f64>) -> DesignColumn {
        DesignColumn {
            name: name.into(),
            target_type: ty.into(),
            distinct,
            monotonic: None,
        }
    }

    #[test]
    fn an_append_only_table_gets_plain_mergetree() {
        let (e, why, findings) = choose_engine(&TableEvidence::default());
        assert_eq!(e, Engine::MergeTree);
        assert!(why.contains("append-only"));
        assert!(findings.is_empty());
    }

    /// Choosing ReplacingMergeTree always emits the eventual-dedup finding: it is invisible on load
    /// day and changes how the table must be read and validated forever after.
    #[test]
    fn replacing_mergetree_always_warns_about_eventual_dedup() {
        let ev = TableEvidence {
            has_updates: true,
            update_time_column: Some("updated_at".into()),
            ..Default::default()
        };
        let (e, _, findings) = choose_engine(&ev);
        assert_eq!(
            e,
            Engine::ReplacingMergeTree {
                version_column: Some("updated_at".into())
            }
        );
        assert_eq!(e.render(), "ReplacingMergeTree(updated_at)");
        assert!(
            findings.iter().any(|f| f.contains("eventually")),
            "{findings:?}"
        );
    }

    /// No version column is a decision, not a default. Inventing `now()` would make the load
    /// non-deterministic and therefore unvalidatable.
    #[test]
    fn a_missing_version_column_is_a_finding_not_an_invented_now() {
        let ev = TableEvidence {
            has_updates: true,
            update_time_column: None,
            ..Default::default()
        };
        let (_, _, findings) = choose_engine(&ev);
        assert!(findings.iter().any(|f| f.contains("now()")), "{findings:?}");
    }

    /// The PK is frequently the worst ORDER BY in ClickHouse: unique, therefore useless for
    /// skipping granules on the filters people actually run.
    #[test]
    fn order_by_leads_with_observed_filters_not_the_primary_key() {
        let ev = TableEvidence {
            primary_key: vec!["id".into()],
            filter_columns: vec![("tenant_id".into(), 900), ("created_at".into(), 400)],
            ..Default::default()
        };
        let cols = [
            col("id", "Int64", Some(1_000_000.0)),
            col("tenant_id", "Int64", Some(40.0)),
            col("created_at", "DateTime64(6)", Some(500_000.0)),
        ];
        let (order, why) = choose_order_by(&ev, &cols);
        assert_eq!(order, vec!["tenant_id", "created_at", "id"]);
        assert!(why.contains("900 call(s)"), "{why}");
    }

    /// The honest case. A guess presented as a derivation is the same failure as a tier reporting
    /// green with no controls.
    #[test]
    fn with_no_query_evidence_the_design_says_so() {
        let ev = TableEvidence {
            primary_key: vec!["id".into()],
            ..Default::default()
        };
        let (order, why) = choose_order_by(&ev, &[col("id", "Int64", None)]);
        assert_eq!(order, vec!["id"]);
        assert!(
            why.contains("no query-shape evidence"),
            "the design must not present a guess as a derivation: {why}"
        );
    }

    #[test]
    fn a_filter_column_that_does_not_exist_is_ignored() {
        let ev = TableEvidence {
            primary_key: vec!["id".into()],
            filter_columns: vec![("dropped_column".into(), 900)],
            ..Default::default()
        };
        let (order, _) = choose_order_by(&ev, &[col("id", "Int64", None)]);
        assert_eq!(order, vec!["id"]);
    }

    #[test]
    fn a_small_table_is_not_partitioned() {
        let ev = TableEvidence {
            row_count: 50_000,
            time_column: Some(("created_at".into(), 24)),
            ..Default::default()
        };
        let (p, why, _) = choose_partition(&ev);
        assert_eq!(p, None);
        assert!(why.contains("threshold"), "{why}");
    }

    #[test]
    fn a_large_table_with_a_time_column_is_partitioned_by_month() {
        let ev = TableEvidence {
            row_count: 500_000_000,
            time_column: Some(("created_at".into(), 36)),
            ..Default::default()
        };
        let (p, _, findings) = choose_partition(&ev);
        assert_eq!(p.as_deref(), Some("toYYYYMM(created_at)"));
        assert!(findings.is_empty());
    }

    /// Over-partitioning is refused with a reason, not emitted and regretted.
    #[test]
    fn a_projected_partition_explosion_is_refused() {
        let ev = TableEvidence {
            row_count: 500_000_000,
            time_column: Some(("event_time".into(), 5_000)),
            ..Default::default()
        };
        let (p, why, findings) = choose_partition(&ev);
        assert_eq!(p, None);
        assert!(why.contains("refused"), "{why}");
        assert!(
            findings.iter().any(|f| f.contains("5000 partitions")),
            "{findings:?}"
        );
    }

    #[test]
    fn monotonic_integers_get_a_delta_codec() {
        let cols = [
            DesignColumn {
                monotonic: Some(true),
                ..col("id", "Int64", None)
            },
            DesignColumn {
                monotonic: Some(false),
                ..col("amount", "Decimal64(2)", None)
            },
            col("name", "String", None),
        ];
        let codecs = choose_codecs(&cols);
        assert_eq!(codecs[0], ("id".into(), "CODEC(Delta, ZSTD(1))".into()));
        assert_eq!(codecs[1], ("name".into(), "CODEC(ZSTD(1))".into()));
        assert_eq!(
            codecs.len(),
            2,
            "an unmeasured numeric gets no codec: {codecs:?}"
        );
    }

    #[test]
    fn a_table_with_nothing_to_order_by_is_a_finding() {
        let d = design(
            "s.t",
            &TableEvidence::default(),
            &[col("x", "String", None)],
        );
        assert!(d.order_by.is_empty());
        assert!(
            d.findings.iter().any(|f| f.contains("nothing to order by")),
            "{:?}",
            d.findings
        );
    }
}
