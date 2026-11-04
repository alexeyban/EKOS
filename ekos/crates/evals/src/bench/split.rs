//! The tuning / hold-out split (RFC 0240 §2.3 item 5, §6).
//!
//! Fixed seed, stratified by database and difficulty: within each stratum the questions are
//! ordered by a hash of (seed, id) and the first `round(len × tune_fraction)` go to tuning. No
//! random-number generator is involved, so the split is the same on every machine and release.

use super::suite::SplitSpec;
use super::{BenchScenario, Split};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// The scenarios of `split`, in their original order.
pub fn select(all: &[BenchScenario], spec: &SplitSpec, split: Split) -> Vec<BenchScenario> {
    if split == Split::All {
        return all.to_vec();
    }
    let tune = tune_ids(all, spec);
    all.iter()
        .filter(|s| tune.contains(&s.id) == (split == Split::Tune))
        .cloned()
        .collect()
}

fn tune_ids(all: &[BenchScenario], spec: &SplitSpec) -> std::collections::BTreeSet<String> {
    let mut strata: BTreeMap<(&str, &str), Vec<&BenchScenario>> = BTreeMap::new();
    for s in all {
        strata
            .entry((s.db_id.as_str(), s.difficulty.as_str()))
            .or_default()
            .push(s);
    }
    let mut tune = std::collections::BTreeSet::new();
    for members in strata.values_mut() {
        members.sort_by_key(|s| order_key(spec.seed, &s.id));
        let take = (members.len() as f64 * spec.tune_fraction).round() as usize;
        tune.extend(members.iter().take(take).map(|s| s.id.clone()));
    }
    tune
}

fn order_key(seed: u64, id: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(seed.to_le_bytes());
    h.update(id.as_bytes());
    h.finalize().into()
}

/// A deterministic subset of `n` scenarios: ordered by a hash of (seed, id), the first `n` kept,
/// returned in their original order. Spread over databases and difficulties in proportion, so a
/// short run is not all from the first database. Recorded through the report's question-id hash.
pub fn sample(scenarios: &[BenchScenario], seed: u64, n: usize) -> Vec<BenchScenario> {
    let mut keyed: Vec<(usize, [u8; 32])> = scenarios
        .iter()
        .enumerate()
        .map(|(i, s)| (i, order_key(seed ^ 0x5A4D_504C, &s.id)))
        .collect();
    keyed.sort_by_key(|k| k.1);
    let mut keep: Vec<usize> = keyed.into_iter().take(n).map(|k| k.0).collect();
    keep.sort_unstable();
    keep.into_iter().map(|i| scenarios[i].clone()).collect()
}

/// SHA-256 over the sorted question ids — what a report records as "the questions it ran on".
pub fn ids_hash(scenarios: &[BenchScenario]) -> String {
    let mut ids: Vec<&str> = scenarios.iter().map(|s| s.id.as_str()).collect();
    ids.sort_unstable();
    super::suite::sha256_hex(ids.join("\n").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenario(id: usize, db: &str, difficulty: &str) -> BenchScenario {
        BenchScenario {
            id: id.to_string(),
            db_id: db.into(),
            question: format!("q{id}"),
            evidence: String::new(),
            gold_sql: "SELECT 1".into(),
            difficulty: difficulty.into(),
        }
    }

    #[test]
    fn tune_and_hold_partition_every_stratum_and_are_stable() {
        let mut all = Vec::new();
        for i in 0..100 {
            let db = if i % 2 == 0 { "a" } else { "b" };
            let d = ["simple", "moderate", "challenging"][i % 3];
            all.push(scenario(i, db, d));
        }
        let spec = SplitSpec::default();
        let tune = select(&all, &spec, Split::Tune);
        let hold = select(&all, &spec, Split::Hold);
        assert_eq!(tune.len() + hold.len(), all.len());
        assert!(tune.iter().all(|t| !hold.iter().any(|h| h.id == t.id)));
        // Roughly 40% of each stratum.
        assert!((35..=45).contains(&tune.len()), "{}", tune.len());
        assert_eq!(ids_hash(&tune), ids_hash(&select(&all, &spec, Split::Tune)));
        let other_seed = SplitSpec {
            seed: 7,
            ..spec.clone()
        };
        assert_ne!(
            ids_hash(&tune),
            ids_hash(&select(&all, &other_seed, Split::Tune))
        );
    }

    #[test]
    fn a_sample_is_stable_ordered_and_spread() {
        let all: Vec<BenchScenario> = (0..100)
            .map(|i| scenario(i, if i < 50 { "a" } else { "b" }, "simple"))
            .collect();
        let s = sample(&all, 240, 20);
        assert_eq!(s.len(), 20);
        assert_eq!(s, sample(&all, 240, 20));
        let ids: Vec<usize> = s.iter().map(|x| x.id.parse().unwrap()).collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "original order kept");
        let from_a = s.iter().filter(|x| x.db_id == "a").count();
        assert!((5..=15).contains(&from_a), "{from_a}");
    }
}
