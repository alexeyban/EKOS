//! RFC 0155 — the awkward-value fixture table.
//!
//! Lives in the crate rather than in a test file because the golden tests, the planted-defect
//! controls (RFC 0156) and the eventual per-engine fixture loader all need the *same* values. Two
//! copies of "the tricky cases" is how one of them quietly stops covering something.
//!
//! Every value here has caused a real cross-engine mismatch somewhere, or is one character away
//! from forging the null sentinel.

use crate::{Spec, Value};
use chrono::{NaiveDate, NaiveTime};

/// `(case name, value, spec)` — the cases every canonical-form rule is pinned by.
pub fn cases() -> Vec<(&'static str, Value, Spec)> {
    let plain = Spec::default();
    let scale2 = Spec {
        decimal_scale: Some(2),
    };
    let dec = |d: &str, s: u32| Value::Decimal {
        digits: d.to_string(),
        scale: s,
    };
    vec![
        ("null", Value::Null, plain),
        ("empty_string", Value::Text(String::new()), plain),
        // One character away from the null sentinel.
        ("literal_backslash_n", Value::Text("\\N".into()), plain),
        ("double_backslash", Value::Text("\\\\".into()), plain),
        (
            "embedded_unit_separator",
            Value::Text("a\u{1f}b".into()),
            plain,
        ),
        ("non_ascii", Value::Text("Ünïcödé".into()), plain),
        ("emoji", Value::Text("🦀".into()), plain),
        ("newline", Value::Text("a\nb".into()), plain),
        ("char_trailing_spaces", Value::Char("ab   ".into()), plain),
        ("bool_true", Value::Bool(true), plain),
        ("bool_false", Value::Bool(false), plain),
        ("int_zero", Value::Int(0), plain),
        ("int_negative", Value::Int(-42), plain),
        ("int_max", Value::Int(i64::MAX), plain),
        ("decimal_trailing_zeros", dec("150", 2), scale2),
        ("decimal_widened", dec("1", 0), scale2),
        ("decimal_leading_zero", dec("5", 2), scale2),
        ("decimal_negative", dec("-150", 2), scale2),
        ("decimal_negative_zero", dec("-0", 2), scale2),
        ("decimal_large", dec("123456789012345678901234", 2), scale2),
        (
            "timestamp_utc_whole_second",
            Value::TimestampUtc(
                NaiveDate::from_ymd_opt(2026, 9, 24)
                    .unwrap()
                    .and_hms_micro_opt(12, 30, 0, 0)
                    .unwrap(),
            ),
            plain,
        ),
        (
            "timestamp_utc_microseconds",
            Value::TimestampUtc(
                NaiveDate::from_ymd_opt(2026, 9, 24)
                    .unwrap()
                    .and_hms_micro_opt(12, 30, 0, 123_456)
                    .unwrap(),
            ),
            plain,
        ),
        (
            "timestamp_naive",
            Value::TimestampNaive(
                NaiveDate::from_ymd_opt(1999, 12, 31)
                    .unwrap()
                    .and_hms_micro_opt(23, 59, 59, 999_999)
                    .unwrap(),
            ),
            plain,
        ),
        // Before 1900: outside ClickHouse's Date32 range, a real compatibility finding.
        (
            "date_pre_1900",
            Value::Date(NaiveDate::from_ymd_opt(1850, 6, 15).unwrap()),
            plain,
        ),
        (
            "date_epoch",
            Value::Date(NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()),
            plain,
        ),
        (
            "time_microseconds",
            Value::Time(NaiveTime::from_hms_micro_opt(1, 2, 3, 4).unwrap()),
            plain,
        ),
        (
            "interval_months_and_days",
            Value::Interval {
                months: 1,
                days: 15,
                micros: 3_600_000_000,
            },
            plain,
        ),
        (
            "uuid_uppercase_input",
            Value::Uuid(uuid::Uuid::parse_str("A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11").unwrap()),
            plain,
        ),
        ("bytes", Value::Bytes(vec![0x00, 0x0f, 0xff]), plain),
        ("bytes_empty", Value::Bytes(vec![]), plain),
        ("inet_v4", Value::Inet("192.168.0.1/32".into()), plain),
        ("inet_v6", Value::Inet("2001:db8::1/128".into()), plain),
        (
            "array_with_null_element",
            Value::Array(vec![Value::Int(1), Value::Null, Value::Int(3)]),
            plain,
        ),
        (
            "array_nested",
            Value::Array(vec![
                Value::Array(vec![Value::Text("a".into())]),
                Value::Array(vec![]),
            ]),
            plain,
        ),
        ("array_empty", Value::Array(vec![]), plain),
    ]
}

/// A fixture case expressed as a *typed literal* in each engine, for the live cross-engine check.
///
/// `None` means the engine has no faithful literal for that case, and it is listed in
/// [`UNMAPPED_LIVE_CASES`] with the reason rather than quietly dropped — an uncovered case that
/// nobody can name is how coverage rots.
pub struct LiveCase {
    pub name: &'static str,
    pub rule: crate::ColumnRule,
    pub postgres: Option<&'static str>,
    pub clickhouse: Option<&'static str>,
}

/// Cases with no faithful literal on one engine, and why. Asserted complete by the live harness.
pub const UNMAPPED_LIVE_CASES: &[(&str, &str)] = &[
    (
        "interval_months_and_days",
        "ClickHouse has no composite interval type; RFC 0158 maps it to a months/days/micros split,          so there is nothing to render here until that mapping exists",
    ),
    (
        "array_with_null_element",
        "ClickHouse Array(T) cannot hold NULL without Array(Nullable(T)); the element-level rule is          covered by the Rust golden table and needs the RFC 0159 array mapping to test in-engine",
    ),
    (
        "array_nested",
        "same as array_with_null_element: nested array literals need the RFC 0159 mapping",
    ),
    (
        "array_empty",
        "an untyped empty array literal has no element type on either engine",
    ),
    (
        "date_pre_1900",
        "ClickHouse Date32 bottoms out at 1900-01-01 and **silently clamps** 1850-06-15 to it \
         rather than erroring (verified live on 24.8: both formatDateTime and toString return \
         1900-01-01). That is a real compatibility finding, not a canonical-form defect — see \
         RFC 0158's COMPAT.CH date rules — and the silence is exactly why that rule has to measure \
         affected rows before any data moves",
    ),
    (
        "time_microseconds",
        "ClickHouse has no standalone time-of-day type; RFC 0159 maps a PostgreSQL `time` column \
         to something else, and there is nothing to render until it does",
    ),
];

/// The per-engine typed literals. Deliberately hand-written: generating them from the same code
/// that renders the canonical form would make the check circular.
pub fn live_cases() -> Vec<LiveCase> {
    use crate::ColumnRule as R;
    let c = |name, rule, postgres, clickhouse| LiveCase {
        name,
        rule,
        postgres,
        clickhouse,
    };
    vec![
        c(
            "null",
            R::Text,
            Some("NULL::text"),
            Some("CAST(NULL AS Nullable(String))"),
        ),
        c(
            "empty_string",
            R::Text,
            Some("''::text"),
            Some("CAST('' AS Nullable(String))"),
        ),
        c(
            "literal_backslash_n",
            R::Text,
            Some("E'\\\\N'::text"),
            Some("CAST('\\\\N' AS Nullable(String))"),
        ),
        c(
            "double_backslash",
            R::Text,
            Some("E'\\\\\\\\'::text"),
            Some("CAST('\\\\\\\\' AS Nullable(String))"),
        ),
        c(
            "embedded_unit_separator",
            R::Text,
            Some("E'a\\x1fb'::text"),
            Some("CAST(concat('a', char(31), 'b') AS Nullable(String))"),
        ),
        c(
            "non_ascii",
            R::Text,
            Some("'Ünïcödé'::text"),
            Some("CAST('Ünïcödé' AS Nullable(String))"),
        ),
        c(
            "emoji",
            R::Text,
            Some("'🦀'::text"),
            Some("CAST('🦀' AS Nullable(String))"),
        ),
        c(
            "newline",
            R::Text,
            Some("E'a\\nb'::text"),
            Some("CAST('a\\nb' AS Nullable(String))"),
        ),
        c(
            "char_trailing_spaces",
            R::Char,
            Some("'ab   '::text"),
            Some("CAST('ab   ' AS Nullable(String))"),
        ),
        c(
            "bool_true",
            R::Bool,
            Some("true"),
            Some("CAST(1 AS Nullable(UInt8))"),
        ),
        c(
            "bool_false",
            R::Bool,
            Some("false"),
            Some("CAST(0 AS Nullable(UInt8))"),
        ),
        c(
            "int_zero",
            R::Int,
            Some("0::bigint"),
            Some("CAST(0 AS Nullable(Int64))"),
        ),
        c(
            "int_negative",
            R::Int,
            Some("(-42)::bigint"),
            Some("CAST(-42 AS Nullable(Int64))"),
        ),
        c(
            "int_max",
            R::Int,
            Some("9223372036854775807::bigint"),
            Some("CAST(9223372036854775807 AS Nullable(Int64))"),
        ),
        c(
            "decimal_trailing_zeros",
            R::Decimal(2),
            Some("1.50::numeric"),
            Some("CAST(1.50 AS Nullable(Decimal128(2)))"),
        ),
        c(
            "decimal_widened",
            R::Decimal(2),
            Some("1::numeric"),
            Some("CAST(1 AS Nullable(Decimal128(2)))"),
        ),
        c(
            "decimal_leading_zero",
            R::Decimal(2),
            Some("0.05::numeric"),
            Some("CAST(0.05 AS Nullable(Decimal128(2)))"),
        ),
        c(
            "decimal_negative",
            R::Decimal(2),
            Some("(-1.50)::numeric"),
            Some("CAST(-1.50 AS Nullable(Decimal128(2)))"),
        ),
        c(
            "decimal_negative_zero",
            R::Decimal(2),
            Some("(-0.00)::numeric"),
            Some("CAST(-0.00 AS Nullable(Decimal128(2)))"),
        ),
        c(
            "decimal_large",
            R::Decimal(2),
            Some("1234567890123456789012.34::numeric"),
            Some("CAST('1234567890123456789012.34' AS Nullable(Decimal128(2)))"),
        ),
        c(
            "timestamp_utc_whole_second",
            R::TimestampUtc,
            Some("'2026-09-24 12:30:00'::timestamptz"),
            Some(
                "CAST(toDateTime64('2026-09-24 12:30:00', 6, 'UTC') AS Nullable(DateTime64(6, 'UTC')))",
            ),
        ),
        c(
            "timestamp_utc_microseconds",
            R::TimestampUtc,
            Some("'2026-09-24 12:30:00.123456'::timestamptz"),
            Some(
                "CAST(toDateTime64('2026-09-24 12:30:00.123456', 6, 'UTC') AS Nullable(DateTime64(6, 'UTC')))",
            ),
        ),
        c(
            "timestamp_naive",
            R::TimestampNaive,
            Some("'1999-12-31 23:59:59.999999'::timestamp"),
            Some("CAST(toDateTime64('1999-12-31 23:59:59.999999', 6) AS Nullable(DateTime64(6)))"),
        ),
        c("date_pre_1900", R::Date, Some("'1850-06-15'::date"), None),
        c(
            "date_epoch",
            R::Date,
            Some("'1970-01-01'::date"),
            Some("CAST(toDate32('1970-01-01') AS Nullable(Date32))"),
        ),
        c(
            "time_microseconds",
            R::Time,
            Some("'01:02:03.000004'::time"),
            None,
        ),
        c("interval_months_and_days", R::Text, None, None),
        c(
            "uuid_uppercase_input",
            R::Uuid,
            Some("'A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::uuid"),
            Some("CAST(toUUID('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11') AS Nullable(UUID))"),
        ),
        c(
            "bytes",
            R::Bytes,
            Some("E'\\\\x000fff'::bytea"),
            Some("CAST(unhex('000FFF') AS Nullable(String))"),
        ),
        c(
            "bytes_empty",
            R::Bytes,
            Some("E'\\\\x'::bytea"),
            Some("CAST(unhex('') AS Nullable(String))"),
        ),
        c(
            "inet_v4",
            R::Inet,
            Some("'192.168.0.1/32'::inet"),
            Some("CAST('192.168.0.1/32' AS Nullable(String))"),
        ),
        c(
            "inet_v6",
            R::Inet,
            Some("'2001:db8::1/128'::inet"),
            Some("CAST('2001:db8::1/128' AS Nullable(String))"),
        ),
        c("array_with_null_element", R::Text, None, None),
        c("array_nested", R::Text, None, None),
        c("array_empty", R::Text, None, None),
    ]
}
