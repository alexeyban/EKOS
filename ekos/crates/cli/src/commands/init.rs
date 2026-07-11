use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use std::path::Path;

pub fn run(config: &EkosConfig, cwd: &Path) -> Result<()> {
    let ekos_dir = config.ekos_dir(cwd);
    let artifact_dir = config.artifact_dir(cwd);
    let ledger_dir = ekos_dir.join("ledger");
    let config_dir = ekos_dir.join("config");

    for dir in [&ekos_dir, &artifact_dir, &ledger_dir, &config_dir] {
        std::fs::create_dir_all(dir)?;
        tracing::debug!("created {}", dir.display());
    }

    let ekos_toml = cwd.join("ekos.toml");
    if !ekos_toml.exists() {
        std::fs::write(&ekos_toml, DEFAULT_CONFIG)?;
        println!("Created ekos.toml");
    } else {
        println!("ekos.toml already exists — skipping");
    }

    println!("Initialized .ekos/ workspace at {}", ekos_dir.display());
    println!("  artifacts: {}", artifact_dir.display());
    println!("  ledger:    {}", ledger_dir.display());
    Ok(())
}

const DEFAULT_CONFIG: &str = r#"[workspace]
root = "."
log-level = "info"
log-format = "pretty"

[observe]
paths = ["."]
ignore-patterns = [".ekos", ".git", "target", "node_modules"]
"#;
