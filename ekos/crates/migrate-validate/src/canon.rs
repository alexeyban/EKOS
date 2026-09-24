//! RFC 0155 — the canonical text form.
//!
//! One rule per type family, such that the same logical value renders to the same bytes in
//! PostgreSQL, in ClickHouse and here. Every rule is pinned by a golden test whose expected string
//! is a committed literal, because two engines agreeing with each other proves nothing if both are
//! wrong the same way.

use crate::value::Value;
use crate::{CanonError, Spec};

/// The null sentinel. Distinct from every value by construction: a literal backslash in a
/// text-like value is doubled before the separator is applied, so `\N` means null and `\\N` means
/// the two characters.
pub const NULL_SENTINEL: &str = "\\N";

/// `U+001F` ASCII unit separator — the column join character. Chosen because it cannot appear
/// unescaped in any canonical form below.
pub const UNIT_SEPARATOR: char = '\u{1f}';

/// Double every backslash, and escape a literal `US` so it can never be mistaken for a column
/// boundary.
///
/// Without this the `\N` sentinel is unsound: a text column holding the two characters `\N` would
/// hash identically to a NULL. That is a false green, and "nobody stores that" is the kind of
/// assumption this system exists to avoid relying on.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            UNIT_SEPARATOR => out.push_str("\\x1f"),
            other => out.push(other),
        }
    }
    out
}

/// Render one value in its canonical form.
///
/// `spec` supplies the **approved** decimal scale (from the `MigrationTypeMapping`, RFC 0159) —
/// not the scale the value happens to carry. The approved mapping is what both sides were built
/// against, so it is what both sides must render to.
pub fn canon(v: &Value, spec: &Spec) -> Result<String, CanonError> {
    Ok(match v {
        Value::Null => NULL_SENTINEL.to_string(),
        Value::Bool(b) => (if *b { "t" } else { "f" }).to_string(),
        Value::Int(i) => i.to_string(),
        Value::Decimal { digits, scale } => canon_decimal(digits, *scale, spec.decimal_scale)?,
        Value::Float(_) => return Err(CanonError::FloatExcluded),
        Value::Json(_) => return Err(CanonError::JsonExcluded),
        Value::Text(s) => escape(s),
        // Padding is part of a `char(n)` value. A target that trims it is a real divergence and
        // must be reported, not normalized away here.
        Value::Char(s) => escape(s),
        Value::TimestampUtc(ts) | Value::TimestampNaive(ts) => {
            ts.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
        }
        Value::Date(d) => d.format("%Y-%m-%d").to_string(),
        Value::Time(t) => t.format("%H:%M:%S%.6f").to_string(),
        Value::Interval {
            months,
            days,
            micros,
        } => format!("{months}:{days}:{micros}"),
        Value::Uuid(u) => u.as_hyphenated().to_string().to_lowercase(),
        Value::Bytes(b) => b.iter().map(|x| format!("{x:02x}")).collect(),
        Value::Inet(s) => escape(s),
        Value::Array(items) => {
            let mut parts = Vec::with_capacity(items.len());
            for it in items {
                parts.push(match it {
                    Value::Null => NULL_SENTINEL.to_string(),
                    other => canon(other, spec)?,
                });
            }
            format!("{{{}}}", parts.join(","))
        }
    })
}

/// Fixed scale, no exponent, no trailing-zero ambiguity, `-0` normalized to `0`.
///
/// Rescaling **down** is refused rather than rounded: silently dropping a fractional digit is
/// exactly the "truncated decimal" defect RFC 0156 plants a control for, and a serializer that
/// performs it cannot also detect it.
fn canon_decimal(digits: &str, scale: u32, approved: Option<u32>) -> Result<String, CanonError> {
    let target = approved.unwrap_or(scale);
    if target < scale {
        return Err(CanonError::WouldNarrowDecimal {
            from: scale,
            to: target,
        });
    }

    let (neg, mag) = match digits.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, digits),
    };
    if mag.is_empty() || !mag.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CanonError::MalformedDecimal(digits.to_string()));
    }

    // Widen to the approved scale by appending zeros, then split at the decimal point.
    let mut mag = mag.to_string();
    mag.push_str(&"0".repeat((target - scale) as usize));

    let (int_part, frac_part) = if target == 0 {
        (mag.as_str(), "")
    } else if mag.len() <= target as usize {
        // Fewer digits than the scale: left-pad so `5` at scale 2 becomes `0.05`.
        mag = format!("{}{}", "0".repeat(target as usize + 1 - mag.len()), mag);
        mag.split_at(mag.len() - target as usize)
    } else {
        mag.split_at(mag.len() - target as usize)
    };

    let int_part = int_part.trim_start_matches('0');
    let int_part = if int_part.is_empty() { "0" } else { int_part };

    let body = if frac_part.is_empty() {
        int_part.to_string()
    } else {
        format!("{int_part}.{frac_part}")
    };

    // -0, -0.00 → 0, 0.00. A sign on zero is not information, and engines disagree about it.
    let all_zero = body.bytes().all(|b| b == b'0' || b == b'.');
    Ok(if neg && !all_zero {
        format!("-{body}")
    } else {
        body
    })
}

/// Join a row's canonical column values with the unit separator.
///
/// Column order is the **approved target column order**, so a deliberate reordering does not read
/// as a divergence. The caller supplies values already in that order.
pub fn row_canonical(columns: &[String]) -> String {
    columns.join(&UNIT_SEPARATOR.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> Spec {
        Spec::default()
    }

    #[test]
    fn the_null_sentinel_cannot_be_forged() {
        assert_eq!(canon(&Value::Null, &spec()).unwrap(), "\\N");
        // A text column literally holding `\N` must not collide with NULL.
        assert_eq!(canon(&Value::Text("\\N".into()), &spec()).unwrap(), "\\\\N");
        assert_ne!(
            canon(&Value::Text("\\N".into()), &spec()).unwrap(),
            canon(&Value::Null, &spec()).unwrap()
        );
    }

    #[test]
    fn an_embedded_separator_cannot_forge_a_column_boundary() {
        let sneaky = format!("a{UNIT_SEPARATOR}b");
        assert_eq!(canon(&Value::Text(sneaky), &spec()).unwrap(), "a\\x1fb");
    }

    #[test]
    fn empty_string_is_distinct_from_null() {
        assert_eq!(canon(&Value::Text(String::new()), &spec()).unwrap(), "");
        assert_ne!(canon(&Value::Text(String::new()), &spec()).unwrap(), "\\N");
    }

    #[test]
    fn char_padding_is_preserved() {
        assert_eq!(
            canon(&Value::Char("ab   ".into()), &spec()).unwrap(),
            "ab   "
        );
    }

    #[test]
    fn decimals_render_at_the_approved_scale() {
        let s = Spec {
            decimal_scale: Some(2),
        };
        let d = |digits: &str, scale: u32| {
            canon(
                &Value::Decimal {
                    digits: digits.into(),
                    scale,
                },
                &s,
            )
            .unwrap()
        };
        assert_eq!(d("150", 2), "1.50");
        assert_eq!(d("15", 1), "1.50");
        assert_eq!(d("1", 0), "1.00");
        assert_eq!(d("5", 2), "0.05");
        assert_eq!(d("-150", 2), "-1.50");
        assert_eq!(d("0", 0), "0.00");
        assert_eq!(d("-0", 2), "0.00", "a sign on zero is not information");
    }

    #[test]
    fn narrowing_a_decimal_is_refused_not_rounded() {
        let s = Spec {
            decimal_scale: Some(1),
        };
        assert!(matches!(
            canon(
                &Value::Decimal {
                    digits: "155".into(),
                    scale: 2
                },
                &s
            ),
            Err(CanonError::WouldNarrowDecimal { from: 2, to: 1 })
        ));
    }

    #[test]
    fn floats_and_json_are_excluded_from_hashing() {
        assert!(matches!(
            canon(&Value::Float(1.0), &spec()),
            Err(CanonError::FloatExcluded)
        ));
        assert!(matches!(
            canon(&Value::Json("{}".into()), &spec()),
            Err(CanonError::JsonExcluded)
        ));
    }

    #[test]
    fn arrays_recurse_and_keep_null_elements() {
        let v = Value::Array(vec![
            Value::Int(1),
            Value::Null,
            Value::Array(vec![Value::Text("x".into())]),
        ]);
        assert_eq!(canon(&v, &spec()).unwrap(), "{1,\\N,{x}}");
    }

    #[test]
    fn timestamps_always_carry_six_fractional_digits() {
        let ts = chrono::NaiveDate::from_ymd_opt(2026, 9, 24)
            .unwrap()
            .and_hms_micro_opt(12, 30, 0, 0)
            .unwrap();
        assert_eq!(
            canon(&Value::TimestampUtc(ts), &spec()).unwrap(),
            "2026-09-24T12:30:00.000000"
        );
    }
}
