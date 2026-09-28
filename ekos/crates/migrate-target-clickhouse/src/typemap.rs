//! RFC 0159 — the type mapping registry.
//!
//! Two failure modes bracket this work, and the registry exists to sit between them.
//!
//! A **naive mapping** — `numeric` → `Decimal(76, 20)`, every column `Nullable`, `ORDER BY` the
//! primary key because that is what PostgreSQL had — is what a type-lookup table alone produces. It
//! is correct, enormous and slow.
//!
//! An **aggressive mapping** — `numeric` → `Decimal(18, 2)` because the first thousand rows fit —
//! silently truncates in month three.
//!
//! Both are avoided by the same thing: measured data, and a lossiness class that says what the
//! measurement does and does not prove.

use serde::{Deserialize, Serialize};

/// What a mapping costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lossiness {
    /// Every source value round-trips.
    Exact,
    /// The target holds strictly more.
    Widening,
    /// The target is narrower **and the profile proves no source value is affected**. The claim is
    /// about data at a point in time, so the mapping cites the profile that established it — and a
    /// later re-profile that breaks the bound invalidates the mapping rather than it silently
    /// outliving its evidence.
    NarrowingSafe,
    /// Values will be changed or lost. Requires a disposition and an R3 approval, and can never be
    /// auto-selected however obviously it is what someone wants.
    Lossy,
}

impl Lossiness {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Widening => "widening",
            Self::NarrowingSafe => "narrowing_safe",
            Self::Lossy => "lossy",
        }
    }

    pub fn needs_approval(self) -> bool {
        matches!(self, Self::Lossy)
    }
}

/// What the profiler measured about a column. Everything is `Option` because "not measured" is a
/// real state that must never read as a convenient default.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColumnEvidence {
    pub null_fraction: Option<f64>,
    pub distinct: Option<f64>,
    pub row_count: i64,
    pub numeric_precision_used: Option<i32>,
    pub numeric_scale_used: Option<i32>,
    /// `true` for a column whose domain keeps growing — an identity, a sequence-backed id, a
    /// monotonic timestamp. **Never narrowed on observed maximum**: the profile describes the past,
    /// and this column's future is larger than its past by construction.
    pub growing: bool,
    /// The profile fact this evidence came from, cited by any `NarrowingSafe` mapping.
    pub profile_ref: Option<String>,
}

/// One column's mapping decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mapping {
    pub column: String,
    pub source_type: String,
    pub target_type: String,
    pub lossiness: Lossiness,
    pub rule_id: String,
    pub rationale: String,
    /// The profile that justifies a `NarrowingSafe` mapping. `None` for every other class.
    pub evidence: Option<String>,
}

fn base_and_params(ty: &str) -> (String, Option<(u32, Option<u32>)>) {
    let lower = ty.trim().to_ascii_lowercase();
    match lower.split_once('(') {
        Some((h, rest)) => {
            let inner = rest.trim_end_matches(')');
            let mut it = inner.split(',').map(|p| p.trim().parse::<u32>().ok());
            let first = it.next().flatten();
            let second = it.next().flatten();
            (h.trim().to_string(), first.map(|f| (f, second)))
        }
        None => (lower, None),
    }
}

/// Wrap in `Nullable(T)` unless the profile proves it is unnecessary.
///
/// Dropping `Nullable` is a real storage and query win in ClickHouse, and it is a **narrowing**: a
/// null arriving later becomes a load failure rather than a silent zero. So it is only offered when
/// the source column is `NOT NULL`, or the profile measured zero nulls — and in the second case the
/// mapping says the claim is only as durable as that measurement.
fn nullable(
    target: &str,
    nullable_source: bool,
    ev: &ColumnEvidence,
) -> (String, Lossiness, String) {
    if !nullable_source {
        return (
            target.to_string(),
            Lossiness::Exact,
            "source column is NOT NULL".into(),
        );
    }
    match ev.null_fraction {
        Some(f) if f == 0.0 && ev.row_count > 0 => (
            target.to_string(),
            Lossiness::NarrowingSafe,
            format!(
                "declared nullable, but the profile measured no nulls in {} rows; a null arriving \
                 later becomes a load failure rather than a silent default",
                ev.row_count
            ),
        ),
        _ => (
            nullable_of(target),
            Lossiness::Exact,
            "source column is nullable".into(),
        ),
    }
}

/// `Nullable(T)`, nested the way ClickHouse accepts it: `LowCardinality` must be the **outer**
/// wrapper. `Nullable(LowCardinality(String))` is rejected at `CREATE TABLE` ("Nested type
/// LowCardinality(String) cannot be inside Nullable type") — found loading LedgerSMB's
/// `acc_trans.source`, a nullable low-distinct text column; no fixture had had one.
fn nullable_of(target: &str) -> String {
    match target
        .strip_prefix("LowCardinality(")
        .and_then(|t| t.strip_suffix(')'))
    {
        Some(inner) => format!("LowCardinality(Nullable({inner}))"),
        None => format!("Nullable({target})"),
    }
}

/// Map one PostgreSQL column to a ClickHouse type.
pub fn map_column(
    column: &str,
    source_type: &str,
    nullable_source: bool,
    ev: &ColumnEvidence,
) -> Mapping {
    let (base, params) = base_and_params(source_type);
    let mk = |target: &str, lossiness: Lossiness, rule: &str, why: &str| {
        let (t, null_loss, null_why) = nullable(target, nullable_source, ev);
        // The worse of the two classes wins: a narrowing-safe nullability decision on top of a
        // lossy type is still lossy.
        let combined = match (lossiness, null_loss) {
            (Lossiness::Lossy, _) | (_, Lossiness::Lossy) => Lossiness::Lossy,
            (Lossiness::NarrowingSafe, _) | (_, Lossiness::NarrowingSafe) => {
                Lossiness::NarrowingSafe
            }
            (Lossiness::Widening, _) | (_, Lossiness::Widening) => Lossiness::Widening,
            _ => Lossiness::Exact,
        };
        Mapping {
            column: column.to_string(),
            source_type: source_type.to_string(),
            target_type: t,
            lossiness: combined,
            rule_id: rule.to_string(),
            rationale: format!("{why}; {null_why}"),
            evidence: if combined == Lossiness::NarrowingSafe {
                ev.profile_ref.clone()
            } else {
                None
            },
        }
    };

    match base.as_str() {
        "bigint" | "int8" | "bigserial" => mk("Int64", Lossiness::Exact, "TM.INT64", "direct"),
        "integer" | "int" | "int4" | "serial" => {
            mk("Int32", Lossiness::Exact, "TM.INT32", "direct")
        }
        "smallint" | "int2" | "smallserial" => mk("Int16", Lossiness::Exact, "TM.INT16", "direct"),
        "boolean" | "bool" => mk("Bool", Lossiness::Exact, "TM.BOOL", "direct"),
        "real" | "float4" => mk("Float32", Lossiness::Exact, "TM.FLOAT32", "direct"),
        "double precision" | "float8" => mk("Float64", Lossiness::Exact, "TM.FLOAT64", "direct"),
        "uuid" => mk("UUID", Lossiness::Exact, "TM.UUID", "direct"),
        "bytea" => mk(
            "String",
            Lossiness::Exact,
            "TM.BYTEA",
            "ClickHouse String is byte-safe",
        ),
        "date" => mk(
            "Date32",
            Lossiness::Exact,
            "TM.DATE",
            "Date32 covers 1900-2299; dates before 1900 are a COMPAT.CH.DATE_BEFORE_1900 finding",
        ),
        "timestamp with time zone" | "timestamptz" => mk(
            "DateTime64(6, 'UTC')",
            Lossiness::Exact,
            "TM.TIMESTAMPTZ",
            "pinned to UTC so the rendering does not depend on a session setting",
        ),
        "timestamp without time zone" | "timestamp" => mk(
            "DateTime64(6)",
            Lossiness::Exact,
            "TM.TIMESTAMP",
            "no zone applied: a no-TZ column is a wall-clock reading",
        ),
        "json" | "jsonb" => mk(
            "String",
            Lossiness::Widening,
            "TM.JSON",
            "stored as text; ClickHouse's JSON type is version-dependent and validated at V2 only",
        ),
        "inet" | "cidr" => mk(
            "String",
            Lossiness::Widening,
            "TM.INET",
            "IPv4/IPv6 would need the family split per row; String holds both",
        ),
        "interval" => mk(
            "String",
            Lossiness::Lossy,
            "TM.INTERVAL",
            "ClickHouse has no composite interval; a months/days/microseconds split is the real \
             mapping and needs a decision",
        ),
        "text" | "character varying" | "varchar" => {
            map_string(column, source_type, nullable_source, ev, mk)
        }
        "character" | "char" | "bpchar" => mk(
            "String",
            Lossiness::Lossy,
            "TM.CHAR",
            "PostgreSQL pads char(n) and ClickHouse does not; the padding is a real difference",
        ),
        "numeric" | "decimal" => map_numeric(column, source_type, params, nullable_source, ev, mk),
        _ => mk(
            "String",
            Lossiness::Lossy,
            "TM.UNKNOWN",
            "no mapping rule for this type; String preserves the text form but loses the semantics",
        ),
    }
}

fn map_string(
    _column: &str,
    _source_type: &str,
    _nullable_source: bool,
    ev: &ColumnEvidence,
    mk: impl Fn(&str, Lossiness, &str, &str) -> Mapping,
) -> Mapping {
    // `LowCardinality` is a real win on a low-distinct column and a real loss on a high-distinct
    // one, so it is offered only when the profile says so — and never on an unmeasured column.
    match (ev.distinct, ev.row_count) {
        (Some(d), rows) if rows > 0 && d > 0.0 && d <= 10_000.0 && d / rows as f64 <= 0.1 => mk(
            "LowCardinality(String)",
            Lossiness::Exact,
            "TM.TEXT_LOWCARD",
            &format!(
                "the profile measured about {d:.0} distinct values in {rows} rows, comfortably \
                 inside LowCardinality's useful range"
            ),
        ),
        _ => mk("String", Lossiness::Exact, "TM.TEXT", "direct"),
    }
}

fn map_numeric(
    _column: &str,
    _source_type: &str,
    params: Option<(u32, Option<u32>)>,
    _nullable_source: bool,
    ev: &ColumnEvidence,
    mk: impl Fn(&str, Lossiness, &str, &str) -> Mapping,
) -> Mapping {
    match params {
        // Declared precision and scale: a direct mapping, unless it exceeds what ClickHouse holds.
        Some((p, s)) if p <= 76 => {
            let scale = s.unwrap_or(0);
            let width = if p <= 9 {
                32
            } else if p <= 18 {
                64
            } else if p <= 38 {
                128
            } else {
                256
            };
            mk(
                &format!("Decimal{width}({scale})"),
                Lossiness::Exact,
                "TM.NUMERIC_DECLARED",
                &format!("declared numeric({p},{scale})"),
            )
        }
        Some((p, _)) => mk(
            "String",
            Lossiness::Lossy,
            "TM.NUMERIC_TOO_WIDE",
            &format!("declared precision {p} exceeds ClickHouse's maximum of 76"),
        ),
        // Unconstrained: only the profile can answer, and only for a column that is not growing.
        None => match (ev.numeric_precision_used, ev.numeric_scale_used, ev.growing) {
            (Some(p), Some(s), false) => {
                let precision = p.max(s + 1).clamp(1, 76);
                let width = if precision <= 18 {
                    64
                } else if precision <= 38 {
                    128
                } else {
                    256
                };
                mk(
                    &format!("Decimal{width}({s})"),
                    Lossiness::NarrowingSafe,
                    "TM.NUMERIC_PROFILED",
                    &format!(
                        "unconstrained numeric; the measured data uses precision {p} scale {s}, so \
                         this holds every value seen — and only those"
                    ),
                )
            }
            (_, _, true) => mk(
                "Decimal128(4)",
                Lossiness::Lossy,
                "TM.NUMERIC_GROWING",
                "unconstrained numeric on a growing column: the profile describes the past, and \
                 this column's future is larger by construction, so no measurement can make a \
                 narrowing safe",
            ),
            _ => mk(
                "Decimal128(4)",
                Lossiness::Lossy,
                "TM.NUMERIC_UNPROFILED",
                "unconstrained numeric with no profile. Nothing here is a measurement; run \
                 `ekos migrate profile --tier p1` and re-map rather than accepting this default",
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev() -> ColumnEvidence {
        ColumnEvidence {
            row_count: 100_000,
            profile_ref: Some("PROF.1".into()),
            ..Default::default()
        }
    }

    #[test]
    fn declared_numeric_picks_the_narrowest_decimal_that_fits() {
        for (src, want) in [
            ("numeric(9,2)", "Decimal32(2)"),
            ("numeric(18,2)", "Decimal64(2)"),
            ("numeric(38,4)", "Decimal128(4)"),
            ("numeric(60,4)", "Decimal256(4)"),
        ] {
            let m = map_column("c", src, false, &ev());
            assert_eq!(m.target_type, want, "{src}");
            assert_eq!(m.lossiness, Lossiness::Exact);
        }
    }

    #[test]
    fn precision_beyond_the_target_is_lossy_and_says_why() {
        let m = map_column("c", "numeric(80,2)", false, &ev());
        assert_eq!(m.lossiness, Lossiness::Lossy);
        assert!(m.rationale.contains("76"), "{}", m.rationale);
    }

    /// The payoff the profiler exists for — and the mapping cites the profile, because the claim is
    /// about data at a point in time.
    #[test]
    fn a_profiled_unconstrained_numeric_becomes_narrowing_safe_with_its_evidence() {
        let e = ColumnEvidence {
            numeric_precision_used: Some(14),
            numeric_scale_used: Some(2),
            ..ev()
        };
        let m = map_column("amount", "numeric", false, &e);
        assert_eq!(m.target_type, "Decimal64(2)");
        assert_eq!(m.lossiness, Lossiness::NarrowingSafe);
        assert_eq!(m.evidence.as_deref(), Some("PROF.1"));
    }

    /// The guard that stops the profiler's biggest win from becoming its biggest mistake.
    #[test]
    fn a_growing_column_is_never_narrowed_on_measured_data() {
        let e = ColumnEvidence {
            numeric_precision_used: Some(6),
            numeric_scale_used: Some(0),
            growing: true,
            ..ev()
        };
        let m = map_column("id", "numeric", false, &e);
        assert_eq!(
            m.lossiness,
            Lossiness::Lossy,
            "the profile describes the past; a growing column's future is larger by construction"
        );
        assert!(m.rationale.contains("growing"), "{}", m.rationale);
        assert_eq!(
            m.evidence, None,
            "a lossy mapping has no narrowing evidence to cite"
        );
    }

    #[test]
    fn an_unprofiled_unconstrained_numeric_is_lossy_and_says_what_to_run() {
        let m = map_column("amount", "numeric", false, &ev());
        assert_eq!(m.lossiness, Lossiness::Lossy);
        assert!(
            m.rationale.contains("ekos migrate profile"),
            "{}",
            m.rationale
        );
    }

    #[test]
    fn nullability_is_dropped_only_when_the_profile_proves_it() {
        let unmeasured = map_column("c", "text", true, &ev());
        assert_eq!(unmeasured.target_type, "Nullable(String)");

        let measured = ColumnEvidence {
            null_fraction: Some(0.0),
            ..ev()
        };
        let m = map_column("c", "text", true, &measured);
        assert_eq!(m.target_type, "String");
        assert_eq!(m.lossiness, Lossiness::NarrowingSafe);
        assert!(m.rationale.contains("load failure"), "{}", m.rationale);

        let has_nulls = ColumnEvidence {
            null_fraction: Some(0.02),
            ..ev()
        };
        assert_eq!(
            map_column("c", "text", true, &has_nulls).target_type,
            "Nullable(String)"
        );
    }

    #[test]
    fn a_nullable_low_cardinality_column_nests_the_way_clickhouse_accepts() {
        let low = ColumnEvidence {
            distinct: Some(12.0),
            null_fraction: Some(0.4),
            ..ev()
        };
        assert_eq!(
            map_column("memo", "text", true, &low).target_type,
            "LowCardinality(Nullable(String))"
        );
        assert_eq!(nullable_of("Int32"), "Nullable(Int32)");
    }

    #[test]
    fn low_cardinality_is_offered_only_on_measured_low_distinct_columns() {
        let low = ColumnEvidence {
            distinct: Some(12.0),
            ..ev()
        };
        assert_eq!(
            map_column("country", "text", false, &low).target_type,
            "LowCardinality(String)"
        );

        // High distinct: plain String.
        let high = ColumnEvidence {
            distinct: Some(90_000.0),
            ..ev()
        };
        assert_eq!(
            map_column("email", "text", false, &high).target_type,
            "String"
        );

        // Unmeasured: never guessed.
        assert_eq!(map_column("x", "text", false, &ev()).target_type, "String");
    }

    /// A unique column resolves to ~rowcount distinct. Offering LowCardinality for it is the
    /// failure the `n_distinct` convention causes when read as a count.
    #[test]
    fn a_unique_column_does_not_get_low_cardinality() {
        let unique = ColumnEvidence {
            distinct: Some(100_000.0),
            ..ev()
        };
        assert_eq!(
            map_column("id", "text", false, &unique).target_type,
            "String"
        );
    }

    #[test]
    fn the_worse_lossiness_wins_when_two_decisions_combine() {
        // A lossy type plus a narrowing-safe nullability decision is still lossy.
        let e = ColumnEvidence {
            null_fraction: Some(0.0),
            ..ev()
        };
        let m = map_column("span", "interval", true, &e);
        assert_eq!(m.lossiness, Lossiness::Lossy);
        assert!(Lossiness::Lossy.needs_approval());
        assert!(!Lossiness::NarrowingSafe.needs_approval());
    }

    #[test]
    fn an_unknown_type_is_lossy_rather_than_silently_stringified() {
        let m = map_column("geom", "geometry(Point,4326)", false, &ev());
        assert_eq!(m.lossiness, Lossiness::Lossy);
        assert!(m.rationale.contains("no mapping rule"), "{}", m.rationale);
    }

    #[test]
    fn timestamps_pin_utc_and_naive_timestamps_do_not() {
        assert_eq!(
            map_column("t", "timestamp with time zone", false, &ev()).target_type,
            "DateTime64(6, 'UTC')"
        );
        assert_eq!(
            map_column("t", "timestamp without time zone", false, &ev()).target_type,
            "DateTime64(6)"
        );
    }
}
