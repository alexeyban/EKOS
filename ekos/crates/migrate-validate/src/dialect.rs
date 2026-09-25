//! RFC 0155 — the per-dialect SQL that reproduces the canonical form inside the engine.
//!
//! The Rust implementation in [`crate::canon`] is used on the bisect path, where rows are pulled
//! into the validator. Tiers V1–V3 push the work down to the engines instead, because pulling a
//! hundred million rows across the network to hash them is not a validation strategy.
//!
//! Both paths must produce the same bytes, so these expressions are generated from one table
//! rather than written per call site, and the golden tests assert the engine output against the
//! same committed literals the Rust implementation is asserted against.

use crate::canon::UNIT_SEPARATOR;

/// Which engine an expression is being generated for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Postgres,
    ClickHouse,
}

/// The source column type an expression is generated for. Deliberately coarse: it names the
/// *rule* being applied, not the engine's full type lattice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnRule {
    Text,
    Char,
    Bool,
    Int,
    /// Carries the approved scale, because the mapping decides the rendering, not the value.
    Decimal(u32),
    TimestampUtc,
    TimestampNaive,
    Date,
    Time,
    Uuid,
    Bytes,
    Inet,
}

/// The null sentinel **as a SQL string literal**, which is not the same text on both engines.
///
/// ClickHouse treats `\N` inside a string literal as its NULL escape: `SELECT '\N'` returns NULL,
/// not the two characters. So the sentinel has to be written `'\\N'` there, while PostgreSQL —
/// with `standard_conforming_strings` on, the default since 9.1 — takes `'\N'` literally.
///
/// Caught by the live harness, not by review: every unit test passed and the ClickHouse side
/// silently rendered NULL for *every* null column, which would have made every row containing a
/// null hash differently on the two sides. A total mismatch rather than a subtle one, but only
/// because nothing else depended on it.
fn null_literal(d: Dialect) -> &'static str {
    match d {
        Dialect::Postgres => "'\\N'",
        Dialect::ClickHouse => "'\\\\N'",
    }
}

fn quote_ident(d: Dialect, name: &str) -> String {
    // Both engines use double quotes and both escape an embedded quote by doubling it. Identifiers
    // are never concatenated raw: RFC 0160's rule, applied here too.
    let escaped = name.replace('"', "\"\"");
    let _ = d;
    format!("\"{escaped}\"")
}

/// Escape backslashes and the unit separator inside the engine, matching [`crate::canon`]'s
/// `escape`.
fn escape_sql(d: Dialect, inner: &str) -> String {
    let us = format!("chr({})", UNIT_SEPARATOR as u32);
    match d {
        Dialect::Postgres => format!(
            "replace(replace({inner}, '\\', '\\\\'), {us}, '\\x1f')",
            us = us
        ),
        Dialect::ClickHouse => {
            format!("replaceAll(replaceAll({inner}, '\\\\', '\\\\\\\\'), char(31), '\\\\x1f')")
        }
    }
}

/// The canonical-form expression for one column.
pub fn canon_expr(d: Dialect, column: &str, rule: ColumnRule) -> String {
    canon_expr_of(d, &quote_ident(d, column), rule)
}

/// The canonical-form expression over an arbitrary sub-expression rather than a column.
///
/// Exists so the live cross-engine harness can apply exactly the same rendering to a *literal*,
/// which is what makes it a real check: if the harness rebuilt the expression itself, it would be
/// testing its own copy rather than the one tiers V1–V3 actually push down.
pub fn canon_expr_of(d: Dialect, expr: &str, rule: ColumnRule) -> String {
    let c = expr.to_string();
    let body = match (d, rule) {
        (Dialect::Postgres, ColumnRule::Text | ColumnRule::Char | ColumnRule::Inet) => {
            escape_sql(d, &format!("{c}::text"))
        }
        (Dialect::ClickHouse, ColumnRule::Text | ColumnRule::Char | ColumnRule::Inet) => {
            escape_sql(d, &format!("toString({c})"))
        }
        (Dialect::Postgres, ColumnRule::Bool) => format!("(case when {c} then 't' else 'f' end)"),
        (Dialect::ClickHouse, ColumnRule::Bool) => format!("if({c}, 't', 'f')"),
        (Dialect::Postgres, ColumnRule::Int) => format!("{c}::text"),
        (Dialect::ClickHouse, ColumnRule::Int) => format!("toString({c})"),
        // `to_char` would apply locale-dependent grouping; a plain cast of an already-rescaled
        // numeric gives the fixed-scale, exponent-free form the rule requires.
        (Dialect::Postgres, ColumnRule::Decimal(s)) => format!(
            "trim(to_char({c}, 'FM9999999999999999999990.{}'))",
            "0".repeat(s as usize)
        ),
        // `toString` on a Decimal **strips trailing zeros** (1.50 renders as "1.5"), which breaks
        // the fixed-scale rule and makes every decimal disagree with PostgreSQL.
        // `toDecimalString` renders at an exact scale, and normalizes -0.00 to 0.00 the way
        // `canon_decimal` does. Caught by the live harness, not by review.
        (Dialect::ClickHouse, ColumnRule::Decimal(s)) => {
            format!("toDecimalString(toDecimal128({c}, {s}), {s})")
        }
        (Dialect::Postgres, ColumnRule::TimestampUtc) => {
            format!("to_char({c} at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US')")
        }
        (Dialect::ClickHouse, ColumnRule::TimestampUtc) => {
            format!("formatDateTime(toTimeZone({c}, 'UTC'), '%Y-%m-%dT%H:%i:%S.%f')")
        }
        (Dialect::Postgres, ColumnRule::TimestampNaive) => {
            format!("to_char({c}, 'YYYY-MM-DD\"T\"HH24:MI:SS.US')")
        }
        (Dialect::ClickHouse, ColumnRule::TimestampNaive) => {
            format!("formatDateTime({c}, '%Y-%m-%dT%H:%i:%S.%f')")
        }
        (Dialect::Postgres, ColumnRule::Date) => format!("to_char({c}, 'YYYY-MM-DD')"),
        (Dialect::ClickHouse, ColumnRule::Date) => format!("formatDateTime({c}, '%Y-%m-%d')"),
        (Dialect::Postgres, ColumnRule::Time) => format!("to_char({c}, 'HH24:MI:SS.US')"),
        (Dialect::ClickHouse, ColumnRule::Time) => format!("formatDateTime({c}, '%H:%i:%S.%f')"),
        (Dialect::Postgres, ColumnRule::Uuid) => format!("lower({c}::text)"),
        (Dialect::ClickHouse, ColumnRule::Uuid) => format!("lower(toString({c}))"),
        (Dialect::Postgres, ColumnRule::Bytes) => format!("encode({c}, 'hex')"),
        (Dialect::ClickHouse, ColumnRule::Bytes) => format!("lower(hex({c}))"),
    };
    // NULL is applied last, over the rendered value, so no rule has to handle it itself.
    let null = null_literal(d);
    match d {
        Dialect::Postgres => format!("coalesce({body}, {null})"),
        Dialect::ClickHouse => format!("if(isNull({c}), {null}, {body})"),
    }
}

/// The row-hash expression over a row's column expressions.
pub fn row_hash_expr(d: Dialect, column_exprs: &[String]) -> String {
    let sep = match d {
        Dialect::Postgres => format!("chr({})", UNIT_SEPARATOR as u32),
        Dialect::ClickHouse => "char(31)".to_string(),
    };
    let joined = column_exprs.join(&format!(" || {sep} || "));
    match d {
        Dialect::Postgres => format!("md5({joined})"),
        Dialect::ClickHouse => format!(
            "lower(hex(MD5({})))",
            column_exprs.join(&format!(" || {sep} || "))
        ),
    }
}

/// The 60-bit prefix of a hash expression, as an exact integer.
///
/// **Endianness is the trap here.** PostgreSQL's `('x' || …)::bit(60)::bigint` reads the hex
/// big-endian, the way a human reads it. ClickHouse's `reinterpretAsUInt64` reads the underlying
/// bytes **little-endian**, so the naive translation produces a different integer from the same
/// hash and every bucket disagrees — a total, immediate mismatch rather than a subtle one, which
/// is at least the good kind of wrong. `reverse()` on the unhexed bytes makes it big-endian too.
///
/// The 15 hex characters are padded to 16 with a leading `'0'` so `unhex` sees a whole number of
/// bytes; the padding nibble is the high nibble, which keeps the value under 2^60.
///
/// **Unverified against a live engine.** See the crate docs: nothing in this workspace can execute
/// these until RFC 0157 adds a driver, so this reasoning is careful but untested.
pub fn prefix60_expr(d: Dialect, hash_expr: &str) -> String {
    match d {
        Dialect::Postgres => format!("(('x' || substr({hash_expr}, 1, 15))::bit(60))::bigint"),
        Dialect::ClickHouse => format!(
            "reinterpretAsUInt64(reverse(unhex(concat('0', substring({hash_expr}, 1, 15)))))"
        ),
    }
}

/// The full per-bucket checksum query for one table.
///
/// The sum is taken in exact arithmetic on both sides — `numeric` in PostgreSQL, `Decimal128` in
/// ClickHouse. ClickHouse's `sumWithOverflow` is deliberately **not** used: a silently wrapping
/// checksum is a false green.
pub fn bucket_checksum_query(
    d: Dialect,
    table: &str,
    pk_expr: &str,
    column_exprs: &[String],
    buckets: u32,
) -> String {
    let row = row_hash_expr(d, column_exprs);
    let pk_hash = match d {
        Dialect::Postgres => format!("md5({pk_expr})"),
        Dialect::ClickHouse => format!("lower(hex(MD5({pk_expr})))"),
    };
    let bucket = format!("{} % {buckets}", prefix60_expr(d, &pk_hash));
    let sum = match d {
        Dialect::Postgres => format!("sum(({})::numeric)", prefix60_expr(d, &row)),
        Dialect::ClickHouse => format!("sum(toDecimal128({}, 0))", prefix60_expr(d, &row)),
    };
    format!(
        "SELECT {bucket} AS bucket, count(*) AS row_count, {sum} AS hash_sum \
         FROM {table} GROUP BY bucket ORDER BY bucket"
    )
}

/// The bucket expression on its own, so bisect can both group by it and filter on it.
pub fn bucket_expr(d: Dialect, pk_expr: &str, buckets: u32) -> String {
    let pk_hash = match d {
        Dialect::Postgres => format!("md5({pk_expr})"),
        Dialect::ClickHouse => format!("lower(hex(MD5({pk_expr})))"),
    };
    format!("{} % {buckets}", prefix60_expr(d, &pk_hash))
}

/// RFC 0156 bisect step: re-bucket one coarse bucket's rows at a finer fan-out.
///
/// The coarse predicate goes in `WHERE` rather than `HAVING` so the engine can discard rows before
/// aggregating — on a large table that difference is the whole cost of the step.
pub fn sub_bucket_checksum_query(
    d: Dialect,
    table: &str,
    pk_expr: &str,
    column_exprs: &[String],
    coarse_buckets: u32,
    coarse_bucket: u32,
    fine_buckets: u32,
) -> String {
    let row = row_hash_expr(d, column_exprs);
    let coarse = bucket_expr(d, pk_expr, coarse_buckets);
    let fine = bucket_expr(d, pk_expr, fine_buckets);
    let sum = match d {
        Dialect::Postgres => format!("sum(({})::numeric)", prefix60_expr(d, &row)),
        Dialect::ClickHouse => format!("sum(toDecimal128({}, 0))", prefix60_expr(d, &row)),
    };
    format!(
        "SELECT {fine} AS bucket, count(*) AS row_count, {sum} AS hash_sum \
         FROM {table} WHERE {coarse} = {coarse_bucket} GROUP BY bucket ORDER BY bucket"
    )
}

/// RFC 0156 bisect endpoint: the `(pk, row_hash)` pairs of one bucket.
///
/// This is the only query in the tier stack that returns per-row data, and it is bounded by the
/// bisect threshold before it is ever issued. Full rows are fetched separately, for divergent keys
/// only, and are never persisted (RFC 0154's ledger-scan test).
pub fn key_hash_query(
    d: Dialect,
    table: &str,
    pk_expr: &str,
    column_exprs: &[String],
    buckets: u32,
    bucket: u32,
) -> String {
    let row = row_hash_expr(d, column_exprs);
    let b = bucket_expr(d, pk_expr, buckets);
    format!(
        "SELECT {pk_expr} AS pk, {row} AS row_hash \
         FROM {table} WHERE {b} = {bucket} ORDER BY pk"
    )
}

/// RFC 0156 V1: the row count.
pub fn count_query(d: Dialect, table: &str) -> String {
    let _ = d;
    format!("SELECT count(*) FROM {table}")
}

/// RFC 0156 V2: the per-column aggregate pack.
///
/// Every aggregate is taken over the *canonical* form, not the raw column, so a V2 disagreement
/// means the same thing a V3 disagreement means and the two tiers cannot contradict each other over
/// a rendering difference. `min`/`max` over canonical text also side-steps collation: the canonical
/// form is binary-comparable by construction.
pub fn aggregate_query(d: Dialect, table: &str, column_exprs: &[String]) -> String {
    let null = match d {
        Dialect::Postgres => "'\\N'",
        Dialect::ClickHouse => "'\\\\N'",
    };
    let mut parts = Vec::new();
    for (i, e) in column_exprs.iter().enumerate() {
        parts.push(format!(
            "sum(case when {e} = {null} then 1 else 0 end) AS nulls_{i}"
        ));
        parts.push(format!("min({e}) AS min_{i}"));
        parts.push(format!("max({e}) AS max_{i}"));
        parts.push(format!("sum(length({e})) AS len_{i}"));
    }
    // ClickHouse has no `case when` shorthand difference here, but `sum(case when …)` is valid in
    // both, so one form serves both dialects and there is one less place to diverge.
    format!("SELECT {} FROM {table}", parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bisect_filters_in_where_not_having() {
        let q = sub_bucket_checksum_query(Dialect::Postgres, "t", "pk", &["a".into()], 16, 3, 256);
        assert!(q.contains("WHERE"), "{q}");
        assert!(
            !q.contains("HAVING"),
            "a HAVING filter would aggregate every row first: {q}"
        );
        assert!(q.contains("% 256 AS bucket"), "{q}");
        assert!(q.contains("% 16 = 3"), "{q}");
    }

    #[test]
    fn the_key_hash_query_is_the_only_one_returning_rows() {
        let q = key_hash_query(Dialect::ClickHouse, "t", "pk", &["a".into()], 16, 3);
        assert!(q.contains("AS row_hash"), "{q}");
        assert!(!q.contains("count(*)"), "{q}");
        assert!(q.contains("WHERE"), "bisect must bound the row set: {q}");
    }

    #[test]
    fn aggregates_are_taken_over_the_canonical_form() {
        for d in [Dialect::Postgres, Dialect::ClickHouse] {
            let e = canon_expr(d, "c", ColumnRule::Text);
            let q = aggregate_query(d, "t", std::slice::from_ref(&e));
            assert!(
                q.contains(&e),
                "{d:?} aggregated the raw column, not the canonical form"
            );
            for agg in ["nulls_0", "min_0", "max_0", "len_0"] {
                assert!(q.contains(agg), "{d:?} missing {agg}: {q}");
            }
        }
    }

    #[test]
    fn null_handling_wraps_every_rule() {
        for d in [Dialect::Postgres, Dialect::ClickHouse] {
            for rule in [
                ColumnRule::Text,
                ColumnRule::Int,
                ColumnRule::Date,
                ColumnRule::Uuid,
            ] {
                let e = canon_expr(d, "c", rule);
                assert!(
                    e.contains("\\N"),
                    "{d:?}/{rule:?} lost the null sentinel: {e}"
                );
            }
        }
    }

    #[test]
    fn identifiers_are_quoted_not_concatenated() {
        let e = canon_expr(Dialect::Postgres, "weird\"name", ColumnRule::Text);
        assert!(e.contains("\"weird\"\"name\""), "{e}");
    }

    #[test]
    fn decimal_expressions_carry_the_approved_scale() {
        let ch = canon_expr(Dialect::ClickHouse, "amt", ColumnRule::Decimal(2));
        assert!(
            ch.contains("toDecimalString"),
            "toString strips trailing zeros: {ch}"
        );
        assert!(ch.contains(", 2)"), "{ch}");
        assert!(canon_expr(Dialect::Postgres, "amt", ColumnRule::Decimal(2)).contains(".00"));
    }

    #[test]
    fn the_checksum_query_never_uses_overflowing_arithmetic() {
        let q = bucket_checksum_query(
            Dialect::ClickHouse,
            "db.t",
            "toString(id)",
            &["toString(a)".into()],
            4096,
        );
        assert!(!q.contains("sumWithOverflow"), "{q}");
        assert!(q.contains("toDecimal128"), "{q}");
        assert!(q.contains("% 4096"), "{q}");
    }

    /// Guards the endianness fix: ClickHouse's `reinterpretAsUInt64` is little-endian, so the
    /// bytes must be reversed to match PostgreSQL's big-endian hex read. Removing `reverse()`
    /// makes every bucket disagree.
    #[test]
    fn clickhouse_reads_the_hash_prefix_big_endian() {
        let e = prefix60_expr(Dialect::ClickHouse, "h");
        assert!(
            e.contains("reverse("),
            "little-endian read would mismatch PostgreSQL: {e}"
        );
        assert!(
            e.contains("concat('0'"),
            "15 hex chars must be padded to a whole byte: {e}"
        );
    }

    #[test]
    fn both_dialects_produce_a_grouped_bucket_query() {
        for d in [Dialect::Postgres, Dialect::ClickHouse] {
            let q = bucket_checksum_query(d, "t", "pk", &["a".into(), "b".into()], 16);
            assert!(q.contains("GROUP BY bucket"), "{d:?}: {q}");
            assert!(q.contains("count(*)"), "{d:?}: {q}");
        }
    }
}
