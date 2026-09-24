//! RFC 0155 — canonical value serialization and cross-engine checksums.
//!
//! The smallest and most dangerous component in EKOS Migrate. Counts, aggregates, bisect,
//! divergence classification and the sign-off all inherit its correctness, and its failure mode is
//! a **false green**: two databases that differ, agreeing.
//!
//! It therefore ships before the live connector and before any data moves, proven against
//! hand-written fixtures whose expected hashes are committed literals — three-way agreement
//! between PostgreSQL, ClickHouse and this implementation against a fixed expectation, never
//! two-way agreement between two implementations that can be wrong the same way.
//!
//! # What is proven
//!
//! **Three-way agreement, verified live** (`tests/live_engines.rs`, PostgreSQL 16 and ClickHouse
//! 24.8 from `docker-compose.migrate.yml`): 31 cases on PostgreSQL and 29 on ClickHouse each
//! reproduce the canonical form frozen in `tests/golden.rs`, and both engines reproduce the row
//! hash and the 60-bit bucket prefix. Every assertion is against the committed literal, never
//! against the other engine.
//!
//! Also proven: only NULL renders as the null sentinel and no value can forge a column boundary
//! (`tests/golden.rs`); every serialization-layer planted defect from RFC 0156 changes the bucket
//! checksum, and a clean run or a reordered scan changes nothing (`tests/controls.rs`).
//!
//! **Known gaps, each named with a reason** in [`fixtures::UNMAPPED_LIVE_CASES`] and asserted
//! complete by the harness: intervals and arrays need the RFC 0159 mappings before there is
//! anything to render, ClickHouse has no standalone time-of-day type, and a pre-1900 date is a
//! *compatibility finding* rather than a serialization case — ClickHouse `Date32` bottoms out at
//! 1900-01-01 and silently clamps to it.
//!
//! Three real cross-engine defects were found only by running this, never by review: the
//! ClickHouse NULL-literal escape, `toString` stripping a decimal's trailing zeros, and
//! `reinterpretAsUInt64`'s endianness. See their doc comments in [`dialect`].

pub mod canon;
pub mod dialect;
pub mod fixtures;
pub mod hash;
pub mod value;

pub use canon::{NULL_SENTINEL, UNIT_SEPARATOR, canon, row_canonical};
pub use dialect::{ColumnRule, Dialect, bucket_checksum_query, canon_expr, canon_expr_of};
pub use hash::{BucketChecksum, bucket_of, checksum_buckets, prefix60, row_hash};
pub use value::Value;

/// Per-column rendering parameters that come from the **approved** mapping rather than from the
/// data (RFC 0159).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Spec {
    /// The approved decimal scale. `None` renders a decimal at its own scale, which is correct
    /// only for a column whose mapping is `exact`.
    pub decimal_scale: Option<u32>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CanonError {
    /// Floats are validated at V2 with a documented relative tolerance, never hashed: no decimal
    /// rendering is both lossless and portable across three engines.
    #[error(
        "float columns are excluded from hashing (RFC 0155) — validate them at V2 with a tolerance, \
         or map the column to numeric if exactness is required for sign-off"
    )]
    FloatExcluded,
    /// JSON is validated at V2 by shape. Canonicalizing it is a semantics decision (is a reordered
    /// object the same object?) and belongs to a disposition, not to a serializer.
    #[error("json columns are validated at V2 by length and key count, not hashed (RFC 0155)")]
    JsonExcluded,
    #[error(
        "refusing to render scale {from} at scale {to}: dropping a fractional digit is the \
         truncated-decimal defect the validator exists to detect"
    )]
    WouldNarrowDecimal { from: u32, to: u32 },
    #[error("malformed decimal digits: {0}")]
    MalformedDecimal(String),
}
