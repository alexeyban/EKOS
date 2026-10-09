//! RFC 0171 — source freshness: does the ledger still match the source it was compiled from?
//!
//! `ekos build` records a per-file manifest (path, size, mtime) of everything it observed, in the
//! same walk that computes its fingerprint (`.ekos/source-manifest.json`). A successful
//! `ekos commit` promotes it to `.ekos/committed-manifest.json`, the definition of "what the
//! ledger reflects". [`check`] walks the tree again and diffs the two.
//!
//! Metadata only: no file is read, so this is not a raw-content entry point (RFC 0043). "Changed"
//! means size or mtime differs, so a file that was only touched counts as changed — reported as
//! such, never hidden.

use anyhow::Result;
use chrono::{DateTime, Utc};
use ekos_compiler_core::EkosConfig;
use ekos_ledger::KnowledgeStore;
use ekos_observation_sdk::{FileStamp, ScanContext, source_manifest};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Component, Path, PathBuf};

/// Written by `ekos build`.
pub const SOURCE_MANIFEST: &str = "source-manifest.json";
/// Promoted by a successful `ekos commit`: what the ledger reflects.
pub const COMMITTED_MANIFEST: &str = "committed-manifest.json";
/// Printed with every report, so a touched-only file is never mistaken for an edit.
pub const RULE: &str = "a file counts as changed when its size or modification time differs";

/// One file's metadata in a manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub size: u64,
    pub mtime_nanos: u64,
}

/// Every observed file, keyed by workspace-relative path, plus where the tree was in git.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceManifest {
    pub version: u32,
    pub created_at: DateTime<Utc>,
    /// Set when `ekos commit` promoted this manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_head: Option<String>,
    pub files: BTreeMap<String, Stamp>,
}

impl SourceManifest {
    fn new(git_head: Option<String>) -> Self {
        Self {
            version: 1,
            created_at: Utc::now(),
            committed_at: None,
            git_head,
            files: BTreeMap::new(),
        }
    }

    /// Add one observe path's files. `prefix` is that path relative to the workspace root (empty
    /// for the root itself), so every key is workspace-relative, like evidence paths.
    pub fn add(&mut self, prefix: &str, stamps: &[FileStamp]) {
        for s in stamps {
            let key = if prefix.is_empty() {
                s.path.clone()
            } else {
                format!("{prefix}/{}", s.path)
            };
            self.files.insert(
                key,
                Stamp {
                    size: s.size,
                    mtime_nanos: u64::try_from(s.mtime_nanos).unwrap_or(u64::MAX),
                },
            );
        }
    }
}

/// The files that differ between two manifests, each list sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ManifestDiff {
    pub changed: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl ManifestDiff {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.added.is_empty() && self.removed.is_empty()
    }

    pub fn total(&self) -> usize {
        self.changed.len() + self.added.len() + self.removed.len()
    }
}

/// `then` → `now`.
pub fn diff(then: &SourceManifest, now: &SourceManifest) -> ManifestDiff {
    let mut d = ManifestDiff::default();
    for (path, stamp) in &now.files {
        match then.files.get(path) {
            None => d.added.push(path.clone()),
            Some(old) if old != stamp => d.changed.push(path.clone()),
            Some(_) => {}
        }
    }
    for path in then.files.keys() {
        if !now.files.contains_key(path) {
            d.removed.push(path.clone());
        }
    }
    d
}

/// The directories `ekos build` observes — the same rule `commands/build.rs` applies.
pub fn observe_bases(config: &EkosConfig, cwd: &Path) -> Vec<PathBuf> {
    if config.observe.paths.is_empty() {
        vec![cwd.to_path_buf()]
    } else {
        config.observe.paths.iter().map(|p| cwd.join(p)).collect()
    }
}

/// `base` relative to `cwd` as a `/`-joined string; empty for the workspace root (`"."`).
pub fn prefix_for(base: &Path, cwd: &Path) -> String {
    let rel = base.strip_prefix(cwd).unwrap_or(base);
    rel.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// EKOS's own state directory, relative to the workspace, when it lives inside it. Its files
/// change on every commit and must never count as source.
fn own_state_prefix(config: &EkosConfig, cwd: &Path) -> Option<String> {
    let dir = config.ekos_dir(cwd);
    dir.strip_prefix(cwd)
        .ok()
        .map(|_| prefix_for(&dir, cwd))
        .filter(|p| !p.is_empty())
}

/// Drop EKOS's own state files from a manifest.
fn without_own_state(mut m: SourceManifest, config: &EkosConfig, cwd: &Path) -> SourceManifest {
    if let Some(p) = own_state_prefix(config, cwd) {
        let dir = format!("{p}/");
        m.files.retain(|k, _| !k.starts_with(&dir));
    }
    m
}

/// Build a manifest from per-observe-path stamps that `ekos build` already walked.
pub fn manifest_from(
    config: &EkosConfig,
    cwd: &Path,
    per_base: &[(PathBuf, Vec<FileStamp>)],
) -> SourceManifest {
    let mut m = SourceManifest::new(git_head(cwd));
    for (base, stamps) in per_base {
        m.add(&prefix_for(base, cwd), stamps);
    }
    without_own_state(m, config, cwd)
}

/// Walk every observe path now (metadata only) — what the source looks like at this moment.
pub fn scan(config: &EkosConfig, cwd: &Path) -> SourceManifest {
    let per_base: Vec<(PathBuf, Vec<FileStamp>)> = observe_bases(config, cwd)
        .into_iter()
        .map(|base| {
            let ctx = ScanContext::new(&base)
                .with_ignore_patterns(config.observe.ignore_patterns.clone());
            let stamps = source_manifest(&ctx);
            (base, stamps)
        })
        .collect();
    manifest_from(config, cwd, &per_base)
}

/// `git rev-parse HEAD` in the workspace, when it is a repository with a commit.
pub fn git_head(cwd: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(cwd)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let head = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!head.is_empty()).then_some(head)
}

fn manifest_path(config: &EkosConfig, cwd: &Path, name: &str) -> PathBuf {
    config.ekos_dir(cwd).join(name)
}

pub fn read_manifest(path: &Path) -> Option<SourceManifest> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_manifest(path: &Path, m: &SourceManifest) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string(m)?)?;
    Ok(())
}

/// `ekos build`: record what was observed.
pub fn record_build(config: &EkosConfig, cwd: &Path, m: &SourceManifest) -> Result<()> {
    write_manifest(&manifest_path(config, cwd, SOURCE_MANIFEST), m)
}

/// `ekos commit`, after it succeeded: the built manifest becomes what the ledger reflects.
/// Returns `false` when there is nothing to promote (no `ekos build` since RFC 0171).
pub fn promote_after_commit(config: &EkosConfig, cwd: &Path) -> Result<bool> {
    let Some(mut m) = read_manifest(&manifest_path(config, cwd, SOURCE_MANIFEST)) else {
        return Ok(false);
    };
    m.committed_at = Some(Utc::now());
    write_manifest(&manifest_path(config, cwd, COMMITTED_MANIFEST), &m)?;
    Ok(true)
}

/// `ekos compile` / `ekos commit`: the `FRESH001` warning text when the source moved after
/// `ekos build`, `None` when it did not (or there is no build manifest, or the check is off).
pub fn changed_since_build(config: &EkosConfig, cwd: &Path) -> Option<String> {
    if !config.freshness.enabled {
        return None;
    }
    let built = read_manifest(&manifest_path(config, cwd, SOURCE_MANIFEST))?;
    let d = diff(&built, &scan(config, cwd));
    (!d.is_empty()).then(|| {
        format!(
            "FRESH001 {} file(s) changed since `ekos build` ({} changed, {} added, {} removed{}) \
             — run `ekos build` first, or this compile reflects the older source",
            d.total(),
            d.changed.len(),
            d.added.len(),
            d.removed.len(),
            first_paths(&d, 3)
        )
    })
}

fn first_paths(d: &ManifestDiff, n: usize) -> String {
    let all: Vec<&String> = d
        .changed
        .iter()
        .chain(&d.added)
        .chain(&d.removed)
        .take(n)
        .collect();
    if all.is_empty() {
        String::new()
    } else {
        let more = if d.total() > n { ", …" } else { "" };
        format!(
            ": {}{more}",
            all.iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// Whether the ledger still matches the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Fresh,
    SourceChanged,
    /// Nothing committed since RFC 0171 (or `[freshness] enabled = false`).
    Unknown,
}

/// The query-time report.
#[derive(Debug, Clone, Serialize)]
pub struct Freshness {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_head_then: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_head_now: Option<String>,
    pub changed_count: usize,
    pub added_count: usize,
    pub removed_count: usize,
    /// The first `limit` paths of each list.
    pub changed: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub rule: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Freshness {
    fn unknown(note: &str) -> Self {
        Self {
            status: Status::Unknown,
            committed_at: None,
            git_head_then: None,
            git_head_now: None,
            changed_count: 0,
            added_count: 0,
            removed_count: 0,
            changed: Vec::new(),
            added: Vec::new(),
            removed: Vec::new(),
            rule: RULE,
            note: Some(note.to_string()),
        }
    }

    pub fn total(&self) -> usize {
        self.changed_count + self.added_count + self.removed_count
    }

    /// One line for `ekos status` / `ekos doctor`.
    pub fn summary_line(&self) -> String {
        match self.status {
            Status::Unknown => format!(
                "unknown — {}",
                self.note.as_deref().unwrap_or("no committed manifest")
            ),
            _ => {
                let when = self
                    .committed_at
                    .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                    .unwrap_or_else(|| "?".into());
                let git = self
                    .git_head_then
                    .as_deref()
                    .map(|h| format!(" (git {})", &h[..h.len().min(7)]))
                    .unwrap_or_default();
                if self.status == Status::Fresh {
                    format!("fresh — ledger reflects the source as of {when}{git}")
                } else {
                    format!(
                        "source changed — ledger reflects the source as of {when}{git}; {} changed, {} added, {} removed since",
                        self.changed_count, self.added_count, self.removed_count
                    )
                }
            }
        }
    }
}

/// Compare the source now with what the ledger reflects. `limit` caps each path list.
pub fn check(config: &EkosConfig, cwd: &Path, limit: usize) -> Freshness {
    if !config.freshness.enabled {
        return Freshness::unknown("[freshness] enabled = false");
    }
    let Some(committed) = read_manifest(&manifest_path(config, cwd, COMMITTED_MANIFEST)) else {
        return Freshness::unknown(
            "no committed source manifest yet — run `ekos build` … `ekos commit` once",
        );
    };
    let now = scan(config, cwd);
    let d = diff(&committed, &now);
    let cap = |v: &Vec<String>| v.iter().take(limit).cloned().collect::<Vec<_>>();
    Freshness {
        // A new git commit that changed no observed file is still fresh: the ledger describes
        // exactly these files.
        status: if d.is_empty() {
            Status::Fresh
        } else {
            Status::SourceChanged
        },
        committed_at: committed.committed_at,
        git_head_then: committed.git_head,
        git_head_now: now.git_head,
        changed_count: d.changed.len(),
        added_count: d.added.len(),
        removed_count: d.removed.len(),
        changed: cap(&d.changed),
        added: cap(&d.added),
        removed: cap(&d.removed),
        rule: RULE,
        note: None,
    }
}

/// An object whose evidence cites a file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct CitingObject {
    pub kind: String,
    pub name: String,
    pub id: String,
}

/// For each path, the ledger objects whose evidence cites it — "these facts may be stale".
pub fn citing_objects(
    store: &dyn KnowledgeStore,
    paths: &[String],
) -> Result<BTreeMap<String, Vec<CitingObject>>> {
    let wanted: HashSet<&str> = paths.iter().map(String::as_str).collect();
    let mut out: BTreeMap<String, BTreeSet<CitingObject>> = BTreeMap::new();
    if wanted.is_empty() {
        return Ok(BTreeMap::new());
    }
    for obj in store.all_objects()? {
        for ev_id in &obj.evidence {
            let Some(ev) = store.get_evidence(ev_id)? else {
                continue;
            };
            let p = ev.location.path.trim_start_matches("./");
            if wanted.contains(p) {
                out.entry(p.to_string()).or_default().insert(CitingObject {
                    kind: kind_label(&obj.kind),
                    name: obj.name.clone(),
                    id: obj.id.to_string(),
                });
            }
        }
    }
    Ok(out
        .into_iter()
        .map(|(k, v)| (k, v.into_iter().collect()))
        .collect())
}

fn kind_label(kind: &ekos_kir::ObjectKind) -> String {
    match kind {
        ekos_kir::ObjectKind::Custom(k) => k.clone(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamps(entries: &[(&str, u64, u128)]) -> Vec<FileStamp> {
        entries
            .iter()
            .map(|(p, s, m)| FileStamp {
                path: (*p).into(),
                size: *s,
                mtime_nanos: *m,
            })
            .collect()
    }

    fn manifest(entries: &[(&str, u64, u128)]) -> SourceManifest {
        let mut m = SourceManifest::new(None);
        m.add("", &stamps(entries));
        m
    }

    #[test]
    fn diff_reports_changed_added_and_removed_sorted() {
        let then = manifest(&[("a.sql", 10, 1), ("b.sql", 20, 2), ("gone.rs", 5, 5)]);
        let now = manifest(&[
            ("a.sql", 10, 1),
            ("b.sql", 21, 2),
            ("new.py", 1, 9),
            ("c.md", 3, 3),
        ]);
        let d = diff(&then, &now);
        assert_eq!(d.changed, vec!["b.sql"]);
        assert_eq!(d.added, vec!["c.md", "new.py"]);
        assert_eq!(d.removed, vec!["gone.rs"]);
        assert_eq!(d.total(), 4);
        assert!(diff(&now, &now).is_empty());
    }

    #[test]
    fn a_touched_file_counts_as_changed() {
        let then = manifest(&[("a.sql", 10, 1)]);
        let now = manifest(&[("a.sql", 10, 2)]);
        assert_eq!(diff(&then, &now).changed, vec!["a.sql"]);
    }

    #[test]
    fn keys_are_workspace_relative_for_a_sub_path() {
        let cwd = Path::new("/ws");
        assert_eq!(prefix_for(Path::new("/ws/."), cwd), "");
        assert_eq!(prefix_for(Path::new("/ws/sql/modules"), cwd), "sql/modules");
        let mut m = SourceManifest::new(None);
        m.add("sql", &stamps(&[("x.sql", 1, 1)]));
        assert!(m.files.contains_key("sql/x.sql"));
    }

    #[test]
    fn manifest_round_trips_through_json() {
        let mut m = manifest(&[("a.sql", 10, 1_759_000_000_000_000_000)]);
        m.git_head = Some("abc".into());
        m.committed_at = Some(Utc::now());
        let back: SourceManifest =
            serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back, m);
    }

    fn workspace() -> (tempfile::TempDir, EkosConfig) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("schema.sql"), "create table t (id int);").unwrap();
        (dir, EkosConfig::default())
    }

    #[test]
    fn unknown_until_a_commit_promotes_the_build_manifest() {
        let (dir, config) = workspace();
        let cwd = dir.path();
        assert_eq!(check(&config, cwd, 10).status, Status::Unknown);

        record_build(&config, cwd, &scan(&config, cwd)).unwrap();
        // Built, not committed: the ledger has not caught up yet, so still unknown.
        assert_eq!(check(&config, cwd, 10).status, Status::Unknown);

        assert!(promote_after_commit(&config, cwd).unwrap());
        let f = check(&config, cwd, 10);
        assert_eq!(f.status, Status::Fresh, "{f:?}");
        assert!(f.committed_at.is_some());

        std::fs::write(
            cwd.join("schema.sql"),
            "create table t (id int, name text);",
        )
        .unwrap();
        std::fs::write(cwd.join("new.sql"), "select 1;").unwrap();
        let f = check(&config, cwd, 10);
        assert_eq!(f.status, Status::SourceChanged);
        assert_eq!((f.changed_count, f.added_count, f.removed_count), (1, 1, 0));
        assert_eq!(f.changed, vec!["schema.sql"]);
        assert!(f.summary_line().contains("1 changed, 1 added, 0 removed"));
    }

    #[test]
    fn ekos_state_files_never_count_as_source() {
        let (dir, mut config) = workspace();
        let cwd = dir.path();
        config.observe.ignore_patterns.clear(); // a config that forgot `.ekos`
        record_build(&config, cwd, &scan(&config, cwd)).unwrap();
        promote_after_commit(&config, cwd).unwrap();
        // The manifests themselves now exist under .ekos/ — they must not make it stale.
        assert_eq!(check(&config, cwd, 10).status, Status::Fresh);
        assert!(
            !scan(&config, cwd)
                .files
                .keys()
                .any(|k| k.starts_with(".ekos/"))
        );
    }

    #[test]
    fn changed_since_build_warns_only_when_the_source_moved() {
        let (dir, config) = workspace();
        let cwd = dir.path();
        assert!(
            changed_since_build(&config, cwd).is_none(),
            "no build manifest yet"
        );
        record_build(&config, cwd, &scan(&config, cwd)).unwrap();
        assert!(changed_since_build(&config, cwd).is_none());
        std::fs::write(cwd.join("schema.sql"), "create table t (id bigint);").unwrap();
        let w = changed_since_build(&config, cwd).unwrap();
        assert!(w.starts_with("FRESH001 1 file(s)"), "{w}");
        assert!(w.contains("schema.sql"));
    }

    #[test]
    fn disabled_means_unknown_and_no_warning() {
        let (dir, mut config) = workspace();
        let cwd = dir.path();
        record_build(&config, cwd, &scan(&config, cwd)).unwrap();
        promote_after_commit(&config, cwd).unwrap();
        std::fs::write(cwd.join("schema.sql"), "changed").unwrap();
        config.freshness.enabled = false;
        assert_eq!(check(&config, cwd, 10).status, Status::Unknown);
        assert!(changed_since_build(&config, cwd).is_none());
    }
}
