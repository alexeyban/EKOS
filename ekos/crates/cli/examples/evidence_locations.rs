//! Measures how many rendered evidence claims carry a source location, across a whole eval
//! dataset. This is RFC 0140's headline metric.
//!
//! **Why this exists as an example rather than a test or a `--flag`.** The baseline it compares
//! against ("0 of 1,289 claims carried a line, 26.4% carried any location") came from the
//! `evidence_text` of a full `ekos eval run`, which spends an LLM call per scenario and takes
//! hours. But the evidence set is produced by `plan_question` + `execute`, which are **offline and
//! deterministic** — no LLM, no network. So the exact same measurement runs over all 101 scenarios
//! in seconds, and reruns identically. It reads the developer's real workspace ledger, which is
//! why it is not a CI test.
//!
//! ```text
//! cargo run -p ekos --example evidence_locations
//! cargo run -p ekos --example evidence_locations -- --dataset ekos-full --workspace /path/to/ws
//! ```

use ekos_compiler_core::EkosConfig;
use ekos_runtime::Runtime;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A claim location that names a line: `path:200` or a span `path:200-322`.
fn has_line(location: &str) -> bool {
    let Some((_, tail)) = location.rsplit_once(':') else {
        return false;
    };
    !tail.is_empty()
        && tail
            .split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

#[derive(Default, Clone, Copy)]
struct Tally {
    scenarios: usize,
    claims: usize,
    with_any_location: usize,
    with_line: usize,
    /// Scenarios whose plan produced no claims at all — these contribute no denominator, and
    /// silently dropping them would flatter every percentage below.
    empty: usize,
}

impl Tally {
    fn add(&mut self, other: &Tally) {
        self.scenarios += other.scenarios;
        self.claims += other.claims;
        self.with_any_location += other.with_any_location;
        self.with_line += other.with_line;
        self.empty += other.empty;
    }
}

fn pct(n: usize, d: usize) -> String {
    if d == 0 {
        return "   n/a".to_string();
    }
    format!("{:5.1}%", 100.0 * n as f64 / d as f64)
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    let cwd = arg("--workspace")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    let dataset = arg("--dataset");
    let datasets_dir = arg("--datasets-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.join("evals").join("datasets"));

    let config = EkosConfig::from_file_or_default(&cwd.join("ekos.toml"));
    let (dataset_name, scenarios) =
        ekos_evals::schema::load_dataset(dataset.as_deref(), &datasets_dir)?;

    let store = ekos::commands::store::open_store_read_only(&config, &cwd)?;
    let runtime = Runtime::over(&*store);

    println!("dataset  : {dataset_name} ({} scenarios)", scenarios.len());
    println!("workspace: {}\n", cwd.display());

    let mut by_category: BTreeMap<String, Tally> = BTreeMap::new();
    let mut total = Tally::default();
    // Kept so the report can show what a claim actually looks like now — a percentage alone does
    // not tell you whether the location is a bare file or a real span.
    let mut samples: Vec<String> = Vec::new();

    for scenario in &scenarios {
        let mut t = Tally {
            scenarios: 1,
            ..Default::default()
        };

        match ekos_runtime::reason::plan_question(&scenario.question, &runtime)
            .and_then(|plan| ekos_runtime::reason::execute(&plan, &runtime))
        {
            Ok(set) => {
                if set.items.is_empty() {
                    t.empty = 1;
                }
                for item in &set.items {
                    t.claims += 1;
                    if !item.location.is_empty() {
                        t.with_any_location += 1;
                        if has_line(&item.location) {
                            t.with_line += 1;
                        }
                        // Sampled unconditionally: sampling only line-carrying claims made the
                        // report print nothing precisely when the count was 0 — i.e. exactly when
                        // an example was needed to tell a real regression from a broken measure.
                        if samples.len() < 8 {
                            samples.push(format!(
                                "line={} loc={:?}",
                                has_line(&item.location),
                                item.location
                            ));
                        }
                    }
                }
            }
            // A planning/execution failure is not "zero claims" — report it rather than letting it
            // quietly shrink the denominator.
            Err(e) => eprintln!("  ! {} failed to plan/execute: {e}", scenario.id),
        }

        by_category
            .entry(scenario.category.clone())
            .or_default()
            .add(&t);
        total.add(&t);
    }

    println!(
        "{:<16}{:>7}{:>9}{:>10}{:>9}{:>10}{:>8}",
        "category", "scen", "claims", "any-loc", "%", "line", "%"
    );
    for (category, t) in &by_category {
        println!(
            "{:<16}{:>7}{:>9}{:>10}{:>9}{:>10}{:>8}",
            category,
            t.scenarios,
            t.claims,
            t.with_any_location,
            pct(t.with_any_location, t.claims),
            t.with_line,
            pct(t.with_line, t.claims),
        );
    }
    println!(
        "\n{:<16}{:>7}{:>9}{:>10}{:>9}{:>10}{:>8}",
        "TOTAL",
        total.scenarios,
        total.claims,
        total.with_any_location,
        pct(total.with_any_location, total.claims),
        total.with_line,
        pct(total.with_line, total.claims),
    );
    println!(
        "\nscenarios producing no claims at all: {} of {}",
        total.empty, total.scenarios
    );
    println!(
        "baseline before RFC 0140 §1/§2: 0 of 1289 claims with a line, 26.4% with any location"
    );

    if !samples.is_empty() {
        println!("\nsample claims carrying a line:");
        for s in &samples {
            println!("  {}", s.chars().take(150).collect::<String>());
        }
    }

    Ok(())
}
