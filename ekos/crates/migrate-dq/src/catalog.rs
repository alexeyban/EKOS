//! RFC 0158 — the rule catalog.
//!
//! Each entry measures **affected rows**, not just types. That distinction is the whole point:
//! "this column is `numeric` and ClickHouse needs a precision" flags every `numeric` column in the
//! database and buries the three that actually overflow. "Seventeen rows in one table exceed
//! precision 38, here is the query" turns a blocking unknown into a five-minute decision.

use crate::model::{ColumnContext, Family, Lossiness, Rule, Severity, TableContext, Target};

fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// `schema.table` → `"schema"."table"`, so a generated statement never concatenates a raw
/// identifier (RFC 0160's rule, applied upstream of the classifier).
fn qualify(table: &str) -> String {
    table.split('.').map(ident).collect::<Vec<_>>().join(".")
}

fn count_where(c: &ColumnContext, predicate: &str) -> Option<String> {
    Some(format!(
        "SELECT count(*) FROM {} WHERE {}",
        qualify(&c.table),
        predicate
    ))
}

// ── data-quality rules ───────────────────────────────────────────────────────

fn applies_text(c: &ColumnContext) -> bool {
    matches!(
        c.base_type(),
        "text" | "character varying" | "varchar" | "character" | "char"
    )
}

fn applies_nullable_text(c: &ColumnContext) -> bool {
    applies_text(c) && c.nullable
}

/// An empty string used where NULL was meant. The two are different in every target, and a source
/// that mixes them migrates the mixture.
const DQ_EMPTY_AS_NULL: Rule = Rule {
    id: "DQ.COMPLETE.001",
    title: "empty string used as NULL",
    family: Family::Completeness,
    severity: Severity::Warn,
    target: None,
    lossiness: None,
    applies: applies_nullable_text,
    measure: |c| count_where(c, &format!("{} = ''", ident(&c.column))),
    explain: |c| {
        format!(
            "{} is nullable text and also contains empty strings; the two mean different things in \
             every target",
            c.column
        )
    },
};

/// A sentinel date standing in for NULL. `9999-12-31` is also outside ClickHouse's `Date` range, so
/// this one is a data-quality problem that becomes a compatibility problem.
const DQ_SENTINEL_DATE: Rule = Rule {
    id: "DQ.COMPLETE.002",
    title: "sentinel date standing in for NULL",
    family: Family::Completeness,
    severity: Severity::Warn,
    target: None,
    lossiness: None,
    applies: |c| {
        matches!(
            c.base_type(),
            "date" | "timestamp without time zone" | "timestamp with time zone"
        )
    },
    measure: |c| {
        count_where(
            c,
            &format!(
                "{col} IN (DATE '0001-01-01', DATE '1900-01-01', DATE '9999-12-31')",
                col = ident(&c.column)
            ),
        )
    },
    explain: |c| {
        format!(
            "{} uses a sentinel date where NULL was likely meant",
            c.column
        )
    },
};

/// Duplicates on a column the schema treats as unique-ish. Measured, because "the application
/// guarantees it" is a claim and this is the cheapest way to check it.
const DQ_DUPLICATE_KEY: Rule = Rule {
    id: "DQ.UNIQ.001",
    title: "duplicate values in a key-shaped column",
    family: Family::Uniqueness,
    severity: Severity::Blocking,
    target: None,
    lossiness: None,
    applies: |c| {
        let n = c.column.to_ascii_lowercase();
        (n.ends_with("_id") || n == "id" || n.ends_with("_key") || n.ends_with("_code"))
            && c.distinct_estimate.is_some()
    },
    measure: |c| {
        Some(format!(
            "SELECT COALESCE(sum(n - 1), 0) FROM (SELECT count(*) AS n FROM {t} \
             WHERE {col} IS NOT NULL GROUP BY {col} HAVING count(*) > 1) d",
            t = qualify(&c.table),
            col = ident(&c.column)
        ))
    },
    explain: |c| {
        format!(
            "{} looks like a key and is not declared unique; duplicates would be silently \
             deduplicated by a ReplacingMergeTree target",
            c.column
        )
    },
};

/// JSON that will not parse. A `text` column holding JSON is a common shape, and the target will
/// either reject it on load or accept it as opaque text.
const DQ_MALFORMED_JSON: Rule = Rule {
    id: "DQ.VALID.001",
    title: "malformed JSON in a text column",
    family: Family::Validity,
    severity: Severity::Warn,
    target: None,
    lossiness: None,
    applies: |c| {
        applies_text(c) && {
            let n = c.column.to_ascii_lowercase();
            n.contains("json") || n.contains("payload") || n.contains("metadata")
        }
    },
    measure: |c| {
        count_where(
            c,
            &format!(
                "{col} IS NOT NULL AND {col} <> '' AND NOT ({col} ~ '^\\s*[\\[{{]')",
                col = ident(&c.column)
            ),
        )
    },
    explain: |c| {
        format!(
            "{} is named like JSON but holds values that are not",
            c.column
        )
    },
};

// ── ClickHouse compatibility rules ───────────────────────────────────────────

/// An unconstrained `numeric` has no precision for the target to use. The profile answers it — and
/// when the profile is absent, the rule says so rather than assuming a comfortable default.
const CH_UNCONSTRAINED_NUMERIC: Rule = Rule {
    id: "COMPAT.CH.NUMERIC_UNCONSTRAINED",
    title: "unconstrained numeric needs a chosen precision and scale",
    family: Family::Compatibility,
    severity: Severity::Blocking,
    target: Some(Target::ClickHouse),
    lossiness: Some(Lossiness::Lossy),
    applies: ColumnContext::is_unconstrained_numeric,
    measure: |_| None,
    explain: |c| match (c.numeric_precision_used, c.numeric_scale_used) {
        (Some(p), Some(s)) => format!(
            "{} is unconstrained numeric; the measured data uses precision {p} scale {s}, so \
             Decimal({}, {s}) is narrowing-safe against that profile",
            c.column,
            p.max(s + 1)
        ),
        _ => format!(
            "{} is unconstrained numeric and has not been profiled. Run `ekos migrate profile \
             --tier p1` before choosing a target type; guessing one is how a migration truncates \
             money.",
            c.column
        ),
    },
};

/// Beyond ClickHouse's `Decimal128`/`Decimal256` reach.
const CH_NUMERIC_PRECISION: Rule = Rule {
    id: "COMPAT.CH.NUMERIC_PRECISION",
    title: "numeric precision beyond the target's range",
    family: Family::Compatibility,
    severity: Severity::Blocking,
    target: Some(Target::ClickHouse),
    lossiness: Some(Lossiness::Lossy),
    applies: |c| c.type_params().is_some_and(|(p, _)| p > 76),
    measure: |_| None,
    explain: |c| {
        format!(
            "{} declares precision {}, beyond ClickHouse's maximum of 76",
            c.column,
            c.type_params().map(|(p, _)| p).unwrap_or_default()
        )
    },
};

/// Dates ClickHouse cannot hold. Verified live on 24.8: `toDate32('1850-06-15')` returns
/// `1900-01-01` — it **clamps silently**, with no error, which is precisely why this rule has to
/// count the rows before anything moves.
const CH_DATE_RANGE: Rule = Rule {
    id: "COMPAT.CH.DATE_BEFORE_1900",
    title: "dates before 1900 are silently clamped by ClickHouse",
    family: Family::Compatibility,
    severity: Severity::Blocking,
    target: Some(Target::ClickHouse),
    lossiness: Some(Lossiness::Lossy),
    applies: |c| {
        matches!(
            c.base_type(),
            "date" | "timestamp without time zone" | "timestamp with time zone"
        )
    },
    measure: |c| count_where(c, &format!("{} < DATE '1900-01-01'", ident(&c.column))),
    explain: |c| {
        format!(
            "{} holds dates before 1900; ClickHouse Date32 bottoms out at 1900-01-01 and clamps to \
             it without an error (verified on 24.8)",
            c.column
        )
    },
};

/// `±infinity` timestamps have no representation at all in either target.
const CH_INFINITE_TIMESTAMP: Rule = Rule {
    id: "COMPAT.CH.INFINITE_TIMESTAMP",
    title: "infinite timestamps have no target representation",
    family: Family::Compatibility,
    severity: Severity::Blocking,
    target: Some(Target::ClickHouse),
    lossiness: Some(Lossiness::Lossy),
    applies: |c| c.base_type().starts_with("timestamp"),
    measure: |c| {
        count_where(
            c,
            &format!(
                "{col} = 'infinity'::timestamptz OR {col} = '-infinity'::timestamptz",
                col = ident(&c.column)
            ),
        )
    },
    explain: |c| {
        format!(
            "{} holds ±infinity, which no target can represent",
            c.column
        )
    },
};

/// `char(n)` padding. RFC 0155 preserves it in the canonical form precisely so that a target which
/// trims it shows up as a divergence rather than passing silently.
const CH_CHAR_PADDING: Rule = Rule {
    id: "COMPAT.CH.CHAR_PADDING",
    title: "char(n) padding semantics differ",
    family: Family::Compatibility,
    severity: Severity::Warn,
    target: Some(Target::ClickHouse),
    lossiness: Some(Lossiness::Behavioural),
    applies: |c| matches!(c.base_type(), "character" | "char" | "bpchar"),
    // `{col}::text` does **not** work here: PostgreSQL strips a `bpchar`'s padding on the cast, so
    // the obvious `col::text <> rtrim(col::text)` is always false and the rule silently measures
    // zero. `octet_length` on the column sees the stored, padded value; on the cast it sees the
    // trimmed one. Verified live: 20 of 1,000 padded rows, which the obvious form reported as 0.
    measure: |c| {
        count_where(
            c,
            &format!(
                "{col} IS NOT NULL AND octet_length({col}) <> octet_length({col}::text)",
                col = ident(&c.column)
            ),
        )
    },
    explain: |c| {
        format!(
            "{} is char(n); PostgreSQL pads to the declared width and ClickHouse does not, so \
             trailing spaces are a real difference",
            c.column
        )
    },
};

/// Nullability has a cost in ClickHouse, and removing it is a narrowing that the profile must
/// justify.
const CH_NULLABLE_COST: Rule = Rule {
    id: "COMPAT.CH.NULLABLE",
    title: "nullable column carries Nullable(T) overhead",
    family: Family::Compatibility,
    severity: Severity::Info,
    target: Some(Target::ClickHouse),
    lossiness: Some(Lossiness::Exact),
    applies: |c| c.nullable && c.null_fraction == Some(0.0),
    measure: |c| count_where(c, &format!("{} IS NULL", ident(&c.column))),
    explain: |c| {
        format!(
            "{} is declared nullable but the profile found no nulls; dropping Nullable(T) is a \
             narrowing that only holds while that stays true",
            c.column
        )
    },
};

/// The one people skip. Nothing is corrupted on load day.
pub const CH_NO_UNIQUENESS: TableRule = TableRule {
    id: "COMPAT.CH.NO_CONSTRAINT_ENFORCEMENT",
    title: "the target enforces no uniqueness or referential integrity",
    family: Family::Compatibility,
    severity: Severity::Blocking,
    target: Some(Target::ClickHouse),
    lossiness: Some(Lossiness::Behavioural),
    applies: |t| !t.primary_key_columns.is_empty(),
    explain: |t| {
        format!(
            "{} has a primary key that ClickHouse will not enforce. Nothing breaks on load day; \
             duplicates accumulate afterwards, and a ReplacingMergeTree deduplicates only \
             eventually — a SELECT before a merge still returns them.",
            t.table
        )
    },
};

/// A constraint that is present and not a guarantee.
pub const DQ_UNVALIDATED_CONSTRAINT: TableRule = TableRule {
    id: "DQ.REFINT.001",
    title: "constraint declared but never validated",
    family: Family::ReferentialIntegrity,
    severity: Severity::Blocking,
    target: None,
    lossiness: None,
    applies: |t| !t.unvalidated_constraints.is_empty(),
    explain: |t| {
        format!(
            "{} has {} NOT VALID constraint(s) ({}). The rows that existed when they were added \
             were never checked, so the constraint is not evidence the data satisfies it.",
            t.table,
            t.unvalidated_constraints.len(),
            t.unvalidated_constraints.join(", ")
        )
    },
};

/// A table nothing has written to. A candidate for archiving rather than migrating — and a
/// *candidate*, because statistics reset on a restart.
pub const DQ_STALE_TABLE: TableRule = TableRule {
    id: "DQ.TIMELY.001",
    title: "table has no recorded writes",
    family: Family::Timeliness,
    severity: Severity::Info,
    target: None,
    lossiness: None,
    applies: |t| t.looks_static && t.row_count > 0,
    explain: |t| {
        format!(
            "{} has no inserts, updates or deletes since statistics were last reset — a candidate \
             for archiving rather than migrating. Statistics reset on restart, so confirm before \
             acting.",
            t.table
        )
    },
};

/// A rule about a table rather than a column.
pub struct TableRule {
    pub id: &'static str,
    pub title: &'static str,
    pub family: Family,
    pub severity: Severity,
    pub target: Option<Target>,
    pub lossiness: Option<Lossiness>,
    pub applies: fn(&TableContext) -> bool,
    pub explain: fn(&TableContext) -> String,
}

/// Every column rule.
pub const COLUMN_RULES: &[Rule] = &[
    DQ_EMPTY_AS_NULL,
    DQ_SENTINEL_DATE,
    DQ_DUPLICATE_KEY,
    DQ_MALFORMED_JSON,
    CH_UNCONSTRAINED_NUMERIC,
    CH_NUMERIC_PRECISION,
    CH_DATE_RANGE,
    CH_INFINITE_TIMESTAMP,
    CH_CHAR_PADDING,
    CH_NULLABLE_COST,
];

/// Every table rule.
pub const TABLE_RULES: &[TableRule] =
    &[CH_NO_UNIQUENESS, DQ_UNVALIDATED_CONSTRAINT, DQ_STALE_TABLE];

/// Look a rule up by id, for a finding that needs to explain itself.
pub fn rule_by_id(id: &str) -> Option<&'static Rule> {
    COLUMN_RULES.iter().find(|r| r.id == id)
}
