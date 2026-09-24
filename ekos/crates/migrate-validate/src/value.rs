//! RFC 0155 — the neutral value model the canonical form is defined over.
//!
//! Deliberately not tied to any driver. A live PostgreSQL row (RFC 0157) and a ClickHouse or Delta
//! row (RFC 0159/0165) both map *into* this type, so the canonical form has exactly one definition
//! rather than one per engine — which is the whole point, since a per-engine definition is how two
//! different databases end up agreeing.

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

/// One column value, as recovered from any engine.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    /// An exact decimal, kept as its unscaled digits and a scale so no binary float is involved at
    /// any point. `digits` is the integer form including a leading `-`; `scale` is how many of its
    /// trailing digits are fractional.
    Decimal {
        digits: String,
        scale: u32,
    },
    /// `real` / `double precision`. **Excluded from hashing** — see [`crate::canon`].
    Float(f64),
    Text(String),
    /// `char(n)`: padding is part of the value and is never trimmed.
    Char(String),
    /// A timestamp already normalized to UTC by the reader (`timestamptz`).
    TimestampUtc(NaiveDateTime),
    /// A wall-clock reading with no zone (`timestamp`). Never converted: converting would invent
    /// information that is not in the source.
    TimestampNaive(NaiveDateTime),
    Date(NaiveDate),
    Time(NaiveTime),
    /// PostgreSQL's own three-field interval model, kept as three fields so one month is never
    /// silently normalized to thirty days.
    Interval {
        months: i32,
        days: i32,
        micros: i64,
    },
    Uuid(uuid::Uuid),
    Bytes(Vec<u8>),
    /// `inet` / `cidr`, already in canonical text form with its prefix length.
    Inet(String),
    Array(Vec<Value>),
    /// `jsonb`. Hashed only by shape at V2 — see [`crate::canon`].
    Json(String),
}

impl Value {
    /// `true` for the values excluded from `row_hash` (RFC 0155): floats, because no decimal
    /// rendering is both lossless and portable across three engines, and JSON, because
    /// canonicalizing it is a semantics decision rather than a serialization one.
    pub fn is_hash_excluded(&self) -> bool {
        matches!(self, Self::Float(_) | Self::Json(_))
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Decimal { .. } => "decimal",
            Self::Float(_) => "float",
            Self::Text(_) => "text",
            Self::Char(_) => "char",
            Self::TimestampUtc(_) => "timestamptz",
            Self::TimestampNaive(_) => "timestamp",
            Self::Date(_) => "date",
            Self::Time(_) => "time",
            Self::Interval { .. } => "interval",
            Self::Uuid(_) => "uuid",
            Self::Bytes(_) => "bytea",
            Self::Inet(_) => "inet",
            Self::Array(_) => "array",
            Self::Json(_) => "json",
        }
    }
}
