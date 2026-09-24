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
