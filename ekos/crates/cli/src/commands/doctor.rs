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

/// RFC 0076: this used to default the checked env var to `ANTHROPIC_API_KEY` regardless of which
/// provider was actually configured — a real, confirmed false-negative found live against a real
/// workspace with `provider = "ollama"` and Ollama running correctly locally: this check reported
/// `[FAIL] ... ollama configured but $ANTHROPIC_API_KEY is not set`, true but irrelevant
/// (`ekos_recovery::ollama::OllamaProvider::from_env` reads `OLLAMA_BASE_URL`/`OLLAMA_MODEL`, both
/// optional with sensible defaults — no API key exists for a local Ollama server at all). Ollama
/// is the one built-in provider with no key requirement; every other provider (today: Anthropic)
/// keeps the original check. `key_is_set` is injected (rather than calling `std::env::var`
/// directly) so this is testable without mutating real process environment variables, which would
/// race across parallel test threads.
fn llm_provider_check(
    provider: Option<&str>,
    api_key_env: Option<&str>,
    key_is_set: impl Fn(&str) -> bool,
) -> Check {
    let Some(provider) = provider else {
        return Check::ok("LLM provider", "not configured (required for Phase 6+)");
    };

    if provider == "ollama" {
        return Check::ok(
            "LLM provider",
            format!("{provider} (local provider, no API key required)"),
        );
    }

    let key_var = api_key_env.unwrap_or("ANTHROPIC_API_KEY");
    if key_is_set(key_var) {
        Check::ok("LLM provider", format!("{provider} (key: ${key_var} ✓)"))
    } else {
        Check::fail(
            "LLM provider",
            format!("{provider} configured but ${key_var} is not set"),
        )
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
    checks.push(llm_provider_check(
        config.llm.provider.as_deref(),
        config.llm.api_key_env.as_deref(),
        |key_var| std::env::var(key_var).is_ok(),
    ));

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_passes_regardless_of_any_api_key_env_var() {
        let check = llm_provider_check(Some("ollama"), None, |_| false);
        assert!(check.ok, "ollama needs no API key at all");
        assert!(check.detail.contains("no API key required"));
    }

    #[test]
    fn anthropic_fails_when_its_key_env_var_is_unset() {
        let check = llm_provider_check(Some("anthropic"), None, |_| false);
        assert!(!check.ok);
        assert!(check.detail.contains("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn anthropic_passes_when_its_key_env_var_is_set() {
        let check = llm_provider_check(Some("anthropic"), None, |_| true);
        assert!(check.ok);
    }

    #[test]
    fn a_custom_api_key_env_name_is_respected() {
        let check = llm_provider_check(Some("anthropic"), Some("MY_KEY"), |var| var == "MY_KEY");
        assert!(check.ok);
        assert!(check.detail.contains("MY_KEY"));
    }

    #[test]
    fn no_provider_configured_is_ok_not_a_failure() {
        let check = llm_provider_check(None, None, |_| false);
        assert!(check.ok, "no LLM configured is a valid, non-failing state");
    }
}
