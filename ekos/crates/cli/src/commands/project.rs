//! RFC 0178 — `ekos project add|list|show|remove`: the projects in the EKOS home.
//!
//! A project is a directory `<home>/data/<tenant>/<project>/` holding an `ekos.toml` whose
//! `[workspace] root` names the sources, and a `state/` tree. Nothing here writes into a source
//! tree, and `remove` never deletes without `--purge --yes`.

use crate::detect::{detect_workspace, render_config};
use crate::home::{self, tenant_hint};
use anyhow::{Context, Result, bail};
use ekos_compiler_core::{EkosConfig, ProjectLocation};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// One project, as `project list --json` / `project show --json` print it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ProjectInfo {
    pub tenant: String,
    pub project: String,
    /// `[workspace] root`; `None` when the config cannot be read.
    pub source_root: Option<PathBuf>,
    pub config: PathBuf,
    pub state_dir: PathBuf,
}

fn info(location: &ProjectLocation) -> ProjectInfo {
    let source_root = EkosConfig::from_file(&location.config_path())
        .ok()
        .map(|c| c.workspace.root);
    ProjectInfo {
        tenant: location.tenant.clone(),
        project: location.project.clone(),
        source_root,
        config: location.config_path(),
        state_dir: location.state_dir(),
    }
}

/// Create a project for the sources in `source`. With `detect`, the config is the RFC 0152 scan
/// of that directory; otherwise the default one. Refuses an existing project.
pub fn add(location: &ProjectLocation, source: &Path, detect: bool) -> Result<()> {
    let source = source
        .canonicalize()
        .with_context(|| format!("--source {}", source.display()))?;
    if !source.is_dir() {
        bail!("--source {} is not a directory", source.display());
    }
    if location.dir().exists() {
        bail!(
            "project {:?} already exists in tenant {:?} ({})",
            location.project,
            location.tenant,
            location.dir().display()
        );
    }

    let rendered = if detect {
        let detection = detect_workspace(&source, &EkosConfig::default())?;
        super::init::print_detection(&detection);
        render_config(&detection)
    } else {
        super::init::DEFAULT_CONFIG.to_string()
    };
    let contents = with_source_root(&rendered, &source)?;

    std::fs::create_dir_all(location.dir())
        .with_context(|| format!("creating {}", location.dir().display()))?;
    std::fs::write(location.config_path(), contents)?;
    let (config, root) = home::load_project(location)?;
    for dir in [
        config.ekos_dir(&root),
        config.artifact_dir(&root),
        config.ledger_dir(&root),
        config.ekos_dir(&root).join("config"),
    ] {
        std::fs::create_dir_all(&dir)?;
    }

    println!(
        "Created project {} (tenant {})",
        location.project, location.tenant
    );
    println!("  sources  {}", root.display());
    println!("  config   {}", location.config_path().display());
    println!("  state    {}", location.state_dir().display());
    let select = format!(
        "--project {}{}",
        location.project,
        tenant_hint(&location.tenant)
    );
    println!();
    println!(
        "Next: ekos {select} build && ekos {select} recover && ekos {select} resolve \\\n      \
         && ekos {select} compile && ekos {select} commit"
    );
    Ok(())
}

/// The rendered config with `[workspace] root` set to the absolute source directory. Both config
/// templates state `root = "."`; a template that stopped doing so is caught here, not at the
/// first command run against the project.
fn with_source_root(rendered: &str, source: &Path) -> Result<String> {
    const PLACEHOLDER: &str = "root = \".\"";
    if rendered.matches(PLACEHOLDER).count() != 1 {
        bail!("internal: the config template does not state `[workspace] root` exactly once");
    }
    let Some(source) = source.to_str() else {
        bail!("--source {} is not valid UTF-8", source.display());
    };
    let value = toml::Value::String(source.to_string());
    Ok(rendered.replacen(PLACEHOLDER, &format!("root = {value}"), 1))
}

pub fn list(home_dir: &Path, json: bool) -> Result<()> {
    let projects: Vec<ProjectInfo> = home::list_projects(home_dir)?.iter().map(info).collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&projects)?);
        return Ok(());
    }
    if projects.is_empty() {
        println!("No projects in {}.", home_dir.display());
        println!("Create one with: ekos project add <name> --source <dir>");
        return Ok(());
    }
    for p in &projects {
        println!("{}/{}  {}", p.tenant, p.project, source_text(p));
    }
    Ok(())
}

fn source_text(p: &ProjectInfo) -> String {
    match &p.source_root {
        Some(root) => root.display().to_string(),
        None => "(config cannot be read)".to_string(),
    }
}

pub fn show(location: &ProjectLocation, json: bool) -> Result<()> {
    require_project(location)?;
    let p = info(location);
    if json {
        println!("{}", serde_json::to_string_pretty(&p)?);
        return Ok(());
    }
    println!("project  {}", p.project);
    println!("tenant   {}", p.tenant);
    println!("sources  {}", source_text(&p));
    println!("config   {}", p.config.display());
    println!("state    {}", p.state_dir.display());
    Ok(())
}

fn require_project(location: &ProjectLocation) -> Result<()> {
    if !location.config_path().is_file() {
        bail!(
            "no project {:?} in tenant {:?}",
            location.project,
            location.tenant
        );
    }
    Ok(())
}

/// Remove a project: moved to `<home>/trash/` by default, deleted with `purge` + `yes`. Returns
/// where the project went, `None` when it was deleted. The source tree is not touched.
pub fn remove(
    location: &ProjectLocation,
    purge: bool,
    yes: bool,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Option<PathBuf>> {
    require_project(location)?;
    if purge {
        if !yes {
            bail!(
                "--purge deletes the ledger and every compiled artifact of {}/{}; \
                 add --yes to confirm",
                location.tenant,
                location.project
            );
        }
        std::fs::remove_dir_all(location.dir())
            .with_context(|| format!("deleting {}", location.dir().display()))?;
        println!("Deleted project {}/{}", location.tenant, location.project);
        return Ok(None);
    }
    let trash = location.home.join("trash").join(&location.tenant);
    std::fs::create_dir_all(&trash)?;
    let dest = trash.join(format!(
        "{}-{}",
        location.project,
        now.format("%Y%m%dT%H%M%SZ")
    ));
    if dest.exists() {
        bail!("{} already exists; try again", dest.display());
    }
    std::fs::rename(location.dir(), &dest)
        .with_context(|| format!("moving {} to {}", location.dir().display(), dest.display()))?;
    println!(
        "Removed project {}/{}. Its data is kept in {}",
        location.tenant,
        location.project,
        dest.display()
    );
    Ok(Some(dest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home_dir = tmp.path().join("home");
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("schema.sql"), "create table t (id bigint);").unwrap();
        (tmp, home_dir, src.canonicalize().unwrap())
    }

    fn files_under(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push(p);
                }
            }
        }
        out.sort();
        out
    }

    #[test]
    fn add_writes_config_and_state_under_the_home_and_nothing_into_the_sources() {
        let (_tmp, home_dir, src) = fixture();
        let before = files_under(&src);
        let loc = home::location(&home_dir, "default", "shop").unwrap();
        add(&loc, &src, false).unwrap();

        let (config, root) = home::load_project(&loc).unwrap();
        assert_eq!(root, src);
        assert_eq!(config.ekos_dir(&root), loc.state_dir());
        assert!(loc.state_dir().join("artifacts").is_dir());
        assert!(loc.state_dir().join("ledger").is_dir());
        assert_eq!(files_under(&src), before, "the source tree was written to");

        // A second add never overwrites.
        assert!(add(&loc, &src, false).is_err());
    }

    #[test]
    fn add_with_detect_scans_the_sources_and_still_names_them_absolutely() {
        let (_tmp, home_dir, src) = fixture();
        let loc = home::location(&home_dir, "acme", "shop").unwrap();
        add(&loc, &src, true).unwrap();
        let text = std::fs::read_to_string(loc.config_path()).unwrap();
        assert!(text.contains("Detected inputs"), "{text}");
        let (_, root) = home::load_project(&loc).unwrap();
        assert_eq!(root, src);
        assert!(!src.join("ekos.toml").exists() && !src.join(".ekos").exists());
    }

    #[test]
    fn add_refuses_a_source_that_is_not_a_directory() {
        let (_tmp, home_dir, src) = fixture();
        let loc = home::location(&home_dir, "default", "shop").unwrap();
        assert!(add(&loc, &src.join("schema.sql"), false).is_err());
        assert!(add(&loc, &src.join("missing"), false).is_err());
        assert!(!loc.dir().exists(), "a failed add left a project behind");
    }

    #[test]
    fn a_source_path_with_quotes_survives_the_config() {
        let rendered = with_source_root(
            super::super::init::DEFAULT_CONFIG,
            Path::new("/srv/it's \"here\"/a\\b"),
        )
        .unwrap();
        let config: EkosConfig = toml::from_str(&rendered).unwrap();
        assert_eq!(
            config.workspace.root,
            PathBuf::from("/srv/it's \"here\"/a\\b")
        );
    }

    #[test]
    fn remove_keeps_the_data_unless_purged_and_purge_needs_yes() {
        let (_tmp, home_dir, src) = fixture();
        let loc = home::location(&home_dir, "default", "shop").unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-13T10:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        // Not a project: nothing is moved or deleted.
        assert!(remove(&loc, false, false, now).is_err());
        assert!(remove(&loc, true, true, now).is_err());

        add(&loc, &src, false).unwrap();
        std::fs::write(loc.state_dir().join("ledger/marker"), "x").unwrap();
        let dest = remove(&loc, false, false, now).unwrap().unwrap();
        assert_eq!(dest, home_dir.join("trash/default/shop-20261013T100000Z"));
        assert!(dest.join("state/ledger/marker").is_file());
        assert!(!loc.dir().exists());
        assert!(home::list_projects(&home_dir).unwrap().is_empty());

        add(&loc, &src, false).unwrap();
        assert!(
            remove(&loc, true, false, now).is_err(),
            "purge without --yes"
        );
        assert!(loc.dir().exists());
        assert_eq!(remove(&loc, true, true, now).unwrap(), None);
        assert!(!loc.dir().exists());
        assert!(src.join("schema.sql").is_file(), "sources were touched");
    }
}
