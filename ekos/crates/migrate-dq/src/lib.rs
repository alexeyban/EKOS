//! RFC 0158 — data-quality rules, target compatibility, and the source-coverage completeness check.
//!
//! Three things, and the first two are only useful because of how they differ from what a
//! type-lookup table does:
//!
//! 1. **Rules measure affected rows.** "This column is `numeric` and the target needs a precision"
//!    flags every `numeric` column and buries the three that overflow. "Seventeen rows exceed
//!    precision 38, here is the query" is a five-minute decision.
//! 2. **Behavioural findings are first-class.** Losing foreign-key enforcement corrupts nothing on
//!    load day. It corrupts the database six months later, and it is the finding people skip.
//! 3. **The completeness check** makes RFC 0154's coverage promise mechanical: every source object
//!    is translated with evidence or dispositioned by a named human, and "no rule matched" is a
//!    failure rather than a pass.

pub mod catalog;
pub mod completeness;
pub mod model;

pub use catalog::{COLUMN_RULES, TABLE_RULES, rule_by_id};
pub use completeness::{Accounted, CompletenessReport, SourceObject, check};
pub use model::{ColumnContext, Family, Finding, Lossiness, Rule, Severity, TableContext, Target};

/// Run every column rule that applies, returning findings without their measurements.
///
/// Measurement is the caller's job because it needs a live connection, and this crate deliberately
/// has none: the rules are data and stay testable without a database.
pub fn evaluate_column(c: &ColumnContext) -> Vec<Finding> {
    COLUMN_RULES
        .iter()
        .filter(|r| (r.applies)(c))
        .map(|r| Finding {
            rule_id: r.id.to_string(),
            family: r.family,
            severity: r.severity,
            target: r.target,
            lossiness: r.lossiness,
            object: format!("{}.{}", c.table, c.column),
            message: (r.explain)(c),
            affected_rows: None,
            evidence_sql: (r.measure)(c),
        })
        .collect()
}

/// Run every table rule that applies.
pub fn evaluate_table(t: &TableContext) -> Vec<Finding> {
    TABLE_RULES
        .iter()
        .filter(|r| (r.applies)(t))
        .map(|r| Finding {
            rule_id: r.id.to_string(),
            family: r.family,
            severity: r.severity,
            target: r.target,
            lossiness: r.lossiness,
            object: t.table.clone(),
            message: (r.explain)(t),
            affected_rows: None,
            evidence_sql: None,
        })
        .collect()
}
