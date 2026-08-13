//! `ekos simulate <scenario.yaml>` (RFC 0051) — the CLI entry point every
//! prior RFC in the World Engine continuation deferred. Loads a scenario,
//! runs it for the configured (or overridden) number of rounds, and prints
//! a per-round, per-agent summary.
//!
//! Deliberately does **not** use `commands::store::open_store` (the helper
//! every other command uses to open the workspace's one real ledger):
//! `SimulatedAgent`/`Proposition`/`Custom("ActionExecuted")` entities are
//! fictional and regenerated on every run, and the ledger has no
//! delete/tombstone mechanism anywhere in this codebase (RFC 0043) — writing
//! them into the real workspace ledger by default would permanently
//! commingle fictional simulation data with real, evidence-backed compiled
//! knowledge, with no way to undo it. `--ledger` opts back in explicitly.

use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_ledger::Ledger;
use ekos_simulation::{ScenarioDefinition, Simulation, load_scenario_from_path};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Where a scenario's own ledger lives unless `--ledger` overrides it:
/// `.ekos/simulations/<scenario-id>/ledger.db`, a sibling of — never the
/// same file as — the real workspace ledger. Shared with `commands::replay`
/// (RFC 0054), which needs to resolve the same path without re-running the
/// scenario.
pub(crate) fn scenario_ledger_path(config: &EkosConfig, cwd: &Path, scenario_id: &str) -> PathBuf {
    config
        .ekos_dir(cwd)
        .join("simulations")
        .join(scenario_id)
        .join("ledger.db")
}

pub fn run(
    config: &EkosConfig,
    cwd: &Path,
    scenario_path: &Path,
    rounds_override: Option<u32>,
    ledger_override: Option<PathBuf>,
    seed_override: Option<u64>,
) -> Result<()> {
    let content = std::fs::read_to_string(scenario_path)
        .map_err(|e| anyhow::anyhow!("cannot read scenario {}: {e}", scenario_path.display()))?;
    let peek: ScenarioDefinition = serde_yaml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("cannot parse scenario {}: {e}", scenario_path.display()))?;

    let ledger_path =
        ledger_override.unwrap_or_else(|| scenario_ledger_path(config, cwd, &peek.id));
    if let Some(parent) = ledger_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let ledger = Ledger::open(&ledger_path).map_err(|e| {
        anyhow::anyhow!(
            "cannot open simulation ledger at {}: {e}",
            ledger_path.display()
        )
    })?;

    let mut loaded = load_scenario_from_path(&ledger, scenario_path)
        .map_err(|e| anyhow::anyhow!("failed to load scenario {}: {e}", scenario_path.display()))?;
    if let Some(seed) = seed_override {
        loaded.config.seed = Some(seed);
    }

    println!("Scenario: {} ({})", peek.name, peek.id);
    println!("Ledger:   {}", ledger_path.display());
    println!("Agents:   {}", loaded.config.agents.len());
    println!(
        "Seed:     {}",
        loaded
            .config
            .seed
            .map(|s| s.to_string())
            .unwrap_or_else(|| "none (defaults to a fixed, still-reproducible seed)".to_string())
    );

    let rounds = rounds_override.unwrap_or(loaded.rounds);
    let names_by_id: HashMap<_, _> = loaded
        .names
        .iter()
        .map(|(name, id)| (*id, name.clone()))
        .collect();

    let sim = Simulation::new(&ledger, loaded.config, loaded.engines);
    for round in 0..rounds {
        let result = sim.run_round(round)?;
        println!("\nRound {round}:");
        for (agent_id, decision) in &result.decisions {
            let agent_name = names_by_id.get(agent_id).map(|s| s.as_str()).unwrap_or("?");
            let target = decision
                .action
                .target
                .and_then(|t| names_by_id.get(&t))
                .map(|s| s.as_str())
                .unwrap_or("-");
            println!("  {agent_name} -> {:?}({target})", decision.action.kind);
        }
        for (agent_id, err) in &result.validation_failures {
            let agent_name = names_by_id.get(agent_id).map(|s| s.as_str()).unwrap_or("?");
            println!("  [validation failed] {agent_name}: {err}");
        }
        for (agent_id, err) in &result.conflict_failures {
            let agent_name = names_by_id.get(agent_id).map(|s| s.as_str()).unwrap_or("?");
            println!("  [conflict] {agent_name}: {err}");
        }
    }

    Ok(())
}
