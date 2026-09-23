use crate::detect::{Detection, detect_workspace, render_config};
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use std::path::Path;

/// RFC 0152 — how `ekos init` should write `ekos.toml`.
#[derive(Debug, Clone, Copy, Default)]
pub struct InitOptions {
    /// Scan the workspace and write a config that reflects what is actually in it.
    pub detect: bool,
    /// Print the config that would be written, write nothing.
    pub dry_run: bool,
    /// Overwrite an existing `ekos.toml`.
    pub force: bool,
}

pub fn run(config: &EkosConfig, cwd: &Path) -> Result<()> {
    run_with_options(config, cwd, InitOptions::default())
}

pub fn run_with_options(config: &EkosConfig, cwd: &Path, opts: InitOptions) -> Result<()> {
    let ekos_dir = config.ekos_dir(cwd);
    let artifact_dir = config.artifact_dir(cwd);
    let ledger_dir = ekos_dir.join("ledger");
    let config_dir = ekos_dir.join("config");

    // `--dry-run` must not leave a half-initialized workspace behind: it is a preview of the
    // config, so it creates nothing at all.
    if !opts.dry_run {
        for dir in [&ekos_dir, &artifact_dir, &ledger_dir, &config_dir] {
            std::fs::create_dir_all(dir)?;
            tracing::debug!("created {}", dir.display());
        }
    }

    let detection = if opts.detect {
        let d = detect_workspace(cwd, config)?;
        print_detection(&d);
        Some(d)
    } else {
        None
    };

    let contents = match &detection {
        Some(d) => render_config(d),
        None => DEFAULT_CONFIG.to_string(),
    };

    let ekos_toml = cwd.join("ekos.toml");

    if opts.dry_run {
        println!("--- ekos.toml (dry run, nothing written) ---");
        print!("{contents}");
        return Ok(());
    }

    if ekos_toml.exists() && !opts.force {
        // Never silently overwrite a config a human has edited: this repository's own
        // `ignore-patterns` list is ~20 hand-written entries, each added after a real
        // contamination incident, and regenerating over it would destroy that work.
        println!("ekos.toml already exists — skipping (pass --force to overwrite)");
    } else {
        std::fs::write(&ekos_toml, &contents)?;
        if ekos_toml.exists() && opts.force {
            println!("Wrote ekos.toml (overwrote the existing file)");
        } else {
            println!("Created ekos.toml");
        }
    }

    println!("Initialized .ekos/ workspace at {}", ekos_dir.display());
    println!("  artifacts: {}", artifact_dir.display());
    println!("  ledger:    {}", ledger_dir.display());

    if detection.is_some() {
        println!();
        println!("Next: ekos build && ekos recover && ekos resolve && ekos compile && ekos commit");
        println!("Then: ekos coverage   — confirms every input kind actually produced objects.");
    }
    Ok(())
}

fn print_detection(d: &Detection) {
    println!("Detected workspace contents");
    println!("{}", "─".repeat(60));

    if d.sources.is_empty() {
        println!("  (nothing EKOS recovers from was found here)");
    }
    for s in &d.sources {
        let samples = if s.sample_paths.is_empty() {
            String::new()
        } else {
            format!("  e.g. {}", s.sample_paths.join(", "))
        };
        println!(
            "  {:<22} {:>6} file(s){samples}",
            s.kind.label(),
            s.file_count
        );
    }

    if let Some(g) = &d.sql_dialect {
        let markers = g
            .markers
            .iter()
            .map(|m| format!("{} ({})", m.marker, m.hits))
            .collect::<Vec<_>>()
            .join(", ");
        println!();
        println!(
            "  SQL dialect: {} — from {} sampled file(s): {markers}",
            g.dialect, g.sampled_files
        );
    }

    if !d.contaminants.is_empty() {
        println!();
        println!("  Excluded (not this project's own knowledge):");
        for c in &d.contaminants {
            println!(
                "    {:<18} {} file(s)",
                c.pattern,
                crate::detect::render_count(c)
            );
        }
    }

    if !d.suggestions.is_empty() {
        println!();
        println!("  Not enabled automatically:");
        for s in &d.suggestions {
            println!("    {}", s.headline);
        }
    }
    println!("{}", "─".repeat(60));
}

const DEFAULT_CONFIG: &str = r#"[workspace]
root = "."
log-level = "info"
log-format = "pretty"

[observe]
paths = ["."]
ignore-patterns = [".ekos", ".git", "target", "node_modules"]
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_init_still_writes_the_inert_default_config() {
        let tmp = tempfile::tempdir().unwrap();
        run(&EkosConfig::default(), tmp.path()).unwrap();

        let written = std::fs::read_to_string(tmp.path().join("ekos.toml")).unwrap();
        assert_eq!(
            written, DEFAULT_CONFIG,
            "`ekos init` is unchanged by RFC 0152"
        );
        assert!(tmp.path().join(".ekos").is_dir());
    }

    #[test]
    fn detect_writes_a_config_naming_what_it_found() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("db")).unwrap();
        std::fs::write(
            tmp.path().join("db/schema.sql"),
            "CREATE TABLE t (id SERIAL PRIMARY KEY);\nCOMMENT ON TABLE t IS $$x$$;",
        )
        .unwrap();

        run_with_options(
            &EkosConfig::default(),
            tmp.path(),
            InitOptions {
                detect: true,
                ..Default::default()
            },
        )
        .unwrap();

        let written = std::fs::read_to_string(tmp.path().join("ekos.toml")).unwrap();
        assert!(
            written.contains("default-dialect = \"postgres\""),
            "the detected dialect must reach the file: {written}"
        );
        let parsed: EkosConfig = toml::from_str(&written).unwrap();
        assert_eq!(parsed.recover.sql.default_dialect, "postgres");
    }

    #[test]
    fn detect_excludes_a_venv_it_finds() {
        let tmp = tempfile::tempdir().unwrap();
        let venv = tmp.path().join(".venv/lib/python3.13/site-packages/numpy");
        std::fs::create_dir_all(&venv).unwrap();
        std::fs::write(venv.join("core.py"), "x = 1").unwrap();
        std::fs::write(tmp.path().join("app.py"), "y = 2").unwrap();

        run_with_options(
            &EkosConfig::default(),
            tmp.path(),
            InitOptions {
                detect: true,
                ..Default::default()
            },
        )
        .unwrap();

        let parsed: EkosConfig =
            toml::from_str(&std::fs::read_to_string(tmp.path().join("ekos.toml")).unwrap())
                .unwrap();
        assert!(parsed.observe.ignore_patterns.iter().any(|p| p == ".venv"));
    }

    /// The `.venv` incident in reverse: an inventory that counts third-party files as the
    /// project's own source is the same lie the ledger told before those patterns existed.
    #[test]
    fn contaminated_files_are_not_counted_as_project_source() {
        let tmp = tempfile::tempdir().unwrap();
        let venv = tmp.path().join(".venv/lib/site-packages/numpy");
        std::fs::create_dir_all(&venv).unwrap();
        for i in 0..5 {
            std::fs::write(venv.join(format!("m{i}.py")), "x = 1").unwrap();
        }
        std::fs::write(tmp.path().join("app.py"), "y = 2").unwrap();

        let detection = detect_workspace(tmp.path(), &EkosConfig::default()).unwrap();
        assert_eq!(
            detection.file_count(crate::detect::SourceKind::Python),
            1,
            "only the project's own app.py counts, not the five under .venv"
        );
    }

    #[test]
    fn an_existing_config_is_never_overwritten_without_force() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("ekos.toml"), "# hand-written\n").unwrap();
        std::fs::write(tmp.path().join("app.py"), "y = 2").unwrap();

        run_with_options(
            &EkosConfig::default(),
            tmp.path(),
            InitOptions {
                detect: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("ekos.toml")).unwrap(),
            "# hand-written\n"
        );

        run_with_options(
            &EkosConfig::default(),
            tmp.path(),
            InitOptions {
                detect: true,
                force: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            std::fs::read_to_string(tmp.path().join("ekos.toml"))
                .unwrap()
                .contains("generated by `ekos init --detect`")
        );
    }

    #[test]
    fn dry_run_writes_nothing_at_all() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("app.py"), "y = 2").unwrap();

        run_with_options(
            &EkosConfig::default(),
            tmp.path(),
            InitOptions {
                detect: true,
                dry_run: true,
                ..Default::default()
            },
        )
        .unwrap();

        assert!(!tmp.path().join("ekos.toml").exists());
        assert!(
            !tmp.path().join(".ekos").exists(),
            "a preview must not leave a half-initialized workspace behind"
        );
    }
}
