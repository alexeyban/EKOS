//! RFC 0155 — the row hash, bucketing and the order-independent bucket checksum.

use md5::{Digest, Md5};

/// `md5` of a row's canonical form, lowercase hex.
///
/// Chosen for availability, not for strength: PostgreSQL, ClickHouse and Spark SQL all have it
/// natively, and this is a comparison function, never a security primitive. Adversarial collision
/// resistance is not a property it needs — do not "upgrade" it to SHA-256 and lose an engine.
pub fn row_hash(canonical: &str) -> String {
    let mut h = Md5::new();
    h.update(canonical.as_bytes());
    format!("{:x}", h.finalize())
}

/// The first 60 bits of a hex hash, as an integer.
///
/// 60 bits fits in an `i64` with room to sum a large bucket without overflow, and every target
/// engine can produce and sum it exactly — which is the reason for the width. A wider prefix would
/// force engines into approximate or overflowing arithmetic, and an approximate checksum is a
/// false green waiting to happen.
pub fn prefix60(hex: &str) -> u64 {
    u64::from_str_radix(&hex[..15], 16).expect("md5 hex is 32 hex chars")
}

/// Which bucket a row's primary key falls in.
pub fn bucket_of(pk_canonical: &str, buckets: u32) -> u32 {
    debug_assert!(buckets > 0, "bucket count must be positive");
    (prefix60(&row_hash(pk_canonical)) % u64::from(buckets)) as u32
}

/// One bucket's order-independent summary.
///
/// The pair, not just the sum: XOR would also be order-independent and overflow-free, but a
/// duplicated row cancels itself out under XOR — hiding exactly the duplicate-row defect a
/// `ReplacingMergeTree` target makes likely. `count` catches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct BucketChecksum {
    pub count: u64,
    /// Sum of each row hash's 60-bit prefix, as exact integer arithmetic (`u128` here, `numeric`
    /// in PostgreSQL, `Decimal128` in ClickHouse).
    pub sum: u128,
}

impl BucketChecksum {
    pub fn add(&mut self, row_hash_hex: &str) {
        self.count += 1;
        self.sum += u128::from(prefix60(row_hash_hex));
    }
}

/// Fold a set of rows into per-bucket checksums. Order of input never affects the result.
pub fn checksum_buckets<'a, I>(
    rows: I,
    buckets: u32,
) -> std::collections::BTreeMap<u32, BucketChecksum>
where
    I: IntoIterator<Item = (&'a str, &'a str)>, // (pk canonical, row canonical)
{
    let mut out: std::collections::BTreeMap<u32, BucketChecksum> = Default::default();
    for (pk, row) in rows {
        let b = bucket_of(pk, buckets);
        out.entry(b).or_default().add(&row_hash(row));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_hash_matches_the_reference_md5() {
        // The value every engine's own md5() must agree with. A committed literal, not a
        // cross-check between two implementations that could both be wrong.
        assert_eq!(row_hash(""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(row_hash("abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn prefix60_takes_the_first_fifteen_hex_digits() {
        assert_eq!(
            prefix60("900150983cd24fb0d6963f7d28e17f72"),
            0x900150983cd24fb
        );
        assert!(prefix60("ffffffffffffffffffffffffffffffff") < (1u64 << 60));
    }

    #[test]
    fn bucketing_is_stable_and_bounded() {
        for n in [1u32, 16, 4096] {
            for key in ["1", "2", "orders:42", ""] {
                let b = bucket_of(key, n);
                assert!(b < n);
                assert_eq!(b, bucket_of(key, n), "bucketing must be deterministic");
            }
        }
    }

    #[test]
    fn a_checksum_is_order_independent() {
        let rows = [("1", "a"), ("2", "b"), ("3", "c")];
        let forward = checksum_buckets(rows.iter().map(|(a, b)| (*a, *b)), 4);
        let backward = checksum_buckets(rows.iter().rev().map(|(a, b)| (*a, *b)), 4);
        assert_eq!(forward, backward);
    }

    /// The reason the checksum is a pair rather than a sum: under XOR a duplicated row cancels
    /// itself out, which is precisely the defect an eventually-deduplicating target makes likely.
    #[test]
    fn a_duplicated_row_changes_the_checksum() {
        let clean = checksum_buckets([("1", "a"), ("2", "b")], 1);
        let duped = checksum_buckets([("1", "a"), ("2", "b"), ("2", "b")], 1);
        assert_ne!(clean, duped);
        assert_eq!(clean[&0].count + 1, duped[&0].count);
    }

    #[test]
    fn a_dropped_row_changes_the_checksum() {
        let clean = checksum_buckets([("1", "a"), ("2", "b")], 1);
        let short = checksum_buckets([("1", "a")], 1);
        assert_ne!(clean, short);
    }

    #[test]
    fn a_changed_value_changes_the_checksum_without_changing_the_count() {
        let a = checksum_buckets([("1", "x"), ("2", "y")], 1);
        let b = checksum_buckets([("1", "x"), ("2", "z")], 1);
        assert_eq!(
            a[&0].count, b[&0].count,
            "a count check alone cannot see this"
        );
        assert_ne!(a[&0].sum, b[&0].sum);
    }
}
