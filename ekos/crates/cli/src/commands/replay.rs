//! `ekos replay <scenario.yaml> [--round N]` (RFC 0054) — reads a
//! *previously recorded* simulation back out of its ledger. Deliberately
//! does not call `load_scenario_from_path` (which always writes, as a
//! loader): replay only reads, so names are resolved straight from the
//! ledger's own `all_objects()`, never by re-appending scenario data.

use super::simulate::scenario_ledger_path;
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_kir::KirId;
use ekos_ledger::Ledger;
use ekos_simulation::{Replay, ScenarioDefinition};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub fn run(
    config: &EkosConfig,
    cwd: &Path,
    scenario_path: &Path,
    round: Option<u32>,
    ledger_override: Option<PathBuf>,
) -> Result<()> {
    let content = std::fs::read_to_string(scenario_path)
        .map_err(|e| anyhow::anyhow!("cannot read scenario {}: {e}", scenario_path.display()))?;
    let peek: ScenarioDefinition = serde_yaml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("cannot parse scenario {}: {e}", scenario_path.display()))?;

    let ledger_path =
        ledger_override.unwrap_or_else(|| scenario_ledger_path(config, cwd, &peek.id));
    let ledger = Ledger::open(&ledger_path).map_err(|e| {
        anyhow::anyhow!(
            "cannot open simulation ledger at {}: {e}",
            ledger_path.display()
        )
    })?;

    let replay = Replay::open(&ledger).map_err(|e| {
        anyhow::anyhow!(
            "cannot replay {}: {e} — has `ekos simulate {}` been run yet?",
            ledger_path.display(),
            scenario_path.display()
        )
    })?;

    let names_by_id: HashMap<KirId, String> = ledger
        .all_objects()?
        .into_iter()
        .map(|o| (o.id, o.name))
        .collect();
    let resolve = |raw: &str| -> String {
        KirId::from_str(raw)
            .ok()
            .and_then(|id| names_by_id.get(&id).cloned())
            .unwrap_or_else(|| raw.to_string())
    };

    println!("Scenario: {} ({})", peek.name, peek.id);
    println!("Ledger:   {}", ledger_path.display());

    let rounds = match round {
        Some(r) => vec![r],
        None => replay.rounds()?,
    };

    for r in rounds {
        let events = replay.events_in_round(r)?;
        println!("\nRound {r}:");
        for event in &events {
            let actor = event.payload["actor"]
                .as_str()
                .map(resolve)
                .unwrap_or_else(|| "?".to_string());
            let kind = event.payload["kind"].as_str().unwrap_or("?");
            let target = event.payload["target"]
                .as_str()
                .map(resolve)
                .unwrap_or_else(|| "-".to_string());
            let observers: Vec<String> = replay
                .observed_by(&event.id)?
                .iter()
                .map(|id| {
                    names_by_id
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| id.to_string())
                })
                .collect();
            println!(
                "  {actor} -> {kind}({target})  [observed by: {}]",
                observers.join(", ")
            );
        }
    }

    Ok(())
}
