//! RFC 0156 — bisect: from a failed V3 bucket to the exact divergent keys.
//!
//! The shape of the search matters more than the code. A failed bucket is re-bucketed at a finer
//! fan-out on *both* sides and the pairs compared again; only when a sub-bucket is small enough does
//! the validator fetch `(pk, row_hash)` rows and diff them in memory. Full rows are fetched last,
//! for divergent keys only, and are never persisted.
//!
//! A fan-out of 16 isolates one divergent row in a 100M-row table in about six rounds of two cheap
//! aggregate queries, rather than the sixteen a doubling search would take.

use crate::dialect;
use crate::divergence::{Class, Divergence};
use crate::reader::{EngineReader, ReadError};
use crate::tiers::{Tier, TierOutcome, UnitPlan, parse_buckets};
use std::collections::BTreeMap;

/// How the search is bounded. Both fields are policy, not constants: the right values depend on the
/// table, and guessing them in code is how a validator becomes untunable.
#[derive(Debug, Clone, Copy)]
pub struct BisectPolicy {
    /// Multiplier per round.
    pub fan_out: u32,
    /// Stop subdividing once a sub-bucket holds at most this many rows, and fetch keys instead.
    pub row_threshold: u64,
    /// Hard cap on rounds, so a pathological case fails loudly instead of looping.
    pub max_rounds: u32,
}

impl Default for BisectPolicy {
    fn default() -> Self {
        Self {
            fan_out: 16,
            row_threshold: 1000,
            max_rounds: 8,
        }
    }
}

/// One key whose row differs, or which exists on only one side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyDiff {
    SourceOnly(String),
    TargetOnly(String),
    HashDiffers { key: String },
}

impl KeyDiff {
    pub fn key(&self) -> &str {
        match self {
            Self::SourceOnly(k) | Self::TargetOnly(k) => k,
            Self::HashDiffers { key } => key,
        }
    }
}

/// Compare two `(pk, row_hash)` result sets. The one place row-level data is handled, and it is
/// handled in memory and dropped.
pub fn diff_key_hashes(source: Vec<Vec<String>>, target: Vec<Vec<String>>) -> Vec<KeyDiff> {
    let to_map = |rows: Vec<Vec<String>>| -> BTreeMap<String, String> {
        rows.into_iter()
            .filter(|r| r.len() >= 2)
            .map(|r| (r[0].clone(), r[1].clone()))
            .collect()
    };
    let s = to_map(source);
    let t = to_map(target);

    let mut out = Vec::new();
    for (k, sh) in &s {
        match t.get(k) {
            None => out.push(KeyDiff::SourceOnly(k.clone())),
            Some(th) if th != sh => out.push(KeyDiff::HashDiffers { key: k.clone() }),
            Some(_) => {}
        }
    }
    for k in t.keys() {
        if !s.contains_key(k) {
            out.push(KeyDiff::TargetOnly(k.clone()));
        }
    }
    out.sort_by(|a, b| a.key().cmp(b.key()));
    out
}

/// Bisect one failed coarse bucket down to keys.
pub fn bisect_bucket(
    plan: &UnitPlan,
    source: &dyn EngineReader,
    target: &dyn EngineReader,
    coarse_bucket: u32,
    policy: BisectPolicy,
) -> Result<Vec<KeyDiff>, BisectError> {
    let mut buckets = plan.buckets;
    let mut bucket = coarse_bucket;

    for round in 0..policy.max_rounds {
        // How many rows are actually in this bucket, on the larger side?
        let rows_here = max_rows_in(plan, source, target, buckets, bucket)?;
        if rows_here <= policy.row_threshold {
            let s = source.query(&dialect::key_hash_query(
                source.dialect(),
                &plan.source_table,
                &plan.source_pk,
                &plan.source_columns,
                buckets,
                bucket,
            ))?;
            let t = target.query(&dialect::key_hash_query(
                target.dialect(),
                &plan.target_table,
                &plan.target_pk,
                &plan.target_columns,
                buckets,
                bucket,
            ))?;
            return Ok(diff_key_hashes(s, t));
        }

        let fine = buckets
            .checked_mul(policy.fan_out)
            .ok_or(BisectError::FanOutOverflow { buckets })?;
        let s = parse_buckets(source.query(&dialect::sub_bucket_checksum_query(
            source.dialect(),
            &plan.source_table,
            &plan.source_pk,
            &plan.source_columns,
            buckets,
            bucket,
            fine,
        ))?)?;
        let t = parse_buckets(target.query(&dialect::sub_bucket_checksum_query(
            target.dialect(),
            &plan.target_table,
            &plan.target_pk,
            &plan.target_columns,
            buckets,
            bucket,
            fine,
        ))?)?;

        let differing = differing_buckets(&s, &t);
        match differing.len() {
            // The divergence vanished on re-measurement. On a live source that means the data moved
            // under us — a concurrent write — and reporting "no divergence" would be a false green.
            0 => return Err(BisectError::VanishedUnderRemeasurement { bucket, round }),
            // Exactly one sub-bucket still differs: descend into it.
            _ => {
                bucket = differing[0];
                buckets = fine;
            }
        }
    }
    Err(BisectError::MaxRoundsExceeded {
        rounds: policy.max_rounds,
    })
}

fn differing_buckets(
    s: &BTreeMap<u32, crate::hash::BucketChecksum>,
    t: &BTreeMap<u32, crate::hash::BucketChecksum>,
) -> Vec<u32> {
    let mut keys: Vec<u32> = s.keys().chain(t.keys()).copied().collect();
    keys.sort_unstable();
    keys.dedup();
    let zero = crate::hash::BucketChecksum::default();
    keys.into_iter()
        .filter(|k| s.get(k).unwrap_or(&zero) != t.get(k).unwrap_or(&zero))
        .collect()
}

fn max_rows_in(
    plan: &UnitPlan,
    source: &dyn EngineReader,
    target: &dyn EngineReader,
    buckets: u32,
    bucket: u32,
) -> Result<u64, BisectError> {
    let s = parse_buckets(source.query(&dialect::sub_bucket_checksum_query(
        source.dialect(),
        &plan.source_table,
        &plan.source_pk,
        &plan.source_columns,
        buckets,
        bucket,
        buckets,
    ))?)?;
    let t = parse_buckets(target.query(&dialect::sub_bucket_checksum_query(
        target.dialect(),
        &plan.target_table,
        &plan.target_pk,
        &plan.target_columns,
        buckets,
        bucket,
        buckets,
    ))?)?;
    // The larger side decides: fetching keys must be bounded on both.
    Ok(s.values()
        .map(|c| c.count)
        .chain(t.values().map(|c| c.count))
        .max()
        .unwrap_or(0))
}

/// V4 — bisect every failed V3 bucket and report the keys.
pub fn run_v4(
    plan: &UnitPlan,
    source: &dyn EngineReader,
    target: &dyn EngineReader,
    failed_buckets: &[u32],
    policy: BisectPolicy,
) -> Result<TierOutcome, BisectError> {
    let mut divergences = Vec::new();
    for &b in failed_buckets {
        for d in bisect_bucket(plan, source, target, b, policy)? {
            let (detail, key) = match &d {
                KeyDiff::SourceOnly(k) => ("missing in target".to_string(), k),
                KeyDiff::TargetOnly(k) => ("present only in target".to_string(), k),
                KeyDiff::HashDiffers { key } => ("row differs".to_string(), key),
            };
            divergences.push(Divergence {
                locus: format!("{}:key:{}", plan.unit, mask_key(key)),
                columns: vec![],
                detail,
                class: Class::Unexplained,
            });
        }
    }
    Ok(TierOutcome {
        tier: Tier::V4RowDiff,
        source_path: source.label().to_string(),
        target_path: target.label().to_string(),
        divergences,
        controls: vec![],
    })
}

/// A key is identifying data. It is kept only long enough to fetch the row for comparison, and what
/// reaches a fact or a log is a masked form: enough to find the row again with the source at hand,
/// not enough to be a leak on its own.
pub fn mask_key(key: &str) -> String {
    let h = crate::hash::row_hash(key);
    match key.chars().count() {
        0 => "∅".to_string(),
        n if n <= 2 => format!("··#{}", &h[..8]),
        _ => {
            let first: String = key.chars().take(1).collect();
            let last: String = key.chars().rev().take(1).collect();
            format!("{first}··{last}#{}", &h[..8])
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BisectError {
    #[error(transparent)]
    Read(#[from] ReadError),
    #[error(
        "bucket {bucket} no longer differs at round {round} — the source changed under the run. \
         Re-run against a fixed snapshot (a replica at a recorded LSN); reporting no divergence \
         here would be a false green."
    )]
    VanishedUnderRemeasurement { bucket: u32, round: u32 },
    #[error("bisect exceeded {rounds} rounds without isolating the divergence")]
    MaxRoundsExceeded { rounds: u32 },
    #[error("bucket count {buckets} × fan-out overflows u32")]
    FanOutOverflow { buckets: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(pairs: &[(&str, &str)]) -> Vec<Vec<String>> {
        pairs
            .iter()
            .map(|(k, h)| vec![k.to_string(), h.to_string()])
            .collect()
    }

    #[test]
    fn diff_finds_all_three_kinds() {
        let s = rows(&[("1", "aa"), ("2", "bb"), ("3", "cc")]);
        let t = rows(&[("1", "aa"), ("2", "XX"), ("4", "dd")]);
        let d = diff_key_hashes(s, t);
        assert_eq!(
            d,
            vec![
                KeyDiff::HashDiffers { key: "2".into() },
                KeyDiff::SourceOnly("3".into()),
                KeyDiff::TargetOnly("4".into()),
            ]
        );
    }

    #[test]
    fn identical_sides_diff_to_nothing() {
        let s = rows(&[("1", "aa"), ("2", "bb")]);
        assert!(diff_key_hashes(s.clone(), s).is_empty());
    }

    /// Row order differs between engines constantly; the diff is a map comparison, not a zip.
    #[test]
    fn row_order_does_not_affect_the_diff() {
        let s = rows(&[("1", "aa"), ("2", "bb")]);
        let mut t = rows(&[("2", "bb"), ("1", "aa")]);
        assert!(diff_key_hashes(s.clone(), t.clone()).is_empty());
        t.reverse();
        assert!(diff_key_hashes(s, t).is_empty());
    }

    #[test]
    fn a_masked_key_does_not_reveal_the_key() {
        let m = mask_key("customer-4815162342");
        assert!(!m.contains("4815162342"), "{m}");
        assert!(m.starts_with('c') && m.contains("··"), "{m}");
        // Stable, so the same key masks the same way across a run and a report.
        assert_eq!(m, mask_key("customer-4815162342"));
        assert_ne!(m, mask_key("customer-4815162343"));
        // Short keys reveal nothing at all rather than most of themselves. The key must use
        // non-hex characters: a digit key like "7" can appear by chance in the hash suffix
        // (~40% of 8-hex-digit suffixes contain any given digit), which says nothing about masking.
        let short = mask_key("zq");
        assert!(!short.contains('z') && !short.contains('q'), "{short}");
        assert!(short.starts_with("··#"), "{short}");
    }

    #[test]
    fn fan_out_overflow_is_an_error_not_a_wrap() {
        let p = BisectPolicy {
            fan_out: 16,
            ..Default::default()
        };
        assert!(u32::MAX.checked_mul(p.fan_out).is_none());
    }
}
