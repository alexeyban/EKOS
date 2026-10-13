//! RFC 0178 — the EKOS home and the projects in it.
//!
//! ```text
//! $EKOS_HOME/                         default ~/.ekos
//!   data/<tenant>/<project>/
//!       ekos.toml                     [workspace] root = "/abs/source/dir"
//!       state/                        what `.ekos/` is for an in-tree workspace
//!   trash/<tenant>/<project>-<time>/  a removed project, until purged
//! ```
//!
//! A project exists exactly when its `ekos.toml` exists; there is no registry file. Everything
//! here is path arithmetic and validation. Reading a project's config is [`load_project`].

use anyhow::{Context, Result, bail};
use ekos_compiler_core::{EkosConfig, ProjectLocation};
use std::path::{Path, PathBuf};

/// The tenant a project belongs to when none is named.
pub const DEFAULT_TENANT: &str = "default";

const MAX_NAME_LEN: usize = 63;

/// A tenant or project name: `[a-z0-9][a-z0-9_-]{0,62}`. Checked before any path is built from
/// it, so a name can never leave the home (`..`, `/`, an absolute path).
pub fn validate_name(what: &str, name: &str) -> Result<()> {
    let mut chars = name.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let rest_ok =
        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if !first_ok || !rest_ok || name.len() > MAX_NAME_LEN {
        bail!(
            "invalid {what} name {name:?}: use lowercase letters, digits, `-` and `_`, starting \
             with a letter or digit, at most {MAX_NAME_LEN} characters"
        );
    }
    Ok(())
}

/// The home directory: `--home`, then `EKOS_HOME`, then `<user home>/.ekos`.
pub fn resolve_home(
    flag: Option<PathBuf>,
    env_home: Option<PathBuf>,
    user_home: Option<PathBuf>,
) -> Result<PathBuf> {
    let home = flag
        .or(env_home)
        .or_else(|| user_home.map(|h| h.join(".ekos")))
        .context("cannot locate the EKOS home: pass --home or set EKOS_HOME")?;
    if home.is_absolute() {
        return Ok(home);
    }
    Ok(std::env::current_dir()?.join(home))
}

/// The user's home directory from the environment (`HOME`, or `USERPROFILE` on Windows).
pub fn user_home_from_env() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Which project a command selected, if any. A flag wins over its environment variable.
/// `--tenant` without a project selects nothing and is refused.
pub fn select_project(
    home: &Path,
    project: Option<String>,
    env_project: Option<String>,
    tenant: Option<String>,
    env_tenant: Option<String>,
) -> Result<Option<ProjectLocation>> {
    let tenant_flag = tenant.is_some();
    let Some(project) = project.or(env_project).filter(|p| !p.is_empty()) else {
        if tenant_flag {
            bail!("--tenant names a tenant but no project: add --project <name>");
        }
        return Ok(None);
    };
    let tenant = tenant
        .or(env_tenant)
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| DEFAULT_TENANT.to_string());
    Ok(Some(location(home, &tenant, &project)?))
}

/// The location of `<tenant>/<project>` under `home`, both names validated.
pub fn location(home: &Path, tenant: &str, project: &str) -> Result<ProjectLocation> {
    validate_name("tenant", tenant)?;
    validate_name("project", project)?;
    Ok(ProjectLocation {
        home: home.to_path_buf(),
        tenant: tenant.to_string(),
        project: project.to_string(),
    })
}

/// Load a project's config and its source root. Unlike an in-tree workspace, a missing or broken
/// config is an error: falling back to defaults would compile nothing and report success.
pub fn load_project(location: &ProjectLocation) -> Result<(EkosConfig, PathBuf)> {
    let config_path = location.config_path();
    if !config_path.is_file() {
        bail!(
            "no project {:?} in tenant {:?} ({} not found). Create it with:\n  \
             ekos project add {} --source <dir>{}",
            location.project,
            location.tenant,
            config_path.display(),
            location.project,
            tenant_hint(&location.tenant),
        );
    }
    let mut config = EkosConfig::from_file(&config_path)
        .with_context(|| format!("project config {}", config_path.display()))?;
    let root = config.workspace.root.clone();
    if !root.is_absolute() {
        bail!(
            "{}: [workspace] root must be an absolute path to the project's sources, found {:?}",
            config_path.display(),
            root
        );
    }
    if !root.is_dir() {
        bail!(
            "{}: [workspace] root {} is not a directory",
            config_path.display(),
            root.display()
        );
    }
    config.location = Some(location.clone());
    Ok((config, root))
}

/// ` --tenant <t>` for a command hint, empty for the default tenant.
pub fn tenant_hint(tenant: &str) -> String {
    if tenant == DEFAULT_TENANT {
        String::new()
    } else {
        format!(" --tenant {tenant}")
    }
}

/// Every project in the home, sorted by tenant then name. A directory without `ekos.toml`, or
/// with a name that is not a valid one, is not a project.
pub fn list_projects(home: &Path) -> Result<Vec<ProjectLocation>> {
    let mut out = Vec::new();
    for tenant in subdirs(&home.join("data"))? {
        if validate_name("tenant", &tenant).is_err() {
            continue;
        }
        for project in subdirs(&home.join("data").join(&tenant))? {
            let Ok(loc) = location(home, &tenant, &project) else {
                continue;
            };
            if loc.config_path().is_file() {
                out.push(loc);
            }
        }
    }
    out.sort_by(|a, b| (&a.tenant, &a.project).cmp(&(&b.tenant, &b.project)));
    Ok(out)
}

fn subdirs(dir: &Path) -> Result<Vec<String>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Option<PathBuf> {
        Some(PathBuf::from(s))
    }

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    #[test]
    fn names_that_could_leave_the_home_are_refused() {
        for good in ["shop", "a", "0", "payments-api", "team_1", &"a".repeat(63)] {
            validate_name("project", good).unwrap();
        }
        for bad in [
            "",
            "..",
            ".",
            "a/b",
            "/abs",
            "a\\b",
            "Shop",
            "-lead",
            "_lead",
            "a b",
            "a.b",
            "é",
            &"a".repeat(64),
        ] {
            assert!(validate_name("project", bad).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn home_resolution_precedence() {
        assert_eq!(
            resolve_home(p("/flag"), p("/env"), p("/u")).unwrap(),
            PathBuf::from("/flag")
        );
        assert_eq!(
            resolve_home(None, p("/env"), p("/u")).unwrap(),
            PathBuf::from("/env")
        );
        assert_eq!(
            resolve_home(None, None, p("/u")).unwrap(),
            PathBuf::from("/u/.ekos")
        );
        assert!(resolve_home(None, None, None).is_err());
        // A relative home is anchored once, so a later change of directory cannot move it.
        assert!(resolve_home(p("rel"), None, None).unwrap().is_absolute());
    }

    #[test]
    fn project_selection() {
        let home = Path::new("/h");
        assert_eq!(select_project(home, None, None, None, None).unwrap(), None);
        // A tenant in the environment alone selects nothing; the flag alone is a mistake.
        assert_eq!(
            select_project(home, None, None, None, s("acme")).unwrap(),
            None
        );
        assert!(select_project(home, None, None, s("acme"), None).is_err());

        let loc = select_project(home, s("shop"), s("other"), None, None)
            .unwrap()
            .unwrap();
        assert_eq!(
            (loc.tenant.as_str(), loc.project.as_str()),
            ("default", "shop")
        );
        assert_eq!(loc.dir(), PathBuf::from("/h/data/default/shop"));
        assert_eq!(
            loc.config_path(),
            PathBuf::from("/h/data/default/shop/ekos.toml")
        );
        assert_eq!(loc.state_dir(), PathBuf::from("/h/data/default/shop/state"));

        let loc = select_project(home, None, s("shop"), s("acme"), s("ignored"))
            .unwrap()
            .unwrap();
        assert_eq!(
            (loc.tenant.as_str(), loc.project.as_str()),
            ("acme", "shop")
        );

        assert!(select_project(home, s("../x"), None, None, None).is_err());
        assert!(select_project(home, s("shop"), None, s("a/b"), None).is_err());
    }

    #[test]
    fn a_project_config_must_exist_parse_and_name_an_absolute_source_root() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let loc = location(&tmp.path().join("home"), "default", "shop").unwrap();

        let err = load_project(&loc).unwrap_err().to_string();
        assert!(err.contains("ekos project add shop"), "{err}");

        std::fs::create_dir_all(loc.dir()).unwrap();
        std::fs::write(loc.config_path(), "[workspace]\nroot = \".\"\n").unwrap();
        let err = load_project(&loc).unwrap_err().to_string();
        assert!(err.contains("absolute"), "{err}");

        std::fs::write(loc.config_path(), "[workspace\n").unwrap();
        assert!(
            load_project(&loc).is_err(),
            "a broken config is not a default"
        );

        let missing = tmp.path().join("gone");
        std::fs::write(
            loc.config_path(),
            format!("[workspace]\nroot = {:?}\n", missing.display().to_string()),
        )
        .unwrap();
        let err = load_project(&loc).unwrap_err().to_string();
        assert!(err.contains("not a directory"), "{err}");

        std::fs::write(
            loc.config_path(),
            format!("[workspace]\nroot = {:?}\n", src.display().to_string()),
        )
        .unwrap();
        let (config, root) = load_project(&loc).unwrap();
        assert_eq!(root, src);
        assert_eq!(config.ekos_dir(&root), loc.state_dir());
    }

    #[test]
    fn listing_finds_projects_and_skips_everything_else() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        assert!(list_projects(home).unwrap().is_empty());
        for (t, pr) in [("default", "shop"), ("acme", "shop"), ("acme", "billing")] {
            let loc = location(home, t, pr).unwrap();
            std::fs::create_dir_all(loc.dir()).unwrap();
            std::fs::write(loc.config_path(), "").unwrap();
        }
        // Not projects: no config, an invalid name, a stray file.
        std::fs::create_dir_all(home.join("data/acme/empty")).unwrap();
        std::fs::create_dir_all(home.join("data/acme/Bad Name")).unwrap();
        std::fs::write(home.join("data/acme/Bad Name/ekos.toml"), "").unwrap();
        std::fs::write(home.join("data/notes.txt"), "").unwrap();

        let found: Vec<_> = list_projects(home)
            .unwrap()
            .into_iter()
            .map(|l| format!("{}/{}", l.tenant, l.project))
            .collect();
        assert_eq!(found, ["acme/billing", "acme/shop", "default/shop"]);
    }
}
