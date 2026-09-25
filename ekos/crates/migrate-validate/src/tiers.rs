//! RFC 0156 — the validation tiers, and the rule that a tier which misses its control does not get
//! to report green.

use crate::dialect;
use crate::divergence::{Class, Divergence};
use crate::hash::BucketChecksum;
use crate::reader::{EngineReader, ReadError};

/// What a tier checks. Each is independently runnable; a passing higher tier does not make a lower
/// one redundant, because they fail for different reasons.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Shape: tables, columns, types, nullability, order, constraints, against the approved design.
    V0Structural,
    /// Row counts, per table and per chunk.
    V1Counts,
    /// Per-column aggregates over the canonical form.
    V2Aggregates,
    /// Order-independent bucketed row checksums.
    V3Checksums,
    /// The exact divergent keys and columns, by bisect from failed V3 buckets.
    V4RowDiff,
    /// Migrated logic produces the same results. RFC 0164.
    V5Logic,
    /// User-declared business invariants. RFC 0164+.
    V6Invariants,
}

impl Tier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V0Structural => "v0",
            Self::V1Counts => "v1",
            Self::V2Aggregates => "v2",
            Self::V3Checksums => "v3",
            Self::V4RowDiff => "v4",
            Self::V5Logic => "v5",
            Self::V6Invariants => "v6",
        }
    }

    /// Implemented in this crate today. V5/V6 are RFC 0164's, and calling them here is a bug rather
    /// than a silently empty pass.
    pub fn is_implemented(self) -> bool {
        self <= Self::V4RowDiff
    }
}

/// Whether a planted control fired. The field that decides whether a green result is allowed to be
/// reported as green.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ControlResult {
    pub control: String,
    pub tier: Tier,
    /// `true` = the tier detected the planted defect, as it must.
    pub detected: bool,
}

/// The outcome of running one tier over one unit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TierOutcome {
    pub tier: Tier,
    /// The read paths used, for the independent-oracle audit trail.
    pub source_path: String,
    pub target_path: String,
    pub divergences: Vec<Divergence>,
    pub controls: Vec<ControlResult>,
}

impl TierOutcome {
    /// Did the tier pass?
    ///
    /// **Two conditions, not one.** Zero blocking divergences *and* every control fired. A tier that
    /// found nothing because it cannot see anything is the failure mode this whole crate exists to
    /// prevent, so "no divergences" alone is never enough to report green.
    pub fn passed(&self) -> bool {
        crate::divergence::blocking_count(&self.divergences) == 0 && self.controls_all_fired()
    }

    pub fn controls_all_fired(&self) -> bool {
        self.controls.iter().all(|c| c.detected)
    }

    /// The controls that did not fire. Named, because "controls failed" is not actionable.
    pub fn missed_controls(&self) -> Vec<&str> {
        self.controls
            .iter()
            .filter(|c| !c.detected)
            .map(|c| c.control.as_str())
            .collect()
    }

    /// A one-line verdict that never says "passed" when a control was missed.
    pub fn verdict(&self) -> String {
        let blocking = crate::divergence::blocking_count(&self.divergences);
        if !self.controls_all_fired() {
            return format!(
                "{} FAILED — {} control(s) did not fire: {}. A tier that cannot catch a planted \
                 defect does not get to report green.",
                self.tier.as_str(),
                self.missed_controls().len(),
                self.missed_controls().join(", ")
            );
        }
        if blocking > 0 {
            return format!(
                "{} FAILED — {blocking} unexplained divergence(s)",
                self.tier.as_str()
            );
        }
        format!(
            "{} passed — {} control(s) fired",
            self.tier.as_str(),
            self.controls.len()
        )
    }
}

/// What a unit is validated against: the table on each side, its primary-key expression and its
/// column expressions, already rendered per dialect.
pub struct UnitPlan {
    pub unit: String,
    pub source_table: String,
    pub target_table: String,
    pub source_pk: String,
    pub target_pk: String,
    pub source_columns: Vec<String>,
    pub target_columns: Vec<String>,
    pub buckets: u32,
}

/// V1 — row counts.
pub fn run_v1(
    plan: &UnitPlan,
    source: &dyn EngineReader,
    target: &dyn EngineReader,
) -> Result<TierOutcome, ReadError> {
    let s = source.scalar_u64(&dialect::count_query(source.dialect(), &plan.source_table))?;
    let t = target.scalar_u64(&dialect::count_query(target.dialect(), &plan.target_table))?;
    let divergences = if s == t {
        vec![]
    } else {
        vec![Divergence {
            locus: format!("{}:count", plan.unit),
            columns: vec![],
            detail: format!(
                "source {s} rows, target {t} rows (delta {})",
                t as i64 - s as i64
            ),
            class: Class::Unexplained,
        }]
    };
    Ok(TierOutcome {
        tier: Tier::V1Counts,
        source_path: source.label().to_string(),
        target_path: target.label().to_string(),
        divergences,
        controls: vec![],
    })
}

/// V2 — per-column aggregates over the canonical form.
///
/// Reports the *column* that disagrees and which aggregate, never a value: a min or max is itself
/// row data.
pub fn run_v2(
    plan: &UnitPlan,
    source: &dyn EngineReader,
    target: &dyn EngineReader,
    column_names: &[String],
) -> Result<TierOutcome, ReadError> {
    let s = source.one_row(&dialect::aggregate_query(
        source.dialect(),
        &plan.source_table,
        &plan.source_columns,
    ))?;
    let t = target.one_row(&dialect::aggregate_query(
        target.dialect(),
        &plan.target_table,
        &plan.target_columns,
    ))?;

    let mut divergences = Vec::new();
    const AGGS: [&str; 4] = ["nulls", "min", "max", "length_sum"];
    for (i, name) in column_names.iter().enumerate() {
        for (j, agg) in AGGS.iter().enumerate() {
            let k = i * AGGS.len() + j;
            match (s.get(k), t.get(k)) {
                (Some(a), Some(b)) if a != b => divergences.push(Divergence {
                    locus: format!("{}:{name}:{agg}", plan.unit),
                    columns: vec![name.clone()],
                    detail: format!("{agg} differs"),
                    class: Class::Unexplained,
                }),
                (None, _) | (_, None) => {
                    return Err(ReadError::Shape {
                        expected: column_names.len() * AGGS.len(),
                        got: s.len().min(t.len()),
                        sql: "aggregate pack".into(),
                    });
                }
                _ => {}
            }
        }
    }
    Ok(TierOutcome {
        tier: Tier::V2Aggregates,
        source_path: source.label().to_string(),
        target_path: target.label().to_string(),
        divergences,
        controls: vec![],
    })
}

/// Parse a `(bucket, count, sum)` result set.
pub fn parse_buckets(
    rows: Vec<Vec<String>>,
) -> Result<std::collections::BTreeMap<u32, BucketChecksum>, ReadError> {
    let mut out = std::collections::BTreeMap::new();
    for r in rows {
        if r.len() < 3 {
            return Err(ReadError::Shape {
                expected: 3,
                got: r.len(),
                sql: "bucket checksum".into(),
            });
        }
        let parse_u = |v: &String, as_type: &'static str, column: usize| {
            v.trim().parse::<u128>().map_err(|_| ReadError::Parse {
                value: v.clone(),
                as_type,
                column,
            })
        };
        let bucket = parse_u(&r[0], "u32", 0)? as u32;
        out.insert(
            bucket,
            BucketChecksum {
                count: parse_u(&r[1], "u64", 1)? as u64,
                // Engines render an exact-integer sum with no fractional part, but a `numeric` sum
                // can come back as "123.0" from some drivers — tolerate a trailing ".0" rather than
                // failing a whole run on a formatting detail.
                sum: parse_u(&r[2].trim_end_matches(".0").to_string(), "u128", 2)?,
            },
        );
    }
    Ok(out)
}

/// V3 — bucketed checksums. Returns one divergence per differing bucket, which is exactly the input
/// bisect needs.
pub fn run_v3(
    plan: &UnitPlan,
    source: &dyn EngineReader,
    target: &dyn EngineReader,
) -> Result<TierOutcome, ReadError> {
    let s = parse_buckets(source.query(&dialect::bucket_checksum_query(
        source.dialect(),
        &plan.source_table,
        &plan.source_pk,
        &plan.source_columns,
        plan.buckets,
    ))?)?;
    let t = parse_buckets(target.query(&dialect::bucket_checksum_query(
        target.dialect(),
        &plan.target_table,
        &plan.target_pk,
        &plan.target_columns,
        plan.buckets,
    ))?)?;

    let mut divergences = Vec::new();
    let mut keys: Vec<u32> = s.keys().chain(t.keys()).copied().collect();
    keys.sort_unstable();
    keys.dedup();
    for b in keys {
        let (a, c) = (s.get(&b), t.get(&b));
        if a == c {
            continue;
        }
        // An absent bucket and a zero-count bucket mean the same thing; engines differ on whether
        // they emit the row, and that is not a divergence.
        let zero = BucketChecksum::default();
        let (a, c) = (a.unwrap_or(&zero), c.unwrap_or(&zero));
        if a == c {
            continue;
        }
        divergences.push(Divergence {
            locus: format!("{}:bucket:{b}", plan.unit),
            columns: vec![],
            detail: format!(
                "source count {} sum {}, target count {} sum {}",
                a.count, a.sum, c.count, c.sum
            ),
            class: Class::Unexplained,
        });
    }
    Ok(TierOutcome {
        tier: Tier::V3Checksums,
        source_path: source.label().to_string(),
        target_path: target.label().to_string(),
        divergences,
        controls: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Dialect;
    use crate::reader::MockReader;

    fn plan() -> UnitPlan {
        UnitPlan {
            unit: "public.orders".into(),
            source_table: "public.orders".into(),
            target_table: "sb.orders".into(),
            source_pk: "pk".into(),
            target_pk: "pk".into(),
            source_columns: vec!["a".into()],
            target_columns: vec!["a".into()],
            buckets: 4,
        }
    }

    #[test]
    fn a_tier_with_a_missed_control_never_reports_passed() {
        let o = TierOutcome {
            tier: Tier::V3Checksums,
            source_path: "pg".into(),
            target_path: "ch".into(),
            divergences: vec![],
            controls: vec![
                ControlResult {
                    control: "dropped_row".into(),
                    tier: Tier::V3Checksums,
                    detected: true,
                },
                ControlResult {
                    control: "column_swap".into(),
                    tier: Tier::V3Checksums,
                    detected: false,
                },
            ],
        };
        assert!(!o.passed(), "zero divergences must not be enough");
        assert_eq!(o.missed_controls(), vec!["column_swap"]);
        assert!(o.verdict().contains("FAILED"), "{}", o.verdict());
        assert!(o.verdict().contains("column_swap"));
    }

    #[test]
    fn a_tier_with_no_controls_at_all_is_not_silently_green() {
        // `controls_all_fired` is vacuously true with an empty list, so `passed()` alone cannot
        // distinguish "ran the controls" from "ran none". The verdict says how many fired, which is
        // what RFC 0162's report renders beside the result.
        let o = TierOutcome {
            tier: Tier::V1Counts,
            source_path: "pg".into(),
            target_path: "ch".into(),
            divergences: vec![],
            controls: vec![],
        };
        assert!(o.passed());
        assert!(
            o.verdict().contains("0 control(s) fired"),
            "{}",
            o.verdict()
        );
    }

    #[test]
    fn v1_reports_the_delta_not_the_rows() {
        let s = MockReader::new(Dialect::Postgres, "pg-replica").with(
            "SELECT count(*) FROM public.orders",
            vec![vec!["100".into()]],
        );
        let t = MockReader::new(Dialect::ClickHouse, "ch-readonly")
            .with("SELECT count(*) FROM sb.orders", vec![vec!["99".into()]]);
        let o = run_v1(&plan(), &s, &t).unwrap();
        assert_eq!(o.divergences.len(), 1);
        assert!(o.divergences[0].detail.contains("delta -1"));
        assert_eq!(o.source_path, "pg-replica");
        assert_eq!(
            o.target_path, "ch-readonly",
            "the read path must be recorded"
        );
    }

    #[test]
    fn v1_equal_counts_produce_nothing() {
        let s = MockReader::new(Dialect::Postgres, "pg").with(
            "SELECT count(*) FROM public.orders",
            vec![vec!["100".into()]],
        );
        let t = MockReader::new(Dialect::ClickHouse, "ch")
            .with("SELECT count(*) FROM sb.orders", vec![vec!["100".into()]]);
        assert!(run_v1(&plan(), &s, &t).unwrap().divergences.is_empty());
    }

    #[test]
    fn an_absent_bucket_equals_a_zero_count_bucket() {
        let p = plan();
        let sq = dialect::bucket_checksum_query(
            Dialect::Postgres,
            &p.source_table,
            &p.source_pk,
            &p.source_columns,
            4,
        );
        let tq = dialect::bucket_checksum_query(
            Dialect::ClickHouse,
            &p.target_table,
            &p.target_pk,
            &p.target_columns,
            4,
        );
        let s = MockReader::new(Dialect::Postgres, "pg")
            .with(sq, vec![vec!["1".into(), "5".into(), "500".into()]]);
        // Target also emits an explicit zero row for bucket 0, which PostgreSQL omitted.
        let t = MockReader::new(Dialect::ClickHouse, "ch").with(
            tq,
            vec![
                vec!["0".into(), "0".into(), "0".into()],
                vec!["1".into(), "5".into(), "500".into()],
            ],
        );
        let o = run_v3(&p, &s, &t).unwrap();
        assert!(
            o.divergences.is_empty(),
            "an omitted empty bucket is not a divergence: {:?}",
            o.divergences
        );
    }

    #[test]
    fn v3_reports_one_divergence_per_differing_bucket() {
        let p = plan();
        let sq = dialect::bucket_checksum_query(
            Dialect::Postgres,
            &p.source_table,
            &p.source_pk,
            &p.source_columns,
            4,
        );
        let tq = dialect::bucket_checksum_query(
            Dialect::ClickHouse,
            &p.target_table,
            &p.target_pk,
            &p.target_columns,
            4,
        );
        let s = MockReader::new(Dialect::Postgres, "pg").with(
            sq,
            vec![
                vec!["0".into(), "5".into(), "500".into()],
                vec!["2".into(), "7".into(), "700".into()],
            ],
        );
        let t = MockReader::new(Dialect::ClickHouse, "ch").with(
            tq,
            vec![
                vec!["0".into(), "5".into(), "501".into()],
                vec!["2".into(), "7".into(), "700".into()],
            ],
        );
        let o = run_v3(&p, &s, &t).unwrap();
        assert_eq!(o.divergences.len(), 1);
        assert_eq!(o.divergences[0].locus, "public.orders:bucket:0");
    }

    #[test]
    fn a_numeric_sum_with_a_trailing_point_zero_parses() {
        let out = parse_buckets(vec![vec!["0".into(), "5".into(), "500.0".into()]]).unwrap();
        assert_eq!(out[&0].sum, 500);
    }

    #[test]
    fn v2_names_the_column_and_aggregate_but_never_a_value() {
        let p = plan();
        let sq = dialect::aggregate_query(Dialect::Postgres, &p.source_table, &p.source_columns);
        let tq = dialect::aggregate_query(Dialect::ClickHouse, &p.target_table, &p.target_columns);
        let s = MockReader::new(Dialect::Postgres, "pg").with(
            sq,
            vec![vec!["0".into(), "aaa".into(), "zzz".into(), "300".into()]],
        );
        let t = MockReader::new(Dialect::ClickHouse, "ch").with(
            tq,
            vec![vec!["3".into(), "aaa".into(), "zzz".into(), "300".into()]],
        );
        let o = run_v2(&p, &s, &t, &["amount".into()]).unwrap();
        assert_eq!(o.divergences.len(), 1);
        assert_eq!(o.divergences[0].locus, "public.orders:amount:nulls");
        assert!(
            !o.divergences[0].detail.contains("aaa") && !o.divergences[0].detail.contains('3'),
            "a min/max or count is row data and must not appear: {:?}",
            o.divergences[0].detail
        );
    }

    #[test]
    fn unimplemented_tiers_say_so() {
        assert!(Tier::V4RowDiff.is_implemented());
        assert!(!Tier::V5Logic.is_implemented());
        assert!(!Tier::V6Invariants.is_implemented());
    }
}
