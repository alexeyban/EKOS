use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use std::path::Path;

struct Check {
    label: &'static str,
    ok: bool,
    detail: String,
}

impl Check {
    fn ok(label: &'static str, detail: impl Into<String>) -> Self {
        Self {
            label,
            ok: true,
            detail: detail.into(),
        }
    }

    fn fail(label: &'static str, detail: impl Into<String>) -> Self {
        Self {
            label,
            ok: false,
            detail: detail.into(),
        }
    }
}

pub fn run(config: &EkosConfig, cwd: &Path, config_path: &Path) -> Result<()> {
    let mut checks = Vec::new();

    // Rust toolchain version
    let rust_version = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    match rust_version {
        Some(v) => checks.push(Check::ok("Rust toolchain", v)),
        None => checks.push(Check::fail("Rust toolchain", "rustc not found in PATH")),
    }

    // Workspace root
    checks.push(Check::ok("Working directory", cwd.display().to_string()));

    // ekos.toml
    if config_path.exists() {
        checks.push(Check::ok("ekos.toml", config_path.display().to_string()));
    } else {
        checks.push(Check::fail(
            "ekos.toml",
            format!("{} not found — run `ekos init`", config_path.display()),
        ));
    }

    // .ekos/ directory
    let ekos_dir = config.ekos_dir(cwd);
    if ekos_dir.exists() {
        checks.push(Check::ok(".ekos/", ekos_dir.display().to_string()));
    } else {
        checks.push(Check::fail(
            ".ekos/",
            format!("{} not found — run `ekos init`", ekos_dir.display()),
        ));
    }

    // Artifact cache writability
    let artifact_dir = config.artifact_dir(cwd);
    if artifact_dir.exists() {
        let writable = std::fs::write(artifact_dir.join(".probe"), b"")
            .map(|_| {
                std::fs::remove_file(artifact_dir.join(".probe")).ok();
                true
            })
            .unwrap_or(false);
        if writable {
            checks.push(Check::ok(
                "Artifact cache",
                artifact_dir.display().to_string(),
            ));
        } else {
            checks.push(Check::fail("Artifact cache", "not writable"));
        }
    } else {
        checks.push(Check::fail("Artifact cache", "not found — run `ekos init`"));
    }

    // LLM config
    if let Some(ref provider) = config.llm.provider {
        let key_var = config
            .llm
            .api_key_env
            .as_deref()
            .unwrap_or("ANTHROPIC_API_KEY");
        if std::env::var(key_var).is_ok() {
            checks.push(Check::ok(
                "LLM provider",
                format!("{provider} (key: ${key_var} ✓)"),
            ));
        } else {
            checks.push(Check::fail(
                "LLM provider",
                format!("{provider} configured but ${key_var} is not set"),
            ));
        }
    } else {
        checks.push(Check::ok(
            "LLM provider",
            "not configured (required for Phase 6+)",
        ));
    }

    println!("EKOS Doctor");
    println!("{}", "─".repeat(40));
    let mut all_ok = true;
    for check in &checks {
        let status = if check.ok { "[OK]  " } else { "[FAIL]" };
        println!("{status} {:<20} {}", check.label, check.detail);
        if !check.ok {
            all_ok = false;
        }
    }
    println!("{}", "─".repeat(40));

    if all_ok {
        println!("All checks passed.");
        Ok(())
    } else {
        anyhow::bail!("Some checks failed — see above.")
    }
}
