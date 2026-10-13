//! RFC 0178 Part 2 — `ekos source add|list|remove`: the named inputs of a project.
//!
//! A source is a `[[sources]]` entry in the project's `ekos.toml`. These commands edit that file
//! in place and keep its comments. They never touch a source directory or the ledger.

use anyhow::{Context, Result, bail};
use ekos_compiler_core::{EkosConfig, SourceConfig, SourceKind, is_valid_name};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// `--source` on `ekos project add`: `<dir>` (the project's one unnamed source) or
/// `<name>=<dir>` (a named source).
pub fn parse_spec(spec: &str) -> Result<(Option<String>, PathBuf)> {
    match spec.split_once('=') {
        Some((name, path)) if is_valid_name(name) && !path.is_empty() => {
            Ok((Some(name.to_string()), PathBuf::from(path)))
        }
        Some((name, _)) if !name.contains(['/', '\\']) => bail!(
            "--source {spec:?}: {name:?} is not a valid source name (lowercase letters, digits, \
             `-` and `_`, starting with a letter or digit)"
        ),
        _ => Ok((None, PathBuf::from(spec))),
    }
}

pub fn parse_kind(kind: &str) -> Result<SourceKind> {
    match kind {
        "dir" => Ok(SourceKind::Dir),
        "git" => Ok(SourceKind::Git),
        other => bail!("unknown source kind {other:?}: use `dir` or `git`"),
    }
}

/// `git` when the directory is a repository, `dir` otherwise.
pub fn detect_kind(dir: &Path) -> SourceKind {
    if dir.join(".git").exists() {
        SourceKind::Git
    } else {
        SourceKind::Dir
    }
}

/// `config_text` with one more `[[sources]]` entry. Everything else, comments included, is kept.
pub fn with_source(config_text: &str, source: &SourceConfig) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = config_text.parse().context("parsing ekos.toml")?;
    let Some(path) = source.path.to_str() else {
        bail!("source path {} is not valid UTF-8", source.path.display());
    };
    let mut table = toml_edit::Table::new();
    table["name"] = toml_edit::value(source.name.as_str());
    table["kind"] = toml_edit::value(source.kind.as_str());
    table["path"] = toml_edit::value(path);
    let entry = doc
        .entry("sources")
        .or_insert(toml_edit::Item::ArrayOfTables(Default::default()));
    let Some(array) = entry.as_array_of_tables_mut() else {
        bail!("ekos.toml: `sources` is not a list of [[sources]] tables");
    };
    array.push(table);
    checked(doc.to_string())
}

/// `config_text` without the `[[sources]]` entry called `name`.
pub fn without_source(config_text: &str, name: &str) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = config_text.parse().context("parsing ekos.toml")?;
    let array = doc
        .get_mut("sources")
        .and_then(toml_edit::Item::as_array_of_tables_mut);
    let position = array.as_ref().and_then(|a| {
        a.iter()
            .position(|t| t.get("name").and_then(toml_edit::Item::as_str) == Some(name))
    });
    let (Some(array), Some(position)) = (array, position) else {
        bail!("no source {name:?} in this project");
    };
    array.remove(position);
    if array.is_empty() {
        doc.remove("sources");
    }
    checked(doc.to_string())
}

/// The edited text must still be a config EKOS accepts; an edit that breaks it is not written.
fn checked(text: String) -> Result<String> {
    let config: EkosConfig = toml::from_str(&text).context("the edited ekos.toml is invalid")?;
    config.check_sources()?;
    Ok(text)
}

/// Add a named source to the config at `config_path`. `ledger_objects` is how many objects the
/// project's ledger holds.
///
/// A project compiled without source names cannot gain them afterwards: its objects' ids were
/// derived without a source key, the ledger is append-only, and naming the sources now would
/// leave every existing object beside a renamed copy of itself.
pub fn add(
    config_path: &Path,
    config: &EkosConfig,
    cwd: &Path,
    ledger_objects: usize,
    name: &str,
    path: &Path,
    kind: Option<SourceKind>,
) -> Result<()> {
    if !is_valid_name(name) {
        bail!(
            "invalid source name {name:?}: use lowercase letters, digits, `-` and `_`, starting \
             with a letter or digit, at most 63 characters"
        );
    }
    if config.sources.is_empty() && ledger_objects > 0 {
        bail!(
            "this project was compiled without source names ({ledger_objects} objects in its \
             ledger). Naming sources now would give every object a new id beside the old one. \
             Create a new project with named sources instead:\n  \
             ekos project add <name> --source <source>=<dir> [--source <source>=<dir> …]"
        );
    }
    let dir = cwd
        .join(path)
        .canonicalize()
        .with_context(|| format!("source path {}", path.display()))?;
    if !dir.is_dir() {
        bail!("{} is not a directory", dir.display());
    }
    let source = SourceConfig {
        name: name.to_string(),
        kind: kind.unwrap_or_else(|| detect_kind(&dir)),
        path: dir.clone(),
    };
    let text = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    std::fs::write(config_path, with_source(&text, &source)?)?;
    println!(
        "Added source {} ({}) → {}",
        source.name,
        source.kind.as_str(),
        dir.display()
    );
    if config.sources.is_empty() {
        println!(
            "This project now observes its named sources only; `[observe] paths` is no longer read."
        );
    }
    println!("Run the pipeline to compile it: ekos build --source {name}, then recover … commit.");
    Ok(())
}

pub fn remove(config_path: &Path, name: &str) -> Result<()> {
    let text = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    std::fs::write(config_path, without_source(&text, name)?)?;
    println!("Removed source {name} from {}", config_path.display());
    println!(
        "Objects already compiled from it stay in the ledger: the ledger is append-only. \
         Its directory was not touched."
    );
    Ok(())
}

/// One source as `ekos source list` prints it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SourceInfo {
    pub name: String,
    pub kind: &'static str,
    pub path: PathBuf,
    pub exists: bool,
    /// Whether the directory is a git repository (commit history can be observed).
    pub git: bool,
}

pub fn infos(config: &EkosConfig, cwd: &Path) -> Vec<SourceInfo> {
    config
        .sources
        .iter()
        .map(|s| {
            let path = cwd.join(&s.path);
            SourceInfo {
                name: s.name.clone(),
                kind: s.kind.as_str(),
                exists: path.is_dir(),
                git: path.join(".git").exists(),
                path,
            }
        })
        .collect()
}

pub fn list(config: &EkosConfig, cwd: &Path, json: bool) -> Result<()> {
    let sources = infos(config, cwd);
    if json {
        println!("{}", serde_json::to_string_pretty(&sources)?);
        return Ok(());
    }
    if sources.is_empty() {
        println!("No named sources: this project observes `[observe] paths` under its root.");
        return Ok(());
    }
    for s in &sources {
        let note = if !s.exists {
            "  (missing)"
        } else if s.kind == "git" && !s.git {
            "  (no .git: no commit history)"
        } else {
            ""
        };
        println!("{}  {}  {}{note}", s.name, s.kind, s.path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "# hand-written\n[workspace]\nroot = \"/srv/shop\"  # the sources\n\n\
                          [observe]\npaths = [\".\"]\n";

    fn source(name: &str, path: &str) -> SourceConfig {
        SourceConfig {
            name: name.into(),
            kind: SourceKind::Git,
            path: PathBuf::from(path),
        }
    }

    #[test]
    fn a_spec_is_a_directory_or_a_named_directory() {
        assert_eq!(
            parse_spec("/srv/a").unwrap(),
            (None, PathBuf::from("/srv/a"))
        );
        assert_eq!(
            parse_spec("api=/srv/a").unwrap(),
            (Some("api".into()), PathBuf::from("/srv/a"))
        );
        // A path that happens to contain `=` is still a path.
        assert_eq!(
            parse_spec("/srv/a=b/c").unwrap(),
            (None, PathBuf::from("/srv/a=b/c"))
        );
        assert!(parse_spec("Api=/srv/a").is_err());
        assert!(parse_spec("api=").is_err());
    }

    #[test]
    fn adding_and_removing_sources_keeps_the_rest_of_the_file() {
        let one = with_source(CONFIG, &source("api", "/repos/api")).unwrap();
        let two = with_source(&one, &source("docs", "/srv/docs")).unwrap();
        assert!(two.contains("# hand-written") && two.contains("# the sources"));
        let cfg: EkosConfig = toml::from_str(&two).unwrap();
        assert_eq!(
            cfg.sources
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["api", "docs"]
        );
        assert_eq!(cfg.sources[0].kind, SourceKind::Git);
        assert_eq!(cfg.sources[1].path, PathBuf::from("/srv/docs"));

        // A duplicate name or directory is refused and nothing would be written.
        assert!(with_source(&two, &source("api", "/elsewhere")).is_err());
        assert!(with_source(&two, &source("other", "/repos/api")).is_err());

        let back = without_source(&two, "api").unwrap();
        let cfg: EkosConfig = toml::from_str(&back).unwrap();
        assert_eq!(cfg.sources.len(), 1);
        assert!(without_source(&back, "api").is_err());
        let empty = without_source(&back, "docs").unwrap();
        assert!(!empty.contains("[[sources]]") && empty.contains("# hand-written"));
        assert!(
            toml::from_str::<EkosConfig>(&empty)
                .unwrap()
                .sources
                .is_empty()
        );
    }

    #[test]
    fn a_compiled_project_without_source_names_cannot_gain_them() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("api");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let config_path = tmp.path().join("ekos.toml");
        std::fs::write(&config_path, CONFIG).unwrap();
        let config = EkosConfig::default();

        let err = add(&config_path, &config, tmp.path(), 12, "api", &dir, None).unwrap_err();
        assert!(err.to_string().contains("without source names"), "{err}");
        assert_eq!(std::fs::read_to_string(&config_path).unwrap(), CONFIG);

        assert!(add(&config_path, &config, tmp.path(), 0, "Bad", &dir, None).is_err());
        assert!(
            add(
                &config_path,
                &config,
                tmp.path(),
                0,
                "api",
                &dir.join("nope"),
                None
            )
            .is_err()
        );

        add(&config_path, &config, tmp.path(), 0, "api", &dir, None).unwrap();
        let cfg = EkosConfig::from_file(&config_path).unwrap();
        assert_eq!(
            cfg.sources[0].kind,
            SourceKind::Git,
            "a repository is detected"
        );
        let listed = infos(&cfg, tmp.path());
        assert!(listed[0].exists && listed[0].git);

        // Once a project has named sources, more can be added whatever the ledger holds.
        let docs = tmp.path().join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        add(&config_path, &cfg, tmp.path(), 500, "docs", &docs, None).unwrap();
        remove(&config_path, "api").unwrap();
        let cfg = EkosConfig::from_file(&config_path).unwrap();
        assert_eq!(cfg.sources.len(), 1);
        assert_eq!(cfg.sources[0].kind, SourceKind::Dir);
    }
}
