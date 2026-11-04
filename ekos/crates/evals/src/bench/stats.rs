//! Sampling noise and paired comparison (RFC 0240 §2.6).
//!
//! A single accuracy on a few hundred questions moves by several points from sampling alone, so
//! every report carries a bootstrap 95% interval and `compare` pairs two runs question by
//! question. The resampling uses a fixed-seed SplitMix64, so an interval is reproducible.

use serde::{Deserialize, Serialize};

/// Resamples per interval.
pub const RESAMPLES: usize = 2000;
const SEED: u64 = 0x0240_b1d5;

struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// A mean and its bootstrap 95% interval, in the same unit as the samples.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Interval {
    pub n: usize,
    pub mean: f64,
    pub low: f64,
    pub high: f64,
}

impl Interval {
    /// Whether the interval leaves zero out — the only case `compare` calls a difference.
    pub fn excludes_zero(&self) -> bool {
        self.low > 0.0 || self.high < 0.0
    }
}

/// Percentile bootstrap of the mean of `xs`.
pub fn bootstrap_mean(xs: &[f64]) -> Interval {
    let n = xs.len();
    if n == 0 {
        return Interval {
            n,
            mean: 0.0,
            low: 0.0,
            high: 0.0,
        };
    }
    let mean = xs.iter().sum::<f64>() / n as f64;
    let mut rng = SplitMix64(SEED ^ n as u64);
    let mut means = Vec::with_capacity(RESAMPLES);
    for _ in 0..RESAMPLES {
        let mut sum = 0.0;
        for _ in 0..n {
            sum += xs[rng.below(n)];
        }
        means.push(sum / n as f64);
    }
    means.sort_by(|a, b| a.total_cmp(b));
    Interval {
        n,
        mean,
        low: means[(RESAMPLES as f64 * 0.025) as usize],
        high: means[((RESAMPLES as f64 * 0.975) as usize).min(RESAMPLES - 1)],
    }
}

/// McNemar's exact test on the discordant pairs: `b` questions only A got right, `c` only B.
/// Returns the two-sided p-value.
pub fn mcnemar_exact(b: usize, c: usize) -> f64 {
    let n = b + c;
    if n == 0 {
        return 1.0;
    }
    let k = b.min(c);
    // P(X ≤ k) for X ~ Binomial(n, 1/2), doubled, in log space to stay finite.
    let ln_half_n = n as f64 * 0.5f64.ln();
    let mut ln_choose = 0.0f64; // ln C(n, 0)
    let mut tail = 0.0;
    for i in 0..=k {
        if i > 0 {
            ln_choose += ((n - i + 1) as f64).ln() - (i as f64).ln();
        }
        tail += (ln_choose + ln_half_n).exp();
    }
    (2.0 * tail).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_runs_give_an_interval_that_contains_zero() {
        let diffs = vec![0.0; 200];
        let i = bootstrap_mean(&diffs);
        assert_eq!(i.mean, 0.0);
        assert!(!i.excludes_zero());
    }

    #[test]
    fn a_known_difference_is_detected_and_a_small_one_is_not() {
        // 200 paired questions; A right on 60 more than B.
        let mut diffs = vec![0.0; 140];
        diffs.extend(vec![1.0; 60]);
        let i = bootstrap_mean(&diffs);
        assert!((i.mean - 0.3).abs() < 1e-12);
        assert!(i.excludes_zero(), "{i:?}");
        // Two flips each way: no difference.
        let mut diffs = vec![0.0; 196];
        diffs.extend([1.0, 1.0, -1.0, -1.0]);
        assert!(!bootstrap_mean(&diffs).excludes_zero());
    }

    #[test]
    fn the_interval_is_reproducible() {
        let xs: Vec<f64> = (0..300).map(|i| (i % 3 == 0) as u8 as f64).collect();
        assert_eq!(bootstrap_mean(&xs), bootstrap_mean(&xs));
        let i = bootstrap_mean(&xs);
        // ~±5 points at 95% for n=300, p≈1/3.
        assert!(i.high - i.low > 0.06 && i.high - i.low < 0.14, "{i:?}");
    }

    #[test]
    fn mcnemar_matches_known_values() {
        assert_eq!(mcnemar_exact(0, 0), 1.0);
        // b=0, c=6: 2 × 0.5^6 = 0.03125.
        assert!((mcnemar_exact(0, 6) - 0.03125).abs() < 1e-12);
        assert_eq!(mcnemar_exact(5, 5), 1.0);
    }
}
