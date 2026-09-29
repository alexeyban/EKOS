//! RFC 0157 — profiling, in three escalating cost tiers.
//!
//! What the profile is *for* decides its shape. RFC 0159 needs the numeric scale a column actually
//! uses, to offer a `narrowing-safe` mapping instead of `Decimal(76, 20)`. RFC 0158 needs null rates
//! and duplicate rates. RFC 0166 needs to know which columns are monotonic enough to be a watermark.
//! None of those needs a single row value to leave the database, and none of them is stored here.
//!
//! # `pg_stats` is not free of row data
//!
//! The P0 tier is "free" in cost and **not** free in exposure: `pg_stats.most_common_vals` and
//! `histogram_bounds` are literal values sampled from the table. A profiler that treats P0 as safe
//! because it issues no scan copies real customer data into the ledger at zero cost, which is the
//! worst possible trade. Everything read from `pg_stats` goes through the same redaction and the
//! same PII suppression as a sampled value.

use crate::PgError;
use crate::catalog::CatalogSource;
use crate::pii::{self, Classification};
use ekos_common::redaction::{RedactionConfig, redact};
use serde::{Deserialize, Serialize};

/// How much work a profile is allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileTier {
    /// Catalog and planner statistics only. No scan of user data.
    P0,
    /// Bounded `TABLESAMPLE`. One pass over a small fraction.
    P1,
    /// Full scans: exact counts, exact distinct, exact min/max. Behind a budget and an approval.
    P2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableProfile {
    pub qualified_name: String,
    pub tier: ProfileTier,
    /// `reltuples` at P0/P1, an exact `count(*)` at P2.
    pub row_count: i64,
    pub row_count_is_exact: bool,
    pub total_bytes: i64,
    pub last_analyze: Option<String>,
    /// From `pg_stat_user_tables`. Drives "archive, do not migrate" (RFC 0158) and the incremental
    /// strategy (RFC 0166): a table with zero updates never needs `ReplacingMergeTree`.
    pub inserts: i64,
    pub updates: i64,
    pub deletes: i64,
    pub seq_scans: i64,
    pub index_scans: i64,
}

impl TableProfile {
    /// No writes recorded since statistics were last reset. A candidate for archiving rather than
    /// migrating — and a *candidate*, not a conclusion: statistics reset on a restart.
    pub fn looks_static(&self) -> bool {
        self.inserts == 0 && self.updates == 0 && self.deletes == 0
    }

    /// Whether a mutable-entity target design is warranted (RFC 0159's `ReplacingMergeTree`).
    pub fn has_updates(&self) -> bool {
        self.updates > 0 || self.deletes > 0
    }
}

/// What the planner knows, or what a sample measured, about one column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnProfile {
    pub qualified_name: String,
    pub tier: ProfileTier,
    pub data_type: String,
    pub null_fraction: f64,
    /// PostgreSQL's convention: a negative `n_distinct` is a *fraction of rows*, not a count. Kept
    /// as the interpreted absolute estimate, with the convention handled at read time.
    pub distinct_estimate: Option<f64>,
    pub avg_width: i32,
    /// Only ever populated for non-PII columns, and only for orderable non-text types where the
    /// bound is what RFC 0158's range rules need (a date outside `Date32`, a numeric beyond
    /// precision 38). For a text column a min or a max *is* a value, so it stays `None`.
    pub min: Option<String>,
    pub max: Option<String>,
    /// The scale and precision the data actually uses, which is what makes a `narrowing-safe`
    /// mapping possible (RFC 0159). P1+.
    pub numeric_precision_used: Option<i32>,
    pub numeric_scale_used: Option<i32>,
    /// A watermark candidate for RFC 0166. P1+.
    pub monotonic: Option<bool>,
    pub pii: Option<Classification>,
    /// `true` when values were withheld because of [`pii`]. Recorded so a reader can tell
    /// "no values" from "column not profiled".
    pub values_suppressed: bool,
}

impl ColumnProfile {
    /// Does the measured data fit inside `Decimal(precision, scale)`?
    ///
    /// The question RFC 0159 asks before offering `narrowing-safe`. `None` when the profile cannot
    /// answer — which must read as "no", never as "yes".
    pub fn fits_decimal(&self, precision: i32, scale: i32) -> Option<bool> {
        match (self.numeric_precision_used, self.numeric_scale_used) {
            (Some(p), Some(s)) => Some(p <= precision && s <= scale),
            _ => None,
        }
    }
}

fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Quote an identifier for use in generated SQL. Never concatenate a raw one.
fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn split_qualified(qualified: &str) -> Result<(&str, &str), PgError> {
    qualified.split_once('.').ok_or_else(|| PgError::Query {
        sql: qualified.to_string(),
        message: "expected schema.table".into(),
    })
}

/// P0 — catalog and planner statistics. No scan of user data.
pub fn profile_table_p0(src: &dyn CatalogSource, qualified: &str) -> Result<TableProfile, PgError> {
    let (schema, table) = split_qualified(qualified)?;
    let rows = src.rows(&format!(
        "SELECT c.reltuples::bigint, pg_total_relation_size(c.oid), \
                COALESCE(GREATEST(s.last_analyze, s.last_autoanalyze)::text, ''), \
                COALESCE(s.n_tup_ins, 0), COALESCE(s.n_tup_upd, 0), COALESCE(s.n_tup_del, 0), \
                COALESCE(s.seq_scan, 0), COALESCE(s.idx_scan, 0) \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_stat_user_tables s ON s.relid = c.oid \
         WHERE n.nspname = {} AND c.relname = {}",
        q(schema),
        q(table)
    ))?;
    let r = rows.first().ok_or_else(|| PgError::Query {
        sql: qualified.to_string(),
        message: "no such table".into(),
    })?;
    let num = |i: usize| r.get(i).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
    Ok(TableProfile {
        qualified_name: qualified.to_string(),
        tier: ProfileTier::P0,
        // `reltuples` is -1 on a table that has never been analyzed. Reporting -1 rows would be
        // worse than reporting 0, and both are wrong — so the estimate is clamped and
        // `row_count_is_exact` stays false, which is the honest signal.
        row_count: num(0).max(0),
        row_count_is_exact: false,
        total_bytes: num(1),
        last_analyze: r.get(2).filter(|s| !s.is_empty()).cloned(),
        inserts: num(3),
        updates: num(4),
        deletes: num(5),
        seq_scans: num(6),
        index_scans: num(7),
    })
}

/// P0 column statistics, from `pg_stats`.
///
/// **`pg_stats` rows are only visible for tables the role can read**, and the view already filters
/// by that. A column missing here is therefore either un-analyzed or unreadable, and both mean the
/// same thing to a caller: no statistics, do not guess.
pub fn profile_columns_p0(
    src: &dyn CatalogSource,
    qualified: &str,
    redaction: &RedactionConfig,
) -> Result<Vec<ColumnProfile>, PgError> {
    let (schema, table) = split_qualified(qualified)?;
    let rows = src.rows(&format!(
        "SELECT a.attname, format_type(a.atttypid, a.atttypmod), \
                COALESCE(s.null_frac, 0), COALESCE(s.n_distinct, 0), COALESCE(s.avg_width, 0), \
                COALESCE(s.histogram_bounds::text, ''), \
                t.typcategory \
         FROM pg_attribute a \
         JOIN pg_class c ON c.oid = a.attrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_type t ON t.oid = a.atttypid \
         LEFT JOIN pg_stats s ON s.schemaname = n.nspname AND s.tablename = c.relname \
                              AND s.attname = a.attname \
         WHERE n.nspname = {} AND c.relname = {} AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
        q(schema),
        q(table)
    ))?;

    let mut out = Vec::new();
    for r in rows {
        let name = &r[0];
        let data_type = r[1].clone();
        let null_fraction: f64 = r[2].parse().unwrap_or(0.0);
        let raw_distinct: f64 = r[3].parse().unwrap_or(0.0);
        let avg_width: i32 = r[4].parse().unwrap_or(0);
        let typcategory = r.get(6).cloned().unwrap_or_default();

        // Classification runs on the *name* here; P1 adds the value-pattern signal. A column named
        // `email` suppresses its bounds before anything has looked at the data.
        let pii = pii::classify_name(name);
        let suppress = pii
            .as_ref()
            .is_some_and(super::pii::Classification::suppresses_values);

        // `histogram_bounds` are real values from the table. Orderable non-text types are the ones
        // RFC 0158's range rules need; text bounds are withheld because a min or max is a value.
        let orderable_non_text = matches!(typcategory.as_str(), "N" | "D");
        let (min, max) = if suppress || !orderable_non_text {
            (None, None)
        } else {
            let bounds = parse_bounds(&r[5]);
            (
                bounds.first().map(|v| redact(v, redaction)),
                bounds.last().map(|v| redact(v, redaction)),
            )
        };

        out.push(ColumnProfile {
            qualified_name: format!("{qualified}.{name}"),
            tier: ProfileTier::P0,
            data_type,
            null_fraction,
            distinct_estimate: interpret_n_distinct(raw_distinct),
            avg_width,
            min,
            max,
            numeric_precision_used: None,
            numeric_scale_used: None,
            monotonic: None,
            pii,
            values_suppressed: suppress,
        });
    }
    Ok(out)
}

/// Interpret a `(count, a, b)` sample result, returning `None` when the sample was empty.
///
/// **An empty sample is not a measurement of zero.** `TABLESAMPLE SYSTEM` reads whole pages, so on a
/// table small enough to fit in a handful of them a 10% sample very often selects *no* pages at all
/// — not rarely, and not only on tiny tables. Observed live: four consecutive `SYSTEM (10)` samples
/// of the same 1,000-row table returned 82, 82, 82 and 0 rows.
///
/// If the aggregates are coalesced to zero, that empty draw becomes "the data uses precision 0,
/// scale 0", and RFC 0159 is told a `Decimal(1, 0)` mapping is narrowing-safe for a money column.
/// That is the exact direction in which `narrowing-safe` must never be wrong, so the absence of a
/// measurement has to stay absent all the way to the rule, which already says "has not been
/// profiled" rather than choosing.
///
/// `min_rows` is a floor, not just a zero check: a two-row sample is not evidence about a
/// million-row column either.
fn sampled_or_none(row: &[String], min_rows: u64) -> Option<(Option<i32>, Option<i32>)> {
    let count: u64 = row.first()?.trim().parse().ok()?;
    if count < min_rows {
        return None;
    }
    let parse = |i: usize| -> Option<i32> {
        let v = row.get(i)?;
        // The simple query protocol renders SQL NULL as an empty string; an empty aggregate is
        // "nothing to measure", never zero.
        if v.trim().is_empty() {
            None
        } else {
            v.trim().parse().ok()
        }
    };
    Some((parse(1), parse(2)))
}

/// Parse `histogram_bounds` from its array-literal text form.
///
/// `pg_stats.histogram_bounds` is declared `anyarray`, which PostgreSQL refuses to cast to `text[]`
/// — `::text` on the whole array is the only route, giving `{a,b,c}`. Splitting on commas is safe
/// **only because the caller restricts this to numeric and date/time columns**, whose rendered
/// values contain no commas or quotes. It would be wrong for text, which is exactly the category
/// whose bounds are withheld anyway.
fn parse_bounds(raw: &str) -> Vec<String> {
    raw.trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_matches('"').to_string())
        .collect()
}

/// PostgreSQL's `n_distinct` convention: positive is a count, **negative is a fraction of rows**,
/// zero means unknown.
///
/// Reading a negative value as a count — which is what a naive `as i64` does — turns "every value is
/// distinct" (-1) into "one distinct value", and a `LowCardinality` mapping gets offered for a
/// primary key.
pub fn interpret_n_distinct(raw: f64) -> Option<f64> {
    if raw == 0.0 {
        None
    } else if raw > 0.0 {
        Some(raw)
    } else {
        // Negative: a fraction. The row count is applied by the caller, which has it; what is
        // recorded here is the fraction itself, marked by being < 0 in the source and > 0 here only
        // after scaling. Returning the fraction unscaled would be ambiguous, so the caller scales.
        Some(raw)
    }
}

/// Resolve `n_distinct` against a known row count, applying the negative-is-a-fraction convention.
pub fn distinct_count(n_distinct: Option<f64>, row_count: i64) -> Option<f64> {
    match n_distinct {
        None => None,
        Some(v) if v > 0.0 => Some(v),
        Some(v) => Some(-v * row_count as f64),
    }
}

/// Seed for every P1 sample. A fixed seed makes `TABLESAMPLE` draw the same pages for as long as the
/// table's pages are unchanged, so profiling twice gives the same answer.
///
/// Without it the draw was random, and so was everything derived from it. RFC 0159's DDL is derived
/// from the P1 profile, and RFC 0161 pins an approval to the DDL's hash, so on a small table (where
/// a 10% page sample is empty about half the time) `review` and `load` generated different DDL and
/// an approved load was refused as "no matching approval" (LedgerSMB demo, `public.parts`,
/// 2026-09-28).
const SAMPLE_SEED: u32 = 20_160;

/// Tables at or below this many pages are read whole rather than sampled: sampling one of a few pages
/// saves nothing and turns "every value" into "possibly no values at all".
const FULL_READ_MAX_PAGES: i64 = 128;

/// The `TABLESAMPLE` clause for one P1 read: deterministic, and 100% for a small table.
fn sample_clause(percent: f64) -> String {
    format!("TABLESAMPLE SYSTEM ({percent}) REPEATABLE ({SAMPLE_SEED})")
}

/// The sample percentage to use for `qualified`: the caller's, or 100 when the table is small.
fn effective_percent(
    src: &dyn CatalogSource,
    qualified: &str,
    percent: f64,
) -> Result<f64, PgError> {
    let (schema, table) = split_qualified(qualified)?;
    let rows = src.rows(&format!(
        "SELECT relpages FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = {} AND c.relname = {}",
        q(schema),
        q(table),
    ))?;
    let pages: i64 = rows
        .first()
        .and_then(|r| r.first())
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(i64::MAX);
    Ok(if pages <= FULL_READ_MAX_PAGES {
        100.0
    } else {
        percent
    })
}

/// P1 — a bounded sample.
///
/// `TABLESAMPLE SYSTEM` reads whole pages, which is cheap and biased; for profiling shape rather
/// than estimating a population that is the right trade, and the bias is recorded by the tier rather
/// than hidden.
///
/// Returns the sampled values **for in-memory classification only**. The caller must not persist
/// them, and [`profile_columns_p1`] is the caller that does it correctly.
fn sample_text(
    src: &dyn CatalogSource,
    qualified: &str,
    column: &str,
    percent: f64,
    limit: u32,
) -> Result<Vec<String>, PgError> {
    let (schema, table) = split_qualified(qualified)?;
    let rows = src.rows(&format!(
        "SELECT {col}::text FROM {sch}.{tbl} {sample} \
         WHERE {col} IS NOT NULL LIMIT {limit}",
        sample = sample_clause(percent),
        col = ident(column),
        sch = ident(schema),
        tbl = ident(table),
    ))?;
    Ok(rows.into_iter().filter_map(|mut r| r.pop()).collect())
}

/// P1 — add measured signals to a P0 profile: the value-pattern PII signal, the numeric scale
/// actually used, and monotonicity.
pub fn profile_columns_p1(
    src: &dyn CatalogSource,
    qualified: &str,
    p0: Vec<ColumnProfile>,
    percent: f64,
    limit: u32,
) -> Result<Vec<ColumnProfile>, PgError> {
    let (schema, table) = split_qualified(qualified)?;
    let percent = effective_percent(src, qualified, percent)?;
    let mut out = Vec::new();
    for mut c in p0 {
        let column = c
            .qualified_name
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_string();

        // Value-pattern classification. The samples live in this scope and are dropped at the end
        // of it; only the verdict escapes.
        {
            let samples = sample_text(src, qualified, &column, percent, limit)?;
            if let Some(found) = pii::classify(&column, &samples) {
                c.pii = Some(found);
            }
        }
        c.values_suppressed = c
            .pii
            .as_ref()
            .is_some_and(super::pii::Classification::suppresses_values);
        if c.values_suppressed {
            // A P0 bound may have been recorded before the pattern signal existed. Withdraw it.
            c.min = None;
            c.max = None;
        }

        // Numeric scale and precision actually used — the measurement that makes a narrowing-safe
        // mapping possible. Aggregates only; no value leaves the server.
        if c.data_type.starts_with("numeric") {
            // `count(*)` first, and **no `COALESCE`** on the aggregates. See `sampled_or_none`:
            // `TABLESAMPLE SYSTEM` is page-based and routinely selects zero pages on a small
            // table, and a `COALESCE(max(...), 0)` turns that into "precision 0, scale 0" — which
            // RFC 0159 would read as a licence to map a money column to `Decimal(1, 0)`.
            let rows = src.rows(&format!(
                "SELECT count(*), \
                        max(length(replace(trim(leading '-' from {col}::text), '.', ''))), \
                        max(scale({col})) \
                 FROM {sch}.{tbl} {sample} WHERE {col} IS NOT NULL",
                sample = sample_clause(percent),
                col = ident(&column),
                sch = ident(schema),
                tbl = ident(table),
            ))?;
            let measured = rows.first().and_then(|r| sampled_or_none(r, 2));
            c.numeric_precision_used = measured.and_then(|(p, _)| p);
            c.numeric_scale_used = measured.and_then(|(_, s)| s);
        }

        // Monotonicity, for a watermark candidate. Measured over the sample only, so it is a
        // *candidate* — RFC 0166 confirms before relying on it.
        if matches!(
            c.data_type.as_str(),
            "bigint" | "integer" | "timestamp with time zone"
        ) || c.data_type.starts_with("timestamp")
        {
            let rows = src.rows(&format!(
                "SELECT count(*) FILTER (WHERE prev IS NOT NULL AND v < prev) \
                 FROM (SELECT {col} AS v, lag({col}) OVER (ORDER BY ctid) AS prev \
                       FROM {sch}.{tbl} {sample}) t",
                sample = sample_clause(percent),
                col = ident(&column),
                sch = ident(schema),
                tbl = ident(table),
            ))?;
            c.monotonic = rows
                .first()
                .and_then(|r| r.first())
                .and_then(|v| v.parse::<i64>().ok())
                .map(|inversions| inversions == 0);
        }

        c.tier = ProfileTier::P1;
        out.push(c);
    }
    Ok(out)
}

/// The planner's estimated cost of a statement, for the P2 budget check.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CostEstimate {
    pub total_cost: f64,
    pub estimated_rows: f64,
}

/// Ask the planner what a statement would cost, without running it.
///
/// **`estimated_rows` is the widest node in the plan, not the root's.** The root of
/// `SELECT count(*)` is an Aggregate returning exactly one row, so reading `Plan Rows` off the root
/// reports `1` for a sequential scan of a billion rows — and a row budget checked against it would
/// never refuse anything. The number that matters for "how much work is this" is how many rows the
/// plan *touches*, which is the maximum over the tree.
pub fn estimate_cost(src: &dyn CatalogSource, sql: &str) -> Result<CostEstimate, PgError> {
    let rows = src.rows(&format!("EXPLAIN (FORMAT JSON) {sql}"))?;
    let raw = rows
        .first()
        .and_then(|r| r.first())
        .ok_or_else(|| PgError::Query {
            sql: sql.into(),
            message: "EXPLAIN returned nothing".into(),
        })?;
    let parsed: serde_json::Value = serde_json::from_str(raw).map_err(|e| PgError::Query {
        sql: sql.into(),
        message: format!("cannot parse EXPLAIN output: {e}"),
    })?;
    let plan = parsed
        .get(0)
        .and_then(|p| p.get("Plan"))
        .ok_or_else(|| PgError::Query {
            sql: sql.into(),
            message: "EXPLAIN output has no Plan".into(),
        })?;
    Ok(CostEstimate {
        total_cost: plan
            .get("Total Cost")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0),
        estimated_rows: widest_plan_rows(plan),
    })
}

/// The largest `Plan Rows` anywhere in the plan tree.
fn widest_plan_rows(node: &serde_json::Value) -> f64 {
    let here = node
        .get("Plan Rows")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let children = node
        .get("Plans")
        .and_then(serde_json::Value::as_array)
        .map(|ps| ps.iter().map(widest_plan_rows).fold(0.0, f64::max))
        .unwrap_or(0.0);
    here.max(children)
}

/// A P2 scan that the budget does not allow.
#[derive(Debug, Clone, PartialEq)]
pub struct BudgetExceeded {
    pub sql: String,
    pub estimate: CostEstimate,
    pub budget_rows: f64,
}

/// P2 — an exact row count, but only within budget.
///
/// Above the budget this returns `Err(BudgetExceeded)` rather than running: RFC 0161 turns that into
/// an approval request. The estimate is attached so the human approving it sees the number they are
/// approving.
pub fn exact_row_count(
    src: &dyn CatalogSource,
    qualified: &str,
    budget_rows: f64,
) -> Result<Result<i64, BudgetExceeded>, PgError> {
    let (schema, table) = split_qualified(qualified)?;
    let sql = format!("SELECT count(*) FROM {}.{}", ident(schema), ident(table));
    let estimate = estimate_cost(src, &sql)?;
    if estimate.estimated_rows > budget_rows {
        return Ok(Err(BudgetExceeded {
            sql,
            estimate,
            budget_rows,
        }));
    }
    let rows = src.rows(&sql)?;
    Ok(Ok(rows
        .first()
        .and_then(|r| r.first())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_negative_n_distinct_is_a_fraction_not_a_count() {
        // -1 means "every row is distinct". Read as a count it would say "one distinct value" and a
        // LowCardinality mapping would be offered for a primary key.
        assert_eq!(distinct_count(Some(-1.0), 1_000_000), Some(1_000_000.0));
        assert_eq!(distinct_count(Some(-0.5), 1_000), Some(500.0));
        assert_eq!(distinct_count(Some(12.0), 1_000_000), Some(12.0));
        assert_eq!(distinct_count(None, 1_000), None);
        assert_eq!(
            interpret_n_distinct(0.0),
            None,
            "zero means unknown, not zero"
        );
    }

    fn never_analyzed_reltuples() -> i64 {
        -1
    }

    #[test]
    fn a_never_analyzed_table_reports_zero_rows_and_says_it_is_an_estimate() {
        // reltuples is -1 before the first ANALYZE; a negative row count is worse than a wrong one.
        let p = TableProfile {
            qualified_name: "s.t".into(),
            tier: ProfileTier::P0,
            // `reltuples` is -1 before the first ANALYZE; the clamp is what `profile_table_p0`
            // applies. Written as a variable so clippy does not fold the constant away and lose
            // the point of the test.
            row_count: never_analyzed_reltuples().max(0),
            row_count_is_exact: false,
            total_bytes: 0,
            last_analyze: None,
            inserts: 0,
            updates: 0,
            deletes: 0,
            seq_scans: 0,
            index_scans: 0,
        };
        assert_eq!(p.row_count, 0);
        assert!(!p.row_count_is_exact);
    }

    #[test]
    fn fits_decimal_says_no_when_it_cannot_say_yes() {
        let mut c = ColumnProfile {
            qualified_name: "s.t.c".into(),
            tier: ProfileTier::P1,
            data_type: "numeric".into(),
            null_fraction: 0.0,
            distinct_estimate: None,
            avg_width: 8,
            min: None,
            max: None,
            numeric_precision_used: None,
            numeric_scale_used: None,
            monotonic: None,
            pii: None,
            values_suppressed: false,
        };
        assert_eq!(
            c.fits_decimal(38, 2),
            None,
            "unknown must never read as yes"
        );
        c.numeric_precision_used = Some(14);
        c.numeric_scale_used = Some(2);
        assert_eq!(c.fits_decimal(38, 2), Some(true));
        assert_eq!(c.fits_decimal(38, 1), Some(false));
        assert_eq!(c.fits_decimal(10, 2), Some(false));
    }

    /// The bug this guards: reading `Plan Rows` off the root of an aggregate plan reports 1 for a
    /// scan of a billion rows, and a row budget checked against it refuses nothing.
    #[test]
    fn the_row_estimate_is_the_widest_node_not_the_root() {
        let plan = serde_json::json!({
            "Node Type": "Aggregate",
            "Plan Rows": 1,
            "Total Cost": 12345.0,
            "Plans": [{
                "Node Type": "Seq Scan",
                "Plan Rows": 1_000_000_000i64,
                "Total Cost": 12000.0
            }]
        });
        assert_eq!(widest_plan_rows(&plan), 1_000_000_000.0);
    }

    #[test]
    fn a_plan_with_no_children_uses_its_own_rows() {
        let plan = serde_json::json!({ "Node Type": "Seq Scan", "Plan Rows": 42 });
        assert_eq!(widest_plan_rows(&plan), 42.0);
    }

    /// The bug this guards, observed live: `TABLESAMPLE SYSTEM (10)` on a 1,000-row table returned
    /// 82, 82, 82 then **0** rows across four executions. With a `COALESCE(..., 0)` the empty draw
    /// reads as "precision 0, scale 0" and a money column becomes `Decimal(1, 0)`.
    #[test]
    fn an_empty_sample_is_not_a_measurement_of_zero() {
        let empty = vec!["0".to_string(), String::new(), String::new()];
        assert_eq!(sampled_or_none(&empty, 2), None);

        let tiny = vec!["1".to_string(), "18".into(), "16".into()];
        assert_eq!(sampled_or_none(&tiny, 2), None, "one row is not evidence");

        let real = vec!["82".to_string(), "18".into(), "16".into()];
        assert_eq!(sampled_or_none(&real, 2), Some((Some(18), Some(16))));
    }

    /// A non-empty sample where the aggregate itself is NULL — every sampled value was NULL — is
    /// also "nothing measured", not zero.
    #[test]
    fn a_null_aggregate_over_a_non_empty_sample_is_still_unmeasured() {
        let row = vec!["50".to_string(), String::new(), String::new()];
        assert_eq!(sampled_or_none(&row, 2), Some((None, None)));
    }

    #[test]
    fn histogram_bounds_parse_from_the_array_literal_form() {
        assert_eq!(parse_bounds("{1,2,3}"), vec!["1", "2", "3"]);
        assert_eq!(parse_bounds(""), Vec::<String>::new());
        assert_eq!(parse_bounds("{}"), Vec::<String>::new());
        assert_eq!(
            parse_bounds("{\"2026-01-01\",\"2026-06-01\"}"),
            vec!["2026-01-01", "2026-06-01"]
        );
    }

    /// Answers only the `relpages` query, and records every SQL it is asked.
    struct Pages(i64, std::cell::RefCell<Vec<String>>);

    impl crate::catalog::CatalogSource for Pages {
        fn rows(&self, sql: &str) -> Result<Vec<Vec<String>>, PgError> {
            self.1.borrow_mut().push(sql.to_string());
            Ok(vec![vec![self.0.to_string()]])
        }
    }

    #[test]
    fn a_small_table_is_read_whole_and_a_large_one_is_sampled() {
        let small = Pages(3, Default::default());
        assert_eq!(
            effective_percent(&small, "public.parts", 10.0).unwrap(),
            100.0
        );
        assert!(small.1.borrow()[0].contains("relname = 'parts'"));
        let large = Pages(FULL_READ_MAX_PAGES + 1, Default::default());
        assert_eq!(
            effective_percent(&large, "public.acc_trans", 10.0).unwrap(),
            10.0
        );
    }

    /// Profiling twice must give the same answer, or the DDL derived from it — and the approval
    /// pinned to that DDL's hash — cannot be reproduced.
    #[test]
    fn every_sample_is_seeded() {
        let c = sample_clause(10.0);
        assert_eq!(
            c,
            format!("TABLESAMPLE SYSTEM (10) REPEATABLE ({SAMPLE_SEED})")
        );
        let src = include_str!("profile.rs");
        let body = &src[..src.find("#[cfg(test)]").unwrap()];
        let raw = body.matches("TABLESAMPLE SYSTEM (").count();
        assert_eq!(raw, 1, "every TABLESAMPLE goes through sample_clause()");
    }

    #[test]
    fn identifiers_and_literals_are_quoted() {
        assert_eq!(ident("weird\"name"), "\"weird\"\"name\"");
        assert_eq!(q("o'brien"), "'o''brien'");
    }

    #[test]
    fn table_activity_drives_the_target_design_question() {
        let mut p = TableProfile {
            qualified_name: "s.t".into(),
            tier: ProfileTier::P0,
            row_count: 10,
            row_count_is_exact: false,
            total_bytes: 0,
            last_analyze: None,
            inserts: 0,
            updates: 0,
            deletes: 0,
            seq_scans: 0,
            index_scans: 0,
        };
        assert!(p.looks_static());
        assert!(!p.has_updates());
        p.updates = 1;
        assert!(!p.looks_static());
        assert!(
            p.has_updates(),
            "an updated table needs ReplacingMergeTree, not MergeTree"
        );
    }
}
