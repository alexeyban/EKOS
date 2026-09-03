use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use serde::Serialize;
use std::path::Path;

struct Check {
    label: &'static str,
    ok: bool,
    detail: String,
}

/// RFC 0129 R5 — the machine-readable form of `ekos doctor`. Mirrors RFC 0127 R2's `StatusJson`
/// in style: one flat object, `schema_version` first, text output left byte-identical.
#[derive(Debug, Serialize)]
pub struct DoctorJson {
    pub schema_version: u32,
    /// `true` iff no check has `status == "fail"`.
    pub ok: bool,
    pub checks: Vec<DoctorCheckJson>,
}

#[derive(Debug, Serialize)]
pub struct DoctorCheckJson {
    pub name: &'static str,
    /// `"ok"` or `"fail"`. `"warn"` is reserved — `ekos doctor` produces no warning-level checks
    /// today, but a consumer should treat any non-`"ok"` value as a problem.
    pub status: &'static str,
    pub detail: String,
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

pub fn run(config: &EkosConfig, cwd: &Path, config_path: &Path, json: bool) -> Result<()> {
    let checks = collect_checks(config, cwd, config_path);
    let all_ok = checks.iter().all(|c| c.ok);

    // RFC 0129 R5: `--json` is a pure alternate presentation over the same checks — it prints one
    // object and always exits 0 (the `ok` field carries the verdict), the same contract as
    // `status --json`. A machine consumer reads `ok`, it does not inspect the exit code.
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&build_doctor_json(&checks))?
        );
        return Ok(());
    }

    println!("EKOS Doctor");
    println!("{}", "─".repeat(40));
    for check in &checks {
        let status = if check.ok { "[OK]  " } else { "[FAIL]" };
        println!("{status} {:<20} {}", check.label, check.detail);
    }
    println!("{}", "─".repeat(40));

    if all_ok {
        println!("All checks passed.");
        Ok(())
    } else {
        anyhow::bail!("Some checks failed — see above.")
    }
}

/// Serialize a check list into the RFC 0129 R5 shape. `ok` is `true` iff every check passed.
fn build_doctor_json(checks: &[Check]) -> DoctorJson {
    DoctorJson {
        schema_version: 1,
        ok: checks.iter().all(|c| c.ok),
        checks: checks
            .iter()
            .map(|c| DoctorCheckJson {
                name: c.label,
                status: if c.ok { "ok" } else { "fail" },
                detail: c.detail.clone(),
            })
            .collect(),
    }
}

fn collect_checks(config: &EkosConfig, cwd: &Path, config_path: &Path) -> Vec<Check> {
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

    checks
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

    // ── RFC 0129 R5 — `ekos doctor --json` ──────────────────────────────────

    #[test]
    fn doctor_json_shape_and_all_ok_flag() {
        let checks = vec![
            Check::ok("Rust toolchain", "rustc 1.98.0"),
            Check::ok(".ekos/", "/tmp/ws/.ekos"),
        ];
        let out = build_doctor_json(&checks);
        assert_eq!(out.schema_version, 1);
        assert!(out.ok);
        assert_eq!(out.checks.len(), 2);
        assert_eq!(out.checks[0].name, "Rust toolchain");
        assert_eq!(out.checks[0].status, "ok");
    }

    #[test]
    fn doctor_json_ok_is_false_when_any_check_fails() {
        let checks = vec![
            Check::ok("Rust toolchain", "rustc 1.98.0"),
            Check::fail("ekos.toml", "not found — run `ekos init`"),
        ];
        let out = build_doctor_json(&checks);
        assert!(!out.ok, "one failing check makes the whole report not-ok");
        assert_eq!(out.checks[1].status, "fail");
    }

    #[test]
    fn doctor_json_serializes_to_the_documented_keys() {
        let out = build_doctor_json(&[Check::fail("ekos.toml", "missing")]);
        let v: serde_json::Value = serde_json::to_value(&out).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["ok"], false);
        assert_eq!(v["checks"][0]["name"], "ekos.toml");
        assert_eq!(v["checks"][0]["status"], "fail");
        assert_eq!(v["checks"][0]["detail"], "missing");
    }

    #[test]
    fn collect_checks_flags_a_missing_config_and_ekos_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let config = EkosConfig::default();
        let missing_cfg = tmp.path().join("ekos.toml");
        let checks = collect_checks(&config, tmp.path(), &missing_cfg);
        let by_name = |n: &str| checks.iter().find(|c| c.label == n).unwrap();
        assert!(!by_name("ekos.toml").ok);
        assert!(!by_name(".ekos/").ok);
        // The Rust-toolchain check is environment-driven, not asserted here.
    }
}
