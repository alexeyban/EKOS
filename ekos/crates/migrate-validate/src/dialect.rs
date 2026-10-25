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
    /// RFC 0190: Trino reading Iceberg tables (EKOS Lakehouse, source ↔ bronze reconciliation).
    /// Pinned by the SQL snapshot here. Agreement with the golden canonical forms, the Rust
    /// hashes and bucket checksums is proven live in the `ekos-lakehouse` repository, which has
    /// the Trino client (`EKOS_LAKEHOUSE_LIVE=1`; 31/31 on Trino 483, 2026-10-16).
    Trino,
    /// RFC 0216: Oracle as a lakehouse source (EKOS Lakehouse, source ↔ bronze reconciliation).
    /// Every expression assumes the session `ekos-ora-live` opens: NLS settings pinned (numeric
    /// characters `.,`, binary comparison), time zone UTC, and a database character set of
    /// AL32UTF8, because `STANDARD_HASH` hashes a string's bytes in the database character set.
    Oracle,
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
        // Trino string literals have no backslash escapes, as PostgreSQL's with
        // `standard_conforming_strings`.
        Dialect::Postgres | Dialect::Trino | Dialect::Oracle => "'\\N'",
        Dialect::ClickHouse => "'\\\\N'",
    }
}

fn quote_ident(d: Dialect, name: &str) -> String {
    // Every engine uses double quotes and escapes an embedded quote by doubling it. Identifiers
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
        // Trino's `replace(s, search, replacement)` and `chr(n)` match PostgreSQL's, and its
        // literals take a backslash literally, so the text is the same.
        Dialect::Postgres | Dialect::Trino => format!(
            "replace(replace({inner}, '\\', '\\\\'), {us}, '\\x1f')",
            us = us
        ),
        Dialect::ClickHouse => {
            format!("replaceAll(replaceAll({inner}, '\\\\', '\\\\\\\\'), char(31), '\\\\x1f')")
        }
        // Oracle literals take a backslash literally too.
        Dialect::Oracle => format!("REPLACE(REPLACE({inner}, '\\', '\\\\'), CHR(31), '\\x1f')"),
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
        (Dialect::Trino, ColumnRule::Text | ColumnRule::Char | ColumnRule::Inet) => {
            escape_sql(d, &format!("CAST({c} AS varchar)"))
        }
        (Dialect::Trino, ColumnRule::Bool) => format!("(CASE WHEN {c} THEN 't' ELSE 'f' END)"),
        (Dialect::Trino, ColumnRule::Int) => format!("CAST({c} AS varchar)"),
        // A Trino decimal renders at its own scale, with a leading `0` and no exponent
        // (`0.05`, `1.50`), so rescaling to the approved scale first gives the fixed-scale form.
        (Dialect::Trino, ColumnRule::Decimal(s)) => {
            format!("CAST(CAST({c} AS decimal(38, {s})) AS varchar)")
        }
        // `format_datetime` is Joda-based and stops at milliseconds; `to_iso8601` of a
        // `timestamp(6)` prints all six fraction digits and, for a value without a zone, no
        // offset. Iceberg `timestamptz` reads as `timestamp(6) with time zone`.
        (Dialect::Trino, ColumnRule::TimestampUtc) => {
            format!("to_iso8601(CAST({c} AT TIME ZONE 'UTC' AS timestamp(6)))")
        }
        (Dialect::Trino, ColumnRule::TimestampNaive) => {
            format!("to_iso8601(CAST({c} AS timestamp(6)))")
        }
        (Dialect::Trino, ColumnRule::Date) => format!("CAST({c} AS varchar)"),
        (Dialect::Trino, ColumnRule::Time) => format!("CAST(CAST({c} AS time(6)) AS varchar)"),
        (Dialect::Trino, ColumnRule::Uuid) => format!("lower(CAST({c} AS varchar))"),
        (Dialect::Trino, ColumnRule::Bytes) => format!("lower(to_hex({c}))"),
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
        // `TO_CHAR` brings an NVARCHAR2 into the database character set, so its bytes hash as
        // UTF-8 like every other text.
        (Dialect::Oracle, ColumnRule::Text | ColumnRule::Char | ColumnRule::Inet) => {
            escape_sql(d, &format!("TO_CHAR({c})"))
        }
        // Oracle tables have no boolean before 23ai: a NUMBER(1) flag.
        (Dialect::Oracle, ColumnRule::Bool) => format!("(CASE WHEN {c} = 1 THEN 't' ELSE 'f' END)"),
        (Dialect::Oracle, ColumnRule::Int) => format!("TO_CHAR({c})"),
        // `FM` drops the padding; the `0` before the point keeps `0.05`'s leading zero. A value
        // with more digits than the scale renders unrounded, so it cannot match the target's
        // fixed-scale form: `TO_CHAR` would round it exactly as a lossy load's cast does, and the
        // two sides would agree over the lost digits.
        (Dialect::Oracle, ColumnRule::Decimal(s)) => format!(
            "(CASE WHEN {c} <> ROUND({c}, {s}) THEN TO_CHAR({c}) ELSE TO_CHAR({c}, '{}') END)",
            oracle_decimal_format(s)
        ),
        // A DATE holds a time to the second; the cast gives it the six fraction digits.
        (Dialect::Oracle, ColumnRule::TimestampUtc) => {
            format!(
                "TO_CHAR(CAST(SYS_EXTRACT_UTC({c}) AS TIMESTAMP(6)), 'YYYY-MM-DD\"T\"HH24:MI:SS.FF6')"
            )
        }
        (Dialect::Oracle, ColumnRule::TimestampNaive) => {
            format!("TO_CHAR(CAST({c} AS TIMESTAMP(6)), 'YYYY-MM-DD\"T\"HH24:MI:SS.FF6')")
        }
        (Dialect::Oracle, ColumnRule::Date) => format!("TO_CHAR({c}, 'YYYY-MM-DD')"),
        (Dialect::Oracle, ColumnRule::Time) => {
            format!("TO_CHAR(CAST({c} AS TIMESTAMP(6)), 'HH24:MI:SS.FF6')")
        }
        (Dialect::Oracle, ColumnRule::Uuid) => format!("LOWER(TO_CHAR({c}))"),
        (Dialect::Oracle, ColumnRule::Bytes) => format!("LOWER(RAWTOHEX({c}))"),
    };
    // NULL is decided on the **column**, not on the rendered value, and the same way on both
    // sides. `coalesce(body, …)` on PostgreSQL only fired when the *rendering* was NULL — and the
    // boolean rule renders NULL as 'f' (`CASE WHEN NULL … ELSE 'f'`), so a NULL boolean hashed as
    // false on PostgreSQL and as the NULL sentinel on ClickHouse. Found as a V2/V3 failure on
    // LedgerSMB's all-NULL `ar.force_closed`; the same asymmetry meant a load that turned NULLs into
    // false would have hashed equal on the PostgreSQL side of a PG→PG check.
    let null = null_literal(d);
    match d {
        Dialect::Postgres => format!("(case when {c} is null then {null} else {body} end)"),
        Dialect::ClickHouse => format!("if(isNull({c}), {null}, {body})"),
        Dialect::Trino | Dialect::Oracle => {
            format!("(CASE WHEN {c} IS NULL THEN {null} ELSE {body} END)")
        }
    }
}

/// `FM` + 38 digit positions, `s` of them after the point: `FM999…990.00`. No point at scale 0.
fn oracle_decimal_format(s: u32) -> String {
    let int = 38usize.saturating_sub(s as usize).max(1);
    let mut f = format!("FM{}0", "9".repeat(int - 1));
    if s > 0 {
        f.push('.');
        f.push_str(&"0".repeat(s as usize));
    }
    f
}

/// RFC 0216: a column's digest — the first 16 hex digits of its canonical form's hash. A row
/// hashed over digests has a bounded length whatever its columns hold (see
/// [`crate::tiers::UnitPlan::digest_columns`]).
pub fn column_digest_expr(d: Dialect, column_expr: &str) -> String {
    let h = hash_hex_expr(d, column_expr);
    match d {
        Dialect::ClickHouse => format!("substring({h}, 1, 16)"),
        Dialect::Oracle => format!("SUBSTR({h}, 1, 16)"),
        Dialect::Postgres | Dialect::Trino => format!("substr({h}, 1, 16)"),
    }
}

/// The row-hash expression over a row's column expressions.
pub fn row_hash_expr(d: Dialect, column_exprs: &[String]) -> String {
    let sep = match d {
        Dialect::Postgres | Dialect::Trino => format!("chr({})", UNIT_SEPARATOR as u32),
        Dialect::ClickHouse => "char(31)".to_string(),
        Dialect::Oracle => format!("CHR({})", UNIT_SEPARATOR as u32),
    };
    if d == Dialect::Trino {
        let mut parts = Vec::with_capacity(column_exprs.len() * 2);
        for (i, e) in column_exprs.iter().enumerate() {
            if i > 0 {
                parts.push(sep.clone());
            }
            parts.push(e.clone());
        }
        return hash_hex_expr(d, &flat_concat(parts));
    }
    hash_hex_expr(d, &column_exprs.join(&format!(" || {sep} || ")))
}

/// Trino's arguments to `concat` per call: well under its limit on a function's arity.
const CONCAT_ARITY: usize = 64;

/// `concat(…)` over `parts`, nested in chunks: the same string as `a || b || …`, but as a flat call
/// rather than a left-deep chain of operators. The Trino read path parses every query and prints it
/// back (the RFC 0160 classifier), and printing a chain recurses once per operator — a row of 92
/// columns overflowed a test thread's stack (RFC 0216, iDempiere `C_ORDER`).
fn flat_concat(mut parts: Vec<String>) -> String {
    while parts.len() > 1 {
        parts = parts
            .chunks(CONCAT_ARITY)
            .map(|c| match c {
                [one] => one.clone(),
                many => format!("concat({})", many.join(", ")),
            })
            .collect();
    }
    parts.pop().unwrap_or_else(|| "''".into())
}

/// SHA-256 of a text expression, as lowercase hex — the engine-side twin of [`crate::row_hash`].
///
/// PostgreSQL's `sha256` takes `bytea`, so the text is encoded as UTF-8 first; ClickHouse hashes a
/// `String`'s bytes directly, which are already UTF-8. Both then hash the same bytes.
pub fn hash_hex_expr(d: Dialect, text_expr: &str) -> String {
    match d {
        Dialect::Postgres => format!("encode(sha256(convert_to({text_expr}, 'UTF8')), 'hex')"),
        Dialect::ClickHouse => format!("lower(hex(SHA256({text_expr})))"),
        // `sha256` takes `varbinary`; `to_utf8` gives the text's UTF-8 bytes.
        Dialect::Trino => format!("lower(to_hex(sha256(to_utf8({text_expr}))))"),
        // The bytes of the text in the database character set, AL32UTF8 by the dialect's
        // contract. Not `UTL_I18N.STRING_TO_RAW`: a SQL `RAW` stops at 2000 bytes.
        Dialect::Oracle => format!("LOWER(RAWTOHEX(STANDARD_HASH({text_expr}, 'SHA256')))"),
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
        // `from_base` reads the digits as a human does, big-endian; 15 hex digits fit a bigint.
        Dialect::Trino => format!("from_base(substr({hash_expr}, 1, 15), 16)"),
        // A hex format model reads big-endian too, into an exact NUMBER.
        Dialect::Oracle => format!("TO_NUMBER(SUBSTR({hash_expr}, 1, 15), 'XXXXXXXXXXXXXXX')"),
    }
}

/// The exact sum of 60-bit prefixes. Trino's `sum(bigint)` fails on overflow rather than
/// wrapping, which would turn a large bucket into an error, so it sums a `decimal(38, 0)`.
fn exact_sum(d: Dialect, prefix: &str) -> String {
    match d {
        Dialect::Postgres => format!("sum(({prefix})::numeric)"),
        Dialect::ClickHouse => format!("sum(toDecimal128({prefix}, 0))"),
        Dialect::Trino => format!("sum(CAST({prefix} AS decimal(38, 0)))"),
        // NUMBER arithmetic is exact to 38 digits.
        Dialect::Oracle => format!("SUM({prefix})"),
    }
}

/// `a % n`; Oracle has no `%` operator.
fn modulo(d: Dialect, a: &str, n: u32) -> String {
    match d {
        Dialect::Oracle => format!("MOD({a}, {n})"),
        Dialect::Postgres | Dialect::ClickHouse | Dialect::Trino => format!("{a} % {n}"),
    }
}

/// Oracle cannot `GROUP BY` an alias or a position: the bucket is computed in a subquery.
fn oracle_buckets(bucket: &str, row_prefix: &str, table: &str, filter: &str) -> String {
    format!(
        "SELECT bucket, COUNT(*) AS row_count, SUM(h) AS hash_sum FROM \
         (SELECT {bucket} AS bucket, {row_prefix} AS h FROM {table}{filter}) \
         GROUP BY bucket ORDER BY bucket"
    )
}

/// Trino cannot `GROUP BY` an output alias; the position means the same.
fn group_and_order(d: Dialect) -> &'static str {
    match d {
        Dialect::Trino => "GROUP BY 1 ORDER BY 1",
        Dialect::Postgres | Dialect::ClickHouse => "GROUP BY bucket ORDER BY bucket",
        Dialect::Oracle => unreachable!("Oracle groups a subquery: oracle_buckets"),
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
    let pk_hash = hash_hex_expr(d, pk_expr);
    let bucket = modulo(d, &prefix60_expr(d, &pk_hash), buckets);
    if d == Dialect::Oracle {
        return oracle_buckets(&bucket, &prefix60_expr(d, &row), table, "");
    }
    let sum = exact_sum(d, &prefix60_expr(d, &row));
    let group = group_and_order(d);
    format!(
        "SELECT {bucket} AS bucket, count(*) AS row_count, {sum} AS hash_sum \
         FROM {table} {group}"
    )
}

/// The bucket expression on its own, so bisect can both group by it and filter on it.
pub fn bucket_expr(d: Dialect, pk_expr: &str, buckets: u32) -> String {
    let pk_hash = hash_hex_expr(d, pk_expr);
    modulo(d, &prefix60_expr(d, &pk_hash), buckets)
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
    if d == Dialect::Oracle {
        let filter = format!(" WHERE {coarse} = {coarse_bucket}");
        return oracle_buckets(&fine, &prefix60_expr(d, &row), table, &filter);
    }
    let sum = exact_sum(d, &prefix60_expr(d, &row));
    let group = group_and_order(d);
    format!(
        "SELECT {fine} AS bucket, count(*) AS row_count, {sum} AS hash_sum \
         FROM {table} WHERE {coarse} = {coarse_bucket} {group}"
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
    let null = null_literal(d);
    let mut parts = Vec::new();
    for (i, e) in column_exprs.iter().enumerate() {
        parts.push(format!(
            "sum(case when {e} = {null} then 1 else 0 end) AS nulls_{i}"
        ));
        parts.push(format!("min({e}) AS min_{i}"));
        parts.push(format!("max({e}) AS max_{i}"));
        // Bytes on both sides: the canonical form is compared as bytes, and PostgreSQL's
        // `length()` counts *characters* while ClickHouse's counts *bytes* — so every column
        // holding one non-ASCII character diverged at V2 (LedgerSMB demo: "Raw Materials — Steel").
        let len = match d {
            Dialect::Postgres => format!("octet_length({e})"),
            Dialect::ClickHouse => format!("length({e})"),
            // Trino's `length` counts characters, as PostgreSQL's does.
            Dialect::Trino => format!("length(to_utf8({e}))"),
            // Bytes in the database character set: UTF-8, by the dialect's contract.
            Dialect::Oracle => format!("LENGTHB({e})"),
        };
        parts.push(format!("sum({len}) AS len_{i}"));
    }
    // ClickHouse has no `case when` shorthand difference here, but `sum(case when …)` is valid in
    // both, so one form serves both dialects and there is one less place to diverge.
    match d {
        // Trino answers NULL over an empty table, as PostgreSQL does.
        Dialect::Postgres | Dialect::Trino | Dialect::Oracle => {
            format!("SELECT {} FROM {table}", parts.join(", "))
        }
        // Over an empty table PostgreSQL's min/max/sum are NULL and ClickHouse's are type defaults
        // ('' / 0), so every empty table failed V2 on nulls, min and max. This setting makes
        // ClickHouse answer NULL, as PostgreSQL does.
        Dialect::ClickHouse => format!(
            "SELECT {} FROM {table} SETTINGS aggregate_functions_null_for_empty = 1",
            parts.join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_renders_the_canonical_forms_in_its_own_functions() {
        let o = |r| canon_expr(Dialect::Oracle, "C", r);
        assert_eq!(
            o(ColumnRule::Text),
            "(CASE WHEN \"C\" IS NULL THEN '\\N' ELSE \
             REPLACE(REPLACE(TO_CHAR(\"C\"), '\\', '\\\\'), CHR(31), '\\x1f') END)"
        );
        assert!(o(ColumnRule::Int).contains("ELSE TO_CHAR(\"C\") END"));
        assert!(
            o(ColumnRule::Decimal(2))
                .contains(&format!("TO_CHAR(\"C\", 'FM{}0.00')", "9".repeat(35))),
            "{}",
            o(ColumnRule::Decimal(2))
        );
        assert!(
            o(ColumnRule::Decimal(0)).contains(&format!("'FM{}0')", "9".repeat(37))),
            "no point at scale 0: {}",
            o(ColumnRule::Decimal(0))
        );
        assert!(
            o(ColumnRule::TimestampNaive)
                .contains("TO_CHAR(CAST(\"C\" AS TIMESTAMP(6)), 'YYYY-MM-DD\"T\"HH24:MI:SS.FF6')")
        );
        assert!(o(ColumnRule::TimestampUtc).contains("SYS_EXTRACT_UTC(\"C\")"));
        assert!(o(ColumnRule::Bytes).contains("LOWER(RAWTOHEX(\"C\"))"));
    }

    #[test]
    fn oracle_hashes_big_endian_and_groups_a_subquery() {
        assert_eq!(
            hash_hex_expr(Dialect::Oracle, "x"),
            "LOWER(RAWTOHEX(STANDARD_HASH(x, 'SHA256')))"
        );
        assert_eq!(
            prefix60_expr(Dialect::Oracle, "h"),
            "TO_NUMBER(SUBSTR(h, 1, 15), 'XXXXXXXXXXXXXXX')"
        );
        let q = bucket_checksum_query(Dialect::Oracle, "\"S\".\"T\"", "pk", &["a".into()], 64);
        assert!(
            q.starts_with(
                "SELECT bucket, COUNT(*) AS row_count, SUM(h) AS hash_sum FROM (SELECT MOD("
            ),
            "{q}"
        );
        assert!(
            q.ends_with("FROM \"S\".\"T\") GROUP BY bucket ORDER BY bucket"),
            "{q}"
        );
        assert!(!q.contains(" % "), "Oracle has no % operator: {q}");
        let sub = sub_bucket_checksum_query(Dialect::Oracle, "t", "pk", &["a".into()], 16, 3, 256);
        assert!(sub.contains(", 256) AS bucket"), "{sub}");
        assert!(
            sub.contains(", 16) = 3)"),
            "the coarse filter is inside the subquery: {sub}"
        );
        assert!(key_hash_query(Dialect::Oracle, "t", "pk", &["a".into()], 16, 3).contains("MOD("));
        assert!(aggregate_query(Dialect::Oracle, "t", &["e".into()]).contains("sum(LENGTHB(e))"));
    }

    #[test]
    fn a_column_digest_is_sixteen_hex_digits_of_the_columns_hash_on_every_engine() {
        for d in [
            Dialect::Postgres,
            Dialect::ClickHouse,
            Dialect::Trino,
            Dialect::Oracle,
        ] {
            let e = column_digest_expr(d, "x");
            assert!(e.contains(&hash_hex_expr(d, "x")), "{e}");
            assert!(e.ends_with(", 1, 16)"), "{e}");
        }
    }

    /// A wide row is a flat `concat`, not a chain of `||` a parser nests one level per column.
    #[test]
    fn a_trino_row_of_any_width_stays_shallow() {
        let cols: Vec<String> = (0..300).map(|i| format!("c{i}")).collect();
        let e = row_hash_expr(Dialect::Trino, &cols);
        assert!(!e.contains("||"), "{e}");
        let (mut depth, mut deepest) = (0i32, 0i32);
        for ch in e.chars() {
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            deepest = deepest.max(depth);
        }
        assert!(deepest < 12, "nesting {deepest}");
        let order: Vec<usize> = cols
            .iter()
            .map(|c| {
                e.find(&format!("{c},"))
                    .or_else(|| e.find(&format!("{c})")))
                    .unwrap()
            })
            .collect();
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "the columns keep their order"
        );
        assert_eq!(
            row_hash_expr(Dialect::Trino, &["a".into()]),
            hash_hex_expr(Dialect::Trino, "a")
        );
    }

    /// The other engines' SQL is unchanged by Oracle's modulo and grouping.
    #[test]
    fn the_other_engines_keep_their_bucket_sql() {
        let pg = bucket_checksum_query(Dialect::Postgres, "t", "pk", &["a".into()], 64);
        assert!(pg.contains("::bigint % 64 AS bucket"), "{pg}");
        assert!(
            pg.ends_with("FROM t GROUP BY bucket ORDER BY bucket"),
            "{pg}"
        );
        let trino = bucket_checksum_query(Dialect::Trino, "t", "pk", &["a".into()], 64);
        assert!(trino.ends_with("FROM t GROUP BY 1 ORDER BY 1"), "{trino}");
    }

    /// A NULL boolean must render as the NULL sentinel on **both** engines. PostgreSQL used to
    /// coalesce the *rendered* value, and the boolean rule renders NULL as 'f'.
    #[test]
    fn v2_measures_bytes_and_treats_an_empty_table_the_same_on_both_engines() {
        let pg = aggregate_query(Dialect::Postgres, "t", &["x".into()]);
        let ch = aggregate_query(Dialect::ClickHouse, "t", &["x".into()]);
        assert!(pg.contains("sum(octet_length(x))"), "{pg}");
        assert!(ch.contains("sum(length(x))"), "{ch}");
        assert!(
            ch.ends_with("SETTINGS aggregate_functions_null_for_empty = 1"),
            "{ch}"
        );
    }

    #[test]
    fn null_is_decided_on_the_column_on_both_engines() {
        let pg = canon_expr(Dialect::Postgres, "b", ColumnRule::Bool);
        let ch = canon_expr(Dialect::ClickHouse, "b", ColumnRule::Bool);
        assert!(pg.starts_with("(case when \"b\" is null then"), "{pg}");
        assert!(ch.starts_with("if(isNull(\"b\")"), "{ch}");
    }

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
        for d in [Dialect::Postgres, Dialect::ClickHouse, Dialect::Trino] {
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
        for d in [Dialect::Postgres, Dialect::ClickHouse, Dialect::Trino] {
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

    /// Trino rejects `GROUP BY <alias>`, and `sum(bigint)` errors on overflow.
    #[test]
    fn trino_groups_by_position_and_sums_exactly() {
        let q = bucket_checksum_query(Dialect::Trino, "t", "pk", &["a".into()], 16);
        assert!(q.ends_with("GROUP BY 1 ORDER BY 1"), "{q}");
        assert!(q.contains("sum(CAST(from_base("), "{q}");
        assert!(q.contains("AS decimal(38, 0)))"), "{q}");
        let q = sub_bucket_checksum_query(Dialect::Trino, "t", "pk", &["a".into()], 16, 3, 256);
        assert!(q.ends_with("GROUP BY 1 ORDER BY 1"), "{q}");
    }

    #[test]
    fn trino_measures_bytes_and_keeps_microseconds() {
        let q = aggregate_query(Dialect::Trino, "t", &["x".into()]);
        assert!(q.contains("sum(length(to_utf8(x)))"), "{q}");
        let ts = canon_expr(Dialect::Trino, "ts", ColumnRule::TimestampUtc);
        assert!(ts.contains("to_iso8601(CAST("), "{ts}");
        assert!(ts.contains("AS timestamp(6)"), "{ts}");
        assert!(
            !ts.contains("format_datetime"),
            "millisecond precision only: {ts}"
        );
    }

    #[test]
    fn both_dialects_produce_a_grouped_bucket_query() {
        for d in [Dialect::Postgres, Dialect::ClickHouse, Dialect::Trino] {
            let q = bucket_checksum_query(d, "t", "pk", &["a".into(), "b".into()], 16);
            assert!(q.contains("GROUP BY "), "{d:?}: {q}");
            assert!(q.contains("count(*)"), "{d:?}: {q}");
        }
    }
}
