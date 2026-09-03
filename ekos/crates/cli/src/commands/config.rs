//! `ekos config validate` / `ekos config preview-scan` (RFC 0130).
//!
//! Both are read-only. `validate` parses `ekos.toml` and flags `[observe]` mistakes that are easy
//! to make and hard to notice; `preview-scan` counts what `ekos build` would observe without
//! reading or compiling anything.

use anyhow::Result;
use ekos_compiler_core::{EkosConfig, config::ObserveConfig};
use ekos_observation_sdk::{ScanContext, walk_observed};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

// ── R7 — validate ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ValidateReport {
    pub schema_version: u32,
    pub ok: bool,
    pub errors: Vec<Finding>,
    pub warnings: Vec<Finding>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Finding {
    pub code: &'static str,
    pub detail: String,
}

/// `[observe]`-focused lint over an already-parsed config. Pure — unit-tested directly.
pub fn observe_warnings(observe: &ObserveConfig, root: &Path) -> Vec<Finding> {
    let mut out = Vec::new();

    if observe.paths.is_empty() {
        out.push(Finding {
            code: "observe-empty",
            detail: "[observe] paths is empty — the whole workspace root is scanned".into(),
        });
    }

    for p in &observe.paths {
        if !root.join(p).exists() {
            out.push(Finding {
                code: "observe-path-missing",
                detail: format!("'{}' does not exist under the workspace root", p.display()),
            });
        }
    }

    for pat in &observe.ignore_patterns {
        if looks_like_a_path_or_glob(pat) {
            out.push(Finding {
                code: "ignore-pattern-looks-like-a-path",
                detail: format!(
                    "'{pat}' looks like a path or glob, but ignore-patterns match a directory \
                     NAME exactly — so this matches nothing. Use just the directory name."
                ),
            });
        }
    }

    out
}

fn looks_like_a_path_or_glob(pat: &str) -> bool {
    pat.contains('/')
        || pat.contains('\\')
        || pat.contains('*')
        || pat.contains('?')
        || pat.contains('[')
        // a leading-dot + extension shape like ".log" or "*.tmp" (the '*' case is already caught)
        || (pat.starts_with('.') && pat[1..].contains('.'))
        || (!pat.starts_with('.') && pat.rsplit_once('.').is_some_and(|(stem, ext)| {
            !stem.is_empty() && !ext.is_empty() && ext.len() <= 4
        }))
}

pub fn build_validate_report(config_path: &Path, root: &Path) -> ValidateReport {
    match EkosConfig::from_file(config_path) {
        Ok(cfg) => {
            let warnings = observe_warnings(&cfg.observe, root);
            ValidateReport {
                schema_version: 1,
                ok: true,
                errors: Vec::new(),
                warnings,
            }
        }
        Err(e) => ValidateReport {
            schema_version: 1,
            ok: false,
            errors: vec![Finding {
                code: "parse-error",
                detail: e.to_string(),
            }],
            warnings: Vec::new(),
        },
    }
}

pub fn validate(config: &EkosConfig, cwd: &Path, config_path: &Path, json: bool) -> Result<()> {
    let _ = config;
    let report = build_validate_report(config_path, cwd);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    if report.ok {
        println!("ekos.toml is valid.");
    } else {
        for e in &report.errors {
            println!("  error: {}", e.detail);
        }
    }
    for w in &report.warnings {
        println!("  warning [{}]: {}", w.code, w.detail);
    }
    if !report.ok {
        anyhow::bail!("ekos.toml did not validate");
    }
    Ok(())
}

// ── R8 — preview-scan ────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PreviewScan {
    pub schema_version: u32,
    pub roots: Vec<String>,
    pub total_files: usize,
    pub total_bytes: u64,
    pub truncated: bool,
    pub by_extension: Vec<ExtCount>,
    pub ignored_dir_hits: Vec<IgnoreHit>,
    pub elapsed_ms: u128,
}

#[derive(Debug, Serialize)]
pub struct ExtCount {
    pub ext: String,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct IgnoreHit {
    pub pattern: String,
    pub dirs_skipped: usize,
}

pub fn build_preview_scan(config: &EkosConfig, cwd: &Path, max_files: usize) -> PreviewScan {
    let started = Instant::now();

    let roots: Vec<std::path::PathBuf> = if config.observe.paths.is_empty() {
        vec![cwd.to_path_buf()]
    } else {
        config.observe.paths.iter().map(|p| cwd.join(p)).collect()
    };

    let mut total_files = 0usize;
    let mut total_bytes = 0u64;
    let mut truncated = false;
    let mut by_ext: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    let mut pruned: BTreeMap<String, usize> = config
        .observe
        .ignore_patterns
        .iter()
        .map(|p| (p.clone(), 0usize))
        .collect();

    for root in &roots {
        let ctx =
            ScanContext::new(root).with_ignore_patterns(config.observe.ignore_patterns.clone());
        walk_observed(
            &ctx,
            |rel, meta| {
                if truncated {
                    return;
                }
                total_files += 1;
                total_bytes += meta.len();
                let ext = rel
                    .rsplit_once('/')
                    .map_or(rel, |(_, f)| f)
                    .rsplit_once('.')
                    .map_or("", |(_, e)| e)
                    .to_string();
                let slot = by_ext.entry(ext).or_insert((0, 0));
                slot.0 += 1;
                slot.1 += meta.len();
                if total_files >= max_files {
                    truncated = true;
                }
            },
            |name| {
                if let Some(n) = pruned.get_mut(name) {
                    *n += 1;
                }
            },
        );
    }

    let mut by_extension: Vec<ExtCount> = by_ext
        .into_iter()
        .map(|(ext, (files, bytes))| ExtCount { ext, files, bytes })
        .collect();
    by_extension.sort_by(|a, b| b.files.cmp(&a.files).then(a.ext.cmp(&b.ext)));

    let ignored_dir_hits = pruned
        .into_iter()
        .map(|(pattern, dirs_skipped)| IgnoreHit {
            pattern,
            dirs_skipped,
        })
        .collect();

    PreviewScan {
        schema_version: 1,
        roots: roots.iter().map(|r| r.display().to_string()).collect(),
        total_files,
        total_bytes,
        truncated,
        by_extension,
        ignored_dir_hits,
        elapsed_ms: started.elapsed().as_millis(),
    }
}

pub fn preview_scan(config: &EkosConfig, cwd: &Path, max_files: usize, json: bool) -> Result<()> {
    let report = build_preview_scan(config, cwd, max_files);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    println!(
        "{} file(s), {} bytes across {} root(s){}",
        report.total_files,
        report.total_bytes,
        report.roots.len(),
        if report.truncated { " (truncated)" } else { "" }
    );
    for hit in &report.ignored_dir_hits {
        if hit.dirs_skipped == 0 {
            println!(
                "  note: ignore-pattern '{}' matched no directories",
                hit.pattern
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn observe(paths: &[&str], ignore: &[&str]) -> ObserveConfig {
        ObserveConfig {
            paths: paths.iter().map(PathBuf::from).collect(),
            ignore_patterns: ignore.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn glob_and_path_shaped_ignore_patterns_are_flagged() {
        let tmp = tempfile::tempdir().unwrap();
        let w = observe(
            &[],
            &[
                "target",
                "src/fixtures",
                "*.tmp",
                "build.rs",
                "node_modules",
            ],
        );
        let flagged = observe_warnings(&w, tmp.path())
            .iter()
            .filter(|f| f.code == "ignore-pattern-looks-like-a-path")
            .count();
        // src/fixtures (has /), *.tmp (has *), build.rs (stem.ext) → 3
        assert_eq!(flagged, 3);

        // plain directory names — including dotfiles — are not flagged
        let clean = observe(
            &[],
            &["target", "node_modules", ".git", ".venv", "fixtures"],
        );
        assert!(
            !observe_warnings(&clean, tmp.path())
                .iter()
                .any(|f| f.code == "ignore-pattern-looks-like-a-path")
        );
    }

    #[test]
    fn a_missing_observe_path_is_a_warning() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("crates")).unwrap();
        let w = observe(&["crates", "does-not-exist"], &["target"]);
        let findings = observe_warnings(&w, tmp.path());
        assert!(
            findings
                .iter()
                .any(|f| f.code == "observe-path-missing" && f.detail.contains("does-not-exist"))
        );
        assert!(!findings.iter().any(|f| f.detail.contains("'crates'")));
    }

    #[test]
    fn empty_observe_paths_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let findings = observe_warnings(&observe(&[], &["target"]), tmp.path());
        assert!(findings.iter().any(|f| f.code == "observe-empty"));
    }

    #[test]
    fn validate_report_surfaces_a_parse_error_as_not_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = tmp.path().join("ekos.toml");
        // An unknown top-level section — `EkosConfig` is `deny_unknown_fields`.
        std::fs::write(&cfg, "[not-a-real-section]\nx = 1\n").unwrap();
        let report = build_validate_report(&cfg, tmp.path());
        assert!(!report.ok);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].code, "parse-error");

        // A clean minimal config validates.
        std::fs::write(&cfg, "[observe]\npaths = []\n").unwrap();
        assert!(build_validate_report(&cfg, tmp.path()).ok);
    }

    #[test]
    fn preview_scan_counts_files_by_extension_and_reports_pruned_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("a.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("b.rs"), "// b").unwrap();
        std::fs::write(root.join("c.md"), "# c").unwrap();
        std::fs::create_dir(root.join("target")).unwrap();
        std::fs::write(root.join("target").join("junk.rs"), "generated").unwrap();
        std::fs::create_dir(root.join("keep")).unwrap();
        std::fs::write(root.join("keep").join("d.rs"), "// d").unwrap();

        let mut config = EkosConfig::default();
        config.observe.ignore_patterns = vec!["target".into(), "glob*".into()];

        let scan = build_preview_scan(&config, root, 200_000);
        assert_eq!(scan.total_files, 4); // a.rs b.rs c.md keep/d.rs — target/junk.rs pruned
        let rs = scan.by_extension.iter().find(|e| e.ext == "rs").unwrap();
        assert_eq!(rs.files, 3);

        let target_hit = scan
            .ignored_dir_hits
            .iter()
            .find(|h| h.pattern == "target")
            .unwrap();
        assert_eq!(target_hit.dirs_skipped, 1);
        let glob_hit = scan
            .ignored_dir_hits
            .iter()
            .find(|h| h.pattern == "glob*")
            .unwrap();
        assert_eq!(glob_hit.dirs_skipped, 0, "a glob pattern prunes nothing");
    }

    #[test]
    fn preview_scan_truncates_at_max_files() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..10 {
            std::fs::write(tmp.path().join(format!("f{i}.txt")), "x").unwrap();
        }
        let config = EkosConfig::default();
        let scan = build_preview_scan(&config, tmp.path(), 3);
        assert!(scan.truncated);
        assert_eq!(scan.total_files, 3);
    }
}
