//! RFC 0158 — the rule model.
//!
//! A rule is **data**: an id, a predicate over what is known about a column or table, and a SQL
//! template that measures how many rows are affected. Adding one is a catalog entry plus a fixture,
//! not a code change — and the fixture is enforced, so "rules are data" does not quietly become
//! "rules are untested data".
//!
//! The inputs are deliberately source-independent. A rule sees a `ColumnContext`, not a PostgreSQL
//! catalog row, so the same catalog serves a future SQL Server or Oracle connector.

use serde::{Deserialize, Serialize};

/// Which target a compatibility rule is about. `None` on a data-quality rule, which is about the
/// source regardless of where it is going.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    ClickHouse,
    Delta,
}

impl Target {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClickHouse => "clickhouse",
            Self::Delta => "delta",
        }
    }
}

/// What a rule belongs to. The families are RFC 0158's, and the enum is exhaustive so a new rule has
/// to be placed deliberately rather than landing in a bucket nobody reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Completeness,
    Uniqueness,
    ReferentialIntegrity,
    Validity,
    Consistency,
    Timeliness,
    /// Target compatibility — a type or semantic the target cannot hold as the source does.
    Compatibility,
}

impl Family {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completeness => "completeness",
            Self::Uniqueness => "uniqueness",
            Self::ReferentialIntegrity => "referential_integrity",
            Self::Validity => "validity",
            Self::Consistency => "consistency",
            Self::Timeliness => "timeliness",
            Self::Compatibility => "compatibility",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warn,
    /// The unit cannot advance until a human dispositions it.
    Blocking,
}

/// How much a mapping costs, when a rule is about one.
///
/// `NarrowingSafe` is the class the profiler earns its keep for: the target is narrower *and the
/// measured data proves nothing is affected*. It is a claim about data at a point in time, so the
/// finding cites the profile that established it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lossiness {
    Exact,
    Widening,
    NarrowingSafe,
    Lossy,
    /// Not a type problem at all: the target holds every value but behaves differently.
    /// Enforcement dropped, dedup made eventual, ordering changed. These are the findings people
    /// skip and then discover six months later.
    Behavioural,
}

impl Lossiness {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Widening => "widening",
            Self::NarrowingSafe => "narrowing_safe",
            Self::Lossy => "lossy",
            Self::Behavioural => "behavioural",
        }
    }

    /// Whether a human must decide before a unit may advance.
    pub fn needs_disposition(self) -> bool {
        matches!(self, Self::Lossy | Self::Behavioural)
    }
}

/// What a rule knows about a column when deciding whether it applies.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ColumnContext {
    pub table: String,
    pub column: String,
    /// The live rendering, e.g. `numeric`, `numeric(12,2)`, `timestamp with time zone`.
    pub data_type: String,
    pub nullable: bool,
    /// From the profile, where one has been taken. `None` means unprofiled — which a rule must
    /// treat as "cannot tell", never as a convenient default.
    pub null_fraction: Option<f64>,
    pub distinct_estimate: Option<f64>,
    pub numeric_precision_used: Option<i32>,
    pub numeric_scale_used: Option<i32>,
    /// `true` when the column is classified as personal data, so a rule must not ask for example
    /// values (RFC 0157).
    pub pii: bool,
    pub row_count: i64,
}

impl ColumnContext {
    /// The type without its parameters: `numeric(12,2)` → `numeric`.
    pub fn base_type(&self) -> &str {
        self.data_type
            .split_once('(')
            .map(|(h, _)| h)
            .unwrap_or(&self.data_type)
            .trim()
    }

    /// The declared parameters, when the type states them.
    pub fn type_params(&self) -> Option<(u32, Option<u32>)> {
        let (_, rest) = self.data_type.split_once('(')?;
        let inner = rest.trim_end_matches(')');
        let mut it = inner.split(',').map(|p| p.trim().parse::<u32>().ok());
        let first = it.next().flatten()?;
        Some((first, it.next().flatten()))
    }

    /// An unconstrained `numeric` — no precision or scale declared, so the target has nothing to
    /// map to without measured data.
    pub fn is_unconstrained_numeric(&self) -> bool {
        matches!(self.base_type(), "numeric" | "decimal") && self.type_params().is_none()
    }
}

/// What a table-level rule knows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableContext {
    pub table: String,
    pub row_count: i64,
    pub has_updates: bool,
    pub looks_static: bool,
    /// Declared constraints that are present but *not enforced* — `NOT VALID` foreign keys,
    /// deferrable uniques. Their existence is not a guarantee, and a migration that trusts them
    /// inherits whatever the source never checked.
    pub unvalidated_constraints: Vec<String>,
    pub primary_key_columns: Vec<String>,
}

/// One rule.
pub struct Rule {
    pub id: &'static str,
    pub title: &'static str,
    pub family: Family,
    pub severity: Severity,
    pub target: Option<Target>,
    pub lossiness: Option<Lossiness>,
    /// Whether the rule has anything to say about this column.
    pub applies: fn(&ColumnContext) -> bool,
    /// The SQL that counts affected rows, or `None` when the rule can conclude from the profile
    /// alone and needs no scan.
    pub measure: fn(&ColumnContext) -> Option<String>,
    /// What the finding should say. Never contains a value from the data.
    pub explain: fn(&ColumnContext) -> String,
}

impl std::fmt::Debug for Rule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rule")
            .field("id", &self.id)
            .field("family", &self.family)
            .finish()
    }
}

/// A rule that fired.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub rule_id: String,
    pub family: Family,
    pub severity: Severity,
    pub target: Option<Target>,
    pub lossiness: Option<Lossiness>,
    pub object: String,
    pub message: String,
    /// How many rows the rule measured as affected. `None` where no scan was run — which the report
    /// must render as "not measured", never as zero.
    pub affected_rows: Option<i64>,
    /// The SQL that produced `affected_rows`, so a reviewer can re-run it themselves.
    pub evidence_sql: Option<String>,
}

impl Finding {
    /// Whether this blocks a unit until dispositioned.
    pub fn blocks(&self) -> bool {
        self.severity == Severity::Blocking
            || self.lossiness.is_some_and(Lossiness::needs_disposition)
    }

    /// A finding measured as affecting zero rows is still a finding — the rule matched — but it is
    /// the cheapest possible disposition, and saying so is the difference between a report someone
    /// acts on and one they skim.
    pub fn is_theoretical(&self) -> bool {
        self.affected_rows == Some(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(ty: &str) -> ColumnContext {
        ColumnContext {
            data_type: ty.into(),
            ..Default::default()
        }
    }

    #[test]
    fn type_parameters_are_parsed_or_absent() {
        assert_eq!(ctx("numeric(12,2)").type_params(), Some((12, Some(2))));
        assert_eq!(ctx("character varying(50)").type_params(), Some((50, None)));
        assert_eq!(ctx("numeric").type_params(), None);
        assert_eq!(ctx("bigint").type_params(), None);
    }

    #[test]
    fn base_type_drops_parameters_and_keeps_multi_word_names() {
        assert_eq!(ctx("numeric(12,2)").base_type(), "numeric");
        assert_eq!(
            ctx("timestamp with time zone").base_type(),
            "timestamp with time zone"
        );
        assert_eq!(ctx("bigint").base_type(), "bigint");
    }

    #[test]
    fn an_unconstrained_numeric_is_the_one_without_parameters() {
        assert!(ctx("numeric").is_unconstrained_numeric());
        assert!(ctx("decimal").is_unconstrained_numeric());
        assert!(!ctx("numeric(12,2)").is_unconstrained_numeric());
        assert!(!ctx("bigint").is_unconstrained_numeric());
    }

    #[test]
    fn lossy_and_behavioural_both_need_a_human() {
        assert!(Lossiness::Lossy.needs_disposition());
        assert!(
            Lossiness::Behavioural.needs_disposition(),
            "losing FK enforcement corrupts nothing on load day and everything six months later"
        );
        assert!(!Lossiness::Exact.needs_disposition());
        assert!(!Lossiness::Widening.needs_disposition());
        assert!(!Lossiness::NarrowingSafe.needs_disposition());
    }

    #[test]
    fn a_behavioural_finding_blocks_even_at_warn_severity() {
        let f = Finding {
            rule_id: "X".into(),
            family: Family::Compatibility,
            severity: Severity::Warn,
            target: Some(Target::ClickHouse),
            lossiness: Some(Lossiness::Behavioural),
            object: "public.orders".into(),
            message: "m".into(),
            affected_rows: None,
            evidence_sql: None,
        };
        assert!(f.blocks());
    }

    #[test]
    fn zero_affected_rows_is_theoretical_but_unmeasured_is_not() {
        let mut f = Finding {
            rule_id: "X".into(),
            family: Family::Compatibility,
            severity: Severity::Warn,
            target: None,
            lossiness: None,
            object: "o".into(),
            message: "m".into(),
            affected_rows: Some(0),
            evidence_sql: None,
        };
        assert!(f.is_theoretical());
        f.affected_rows = None;
        assert!(
            !f.is_theoretical(),
            "not measured must never render as zero affected rows"
        );
    }
}
