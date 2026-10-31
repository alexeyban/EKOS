//! RFC 0183 — `ekos home backup` / `ekos home restore`.
//!
//! A backup is a zstd-compressed tar of the projects in the EKOS home and, when it covers every
//! tenant, the console's `control/` directory. Each project is copied under its ledger's write
//! lock; the console database is copied with SQLite's `VACUUM INTO`.
//!
//! Restore never deletes: an existing project is replaced only with `--force`, and then it is
//! moved to the home's trash first.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::home::{list_projects, validate_name};
use ekos_compiler_core::ProjectLocation;

/// The archive's format number. Restore refuses a larger one.
pub const FORMAT: u32 = 1;
pub const MANIFEST: &str = "ekos-backup.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub format: u32,
    pub created_at: DateTime<Utc>,
    pub ekos_version: String,
    /// Tenant → its projects in the archive.
    pub tenants: BTreeMap<String, Vec<String>>,
    /// Whether `control/` (the console database and run logs) is in the archive.
    pub control: bool,
}

fn stamp(now: DateTime<Utc>) -> String {
    now.format("%Y%m%dT%H%M%SZ").to_string()
}

/// The fact ledger's directory inside a project, where its write lock lives.
fn facts_dir(project: &ProjectLocation) -> PathBuf {
    project.state_dir().join("ledger").join("facts")
}

/// Hold the project's write lock for as long as the result lives. `None` when the project has
/// no fact ledger (nothing compiled yet, or an older SQLite ledger) and so no lock to take.
fn lock_project(project: &ProjectLocation) -> Result<Option<std::fs::File>> {
    let facts = facts_dir(project);
    if !facts.is_dir() {
        return Ok(None);
    }
    let lock = ekos_ledger::fact_ledger::hold_write_lock(&facts).map_err(|_| {
        anyhow::anyhow!(
            "project {}/{} is being written (a pipeline stage is running); \
             run the backup again when it has finished",
            project.tenant,
            project.project
        )
    })?;
    Ok(Some(lock))
}

/// Every plain file under `dir`, relative to it, sorted. Symlinks are not followed and not
/// copied: a state directory holds none, and one that points outside it has no place in an
/// archive that is later unpacked somewhere else.
fn files_under(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        if entry.file_type().is_file() {
            out.push(entry.path().strip_prefix(dir)?.to_path_buf());
        }
    }
    Ok(out)
}

fn append_dir<W: Write>(tar: &mut tar::Builder<W>, dir: &Path, prefix: &Path) -> Result<usize> {
    let files = files_under(dir)?;
    for rel in &files {
        // The lock file is how the copy is protected, not part of what is protected.
        if rel.file_name().is_some_and(|n| n == "write.lock") {
            continue;
        }
        let mut file = match std::fs::File::open(dir.join(rel)) {
            Ok(f) => f,
            // Removed since the walk (a temp file of a reader); it was not state.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(e).with_context(|| format!("opening {}", dir.join(rel).display()));
            }
        };
        tar.append_file(prefix.join(rel), &mut file)
            .with_context(|| format!("archiving {}", dir.join(rel).display()))?;
    }
    Ok(files.len())
}

/// A consistent copy of a SQLite database that may be in use.
fn snapshot_sqlite(db: &Path, dest: &Path) -> Result<()> {
    let conn = rusqlite::Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening {}", db.display()))?;
    conn.execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()])
        .with_context(|| format!("copying {}", db.display()))?;
    Ok(())
}

pub struct BackupSummary {
    pub path: PathBuf,
    pub manifest: Manifest,
    pub files: usize,
    /// Projects copied without a lock, because their store has none.
    pub unlocked: Vec<String>,
}

/// Write a backup of `home` to `out` (default `<home>/backups/…`). `tenant` limits it to one
/// tenant's projects and leaves `control/` out.
pub fn backup(
    home: &Path,
    tenant: Option<&str>,
    out: Option<PathBuf>,
    now: DateTime<Utc>,
) -> Result<BackupSummary> {
    if let Some(t) = tenant {
        validate_name("tenant", t)?;
    }
    let projects: Vec<ProjectLocation> = list_projects(home)?
        .into_iter()
        .filter(|p| tenant.is_none_or(|t| p.tenant == t))
        .collect();
    let control_dir = home.join("control");
    let control = tenant.is_none() && control_dir.is_dir();
    if projects.is_empty() && !control {
        match tenant {
            Some(t) => bail!("tenant {t} has no projects in {}", home.display()),
            None => bail!("nothing to back up in {}", home.display()),
        }
    }

    let out = out.unwrap_or_else(|| {
        let suffix = tenant.map(|t| format!("-{t}")).unwrap_or_default();
        home.join("backups")
            .join(format!("ekos-backup-{}{suffix}.tar.zst", stamp(now)))
    });
    if out.exists() {
        bail!(
            "{} already exists; a backup is never overwritten",
            out.display()
        );
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    // Every lock is taken before the first byte is written, so a project in use fails the
    // backup while there is still nothing to clean up.
    let mut locks = Vec::new();
    let mut unlocked = Vec::new();
    for project in &projects {
        match lock_project(project)? {
            Some(lock) => locks.push(lock),
            None if project.state_dir().join("ledger").is_dir() => {
                unlocked.push(format!("{}/{}", project.tenant, project.project));
            }
            None => {}
        }
    }

    let mut tenants: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for project in &projects {
        tenants
            .entry(project.tenant.clone())
            .or_default()
            .push(project.project.clone());
    }
    let manifest = Manifest {
        format: FORMAT,
        created_at: now,
        ekos_version: env!("CARGO_PKG_VERSION").to_string(),
        tenants,
        control,
    };

    // Written beside the destination and renamed, so a failed backup leaves no file that looks
    // like a finished one.
    let partial = out.with_extension("partial");
    let result = write_archive(home, &projects, control, &manifest, &partial);
    drop(locks);
    let files = match result {
        Ok(files) => files,
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            return Err(e);
        }
    };
    std::fs::rename(&partial, &out)
        .with_context(|| format!("moving the backup to {}", out.display()))?;
    Ok(BackupSummary {
        path: out,
        manifest,
        files,
        unlocked,
    })
}

fn write_archive(
    home: &Path,
    projects: &[ProjectLocation],
    control: bool,
    manifest: &Manifest,
    dest: &Path,
) -> Result<usize> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Ledgers, and the console database with its token hashes.
        options.mode(0o600);
    }
    let file = options
        .open(dest)
        .with_context(|| format!("creating {}", dest.display()))?;
    let encoder = zstd::Encoder::new(file, 3)?;
    let mut tar = tar::Builder::new(encoder);
    tar.follow_symlinks(false);

    let manifest_json = serde_json::to_vec_pretty(manifest)?;
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest_json.len() as u64);
    header.set_mode(0o600);
    header.set_mtime(manifest.created_at.timestamp().max(0) as u64);
    header.set_cksum();
    tar.append_data(&mut header, MANIFEST, manifest_json.as_slice())?;

    let mut files = 0;
    for project in projects {
        let prefix = Path::new("data")
            .join(&project.tenant)
            .join(&project.project);
        files += append_dir(&mut tar, &project.dir(), &prefix)?;
    }
    if control {
        files += append_control(&mut tar, &home.join("control"))?;
    }
    tar.into_inner()?.finish()?.sync_all()?;
    Ok(files)
}

fn append_control<W: Write>(tar: &mut tar::Builder<W>, control: &Path) -> Result<usize> {
    let mut files = 0;
    let db = control.join("console.db");
    if db.is_file() {
        let scratch = tempfile_in(control)?;
        let result = snapshot_sqlite(&db, &scratch).and_then(|()| {
            let mut file = std::fs::File::open(&scratch)?;
            tar.append_file("control/console.db", &mut file)?;
            Ok(())
        });
        let _ = std::fs::remove_file(&scratch);
        result?;
        files += 1;
    }
    let runs = control.join("runs");
    if runs.is_dir() {
        files += append_dir(tar, &runs, Path::new("control/runs"))?;
    }
    Ok(files)
}

/// A path for a scratch file in `dir` that does not exist yet (`VACUUM INTO` creates it).
fn tempfile_in(dir: &Path) -> Result<PathBuf> {
    for n in 0..100u32 {
        let candidate = dir.join(format!(".backup-snapshot-{}-{n}.db", std::process::id()));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!("could not find a scratch file name in {}", dir.display())
}

// ── restore ──────────────────────────────────────────────────────────────────

/// An archive entry's path, checked: relative, made of plain names only.
fn safe_path(raw: &Path) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for part in raw.components() {
        match part {
            Component::Normal(name) => out.push(name),
            Component::CurDir => {}
            _ => bail!(
                "the archive holds the path {:?}, which would leave the directory it is unpacked \
                 into; this is not an EKOS backup",
                raw.display().to_string()
            ),
        }
    }
    if out.as_os_str().is_empty() {
        bail!("the archive holds an entry with no path");
    }
    Ok(out)
}

fn read_manifest(reader: impl Read) -> Result<Manifest> {
    let manifest: Manifest =
        serde_json::from_reader(reader).context("the archive's manifest does not parse")?;
    if manifest.format > FORMAT {
        bail!(
            "this backup was written in format {} by ekos {}; this ekos reads up to format \
             {FORMAT}. Restore it with that version or a newer one",
            manifest.format,
            manifest.ekos_version
        );
    }
    for (tenant, projects) in &manifest.tenants {
        validate_name("tenant", tenant)?;
        for project in projects {
            validate_name("project", project)?;
        }
    }
    Ok(manifest)
}

/// Unpack the archive into `staging`: the manifest first, then plain files and directories
/// only, each at a checked path.
fn unpack(archive: &Path, staging: &Path) -> Result<Manifest> {
    let file =
        std::fs::File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let decoder = zstd::Decoder::new(file)
        .with_context(|| format!("{} is not a zstd archive", archive.display()))?;
    let mut tar = tar::Archive::new(decoder);
    let mut manifest: Option<Manifest> = None;
    for entry in tar.entries().context("reading the archive")? {
        let mut entry = entry.context("reading the archive")?;
        let rel = safe_path(&entry.path()?)?;
        let kind = entry.header().entry_type();
        if manifest.is_none() {
            if rel != Path::new(MANIFEST) || !kind.is_file() {
                bail!(
                    "{} does not start with {MANIFEST}: not an EKOS backup",
                    archive.display()
                );
            }
            manifest = Some(read_manifest(&mut entry)?);
            continue;
        }
        let dest = staging.join(&rel);
        if kind.is_dir() {
            std::fs::create_dir_all(&dest)?;
        } else if kind.is_file() {
            if !(rel.starts_with("data") || rel.starts_with("control")) {
                bail!(
                    "the archive holds {:?}, which is not part of a backup",
                    rel.display().to_string()
                );
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&dest)
                .with_context(|| format!("writing {}", dest.display()))?;
            std::io::copy(&mut entry, &mut out)?;
        } else {
            bail!(
                "the archive holds {:?}, which is neither a file nor a directory (a link, a \
                 device); this is not an EKOS backup",
                rel.display().to_string()
            );
        }
    }
    manifest.with_context(|| format!("{} is empty", archive.display()))
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RestoreSummary {
    /// `tenant/project` restored.
    pub projects: Vec<String>,
    /// Where each replaced project (and the replaced `control/`) went.
    pub replaced: Vec<PathBuf>,
    pub control: bool,
}

/// Restore `archive` into `home`. `tenant` restores that tenant's projects only. Without
/// `force` nothing that exists is replaced, and nothing is changed if anything would be.
pub fn restore(
    home: &Path,
    archive: &Path,
    tenant: Option<&str>,
    force: bool,
    now: DateTime<Utc>,
) -> Result<RestoreSummary> {
    if let Some(t) = tenant {
        validate_name("tenant", t)?;
    }
    std::fs::create_dir_all(home).with_context(|| format!("creating {}", home.display()))?;
    let staging = home.join(format!(".restore-{}-{}", stamp(now), std::process::id()));
    if staging.exists() {
        bail!("{} already exists; try again", staging.display());
    }
    std::fs::create_dir_all(&staging)?;
    let result = restore_from(home, archive, tenant, force, now, &staging);
    // Whatever happened, the staging directory is this command's own.
    let _ = std::fs::remove_dir_all(&staging);
    result
}

fn restore_from(
    home: &Path,
    archive: &Path,
    tenant: Option<&str>,
    force: bool,
    now: DateTime<Utc>,
    staging: &Path,
) -> Result<RestoreSummary> {
    let manifest = unpack(archive, staging)?;

    let mut wanted: Vec<(String, String)> = Vec::new();
    for (t, projects) in &manifest.tenants {
        if tenant.is_some_and(|only| only != t) {
            continue;
        }
        for p in projects {
            if !staging
                .join("data")
                .join(t)
                .join(p)
                .join("ekos.toml")
                .is_file()
            {
                bail!("the archive names project {t}/{p} and does not hold it");
            }
            wanted.push((t.clone(), p.clone()));
        }
    }
    if let Some(only) = tenant
        && wanted.is_empty()
    {
        bail!("the archive holds no project of tenant {only}");
    }
    let control = tenant.is_none() && manifest.control && staging.join("control").is_dir();

    // Decide everything before moving anything.
    let existing: Vec<String> = wanted
        .iter()
        .filter(|(t, p)| home.join("data").join(t).join(p).exists())
        .map(|(t, p)| format!("{t}/{p}"))
        .collect();
    let control_exists = control && home.join("control").join("console.db").exists();
    if !force && (!existing.is_empty() || control_exists) {
        let mut what = existing.clone();
        if control_exists {
            what.push("the console database (control/)".to_string());
        }
        bail!(
            "already present in {}: {}. Nothing was changed. With --force each is moved to the \
             trash and replaced",
            home.display(),
            what.join(", ")
        );
    }

    let mut summary = RestoreSummary::default();
    let when = stamp(now);
    for (t, p) in &wanted {
        let dest = home.join("data").join(t).join(p);
        if dest.exists() {
            let trash = home.join("trash").join(t);
            std::fs::create_dir_all(&trash)?;
            let kept = trash.join(format!("{p}-{when}"));
            if kept.exists() {
                bail!("{} already exists; try again", kept.display());
            }
            std::fs::rename(&dest, &kept)
                .with_context(|| format!("moving {} to {}", dest.display(), kept.display()))?;
            summary.replaced.push(kept);
        }
        std::fs::create_dir_all(dest.parent().expect("a project has a tenant directory"))?;
        std::fs::rename(staging.join("data").join(t).join(p), &dest)
            .with_context(|| format!("placing {}", dest.display()))?;
        summary.projects.push(format!("{t}/{p}"));
    }
    if control {
        let dest = home.join("control");
        if dest.exists() {
            let kept = home.join("trash").join(format!("control-{when}"));
            std::fs::create_dir_all(home.join("trash"))?;
            std::fs::rename(&dest, &kept)
                .with_context(|| format!("moving {} to {}", dest.display(), kept.display()))?;
            summary.replaced.push(kept);
        }
        std::fs::rename(staging.join("control"), &dest)
            .with_context(|| format!("placing {}", dest.display()))?;
        summary.control = true;
    }
    Ok(summary)
}

// ── command line ─────────────────────────────────────────────────────────────

pub fn run_backup(home: &Path, tenant: Option<&str>, out: Option<PathBuf>) -> Result<()> {
    let summary = backup(home, tenant, out, Utc::now())?;
    let projects: usize = summary.manifest.tenants.values().map(Vec::len).sum();
    println!(
        "Backed up {projects} project(s) in {} tenant(s){}: {} file(s)",
        summary.manifest.tenants.len(),
        if summary.manifest.control {
            " and the console's control/"
        } else {
            ""
        },
        summary.files
    );
    println!("  {}", summary.path.display());
    if !summary.unlocked.is_empty() {
        println!(
            "Copied without a write lock (their store has none): {}. If a pipeline was running \
             for one of them, back it up again.",
            summary.unlocked.join(", ")
        );
    }
    println!("The sources are not in a backup; they are not in the EKOS home.");
    Ok(())
}

/// RFC 0232: back up into `<home>/backups/`, upload the archive to `url`, then remove the local
/// copy. The upload must succeed before anything is removed.
pub fn run_backup_to(home: &Path, tenant: Option<&str>, url: &str) -> Result<()> {
    use super::home_remote;
    if !home_remote::is_remote(url) {
        bail!(
            "--to takes an object-storage URL (s3://…, gs://…, https://<account>.blob.core.windows.net/…); use --out for a file"
        );
    }
    home_remote::available()?;
    let summary = backup(home, tenant, None, Utc::now())?;
    let uploaded = home_remote::upload(&summary.path, url).with_context(|| {
        format!(
            "the upload failed; the backup is kept at {}",
            summary.path.display()
        )
    })?;
    std::fs::remove_file(&summary.path)
        .with_context(|| format!("removing the local copy {}", summary.path.display()))?;
    let projects: usize = summary.manifest.tenants.values().map(Vec::len).sum();
    println!(
        "Backed up {projects} project(s) in {} tenant(s){}: {} file(s)",
        summary.manifest.tenants.len(),
        if summary.manifest.control {
            " and the console's control/"
        } else {
            ""
        },
        summary.files
    );
    println!("  {uploaded}");
    if !summary.unlocked.is_empty() {
        println!(
            "Copied without a write lock (their store has none): {}.",
            summary.unlocked.join(", ")
        );
    }
    Ok(())
}

/// RFC 0232: download a backup from object storage into `<home>/backups/` and restore it.
pub fn run_restore_from(home: &Path, url: &str, tenant: Option<&str>, force: bool) -> Result<()> {
    use super::home_remote;
    let target = home_remote::parse_target(url)?;
    let local = home_remote::download(&target, &home.join("backups"))?;
    println!("Downloaded {}", local.display());
    run_restore(home, &local, tenant, force)
}

pub fn run_restore(home: &Path, archive: &Path, tenant: Option<&str>, force: bool) -> Result<()> {
    let summary = restore(home, archive, tenant, force, Utc::now())?;
    println!("Restored into {}:", home.display());
    for p in &summary.projects {
        println!("  project {p}");
    }
    if summary.control {
        println!("  the console's control/ (database and run logs)");
    }
    for kept in &summary.replaced {
        println!("What was there before is kept in {}", kept.display());
    }
    if !summary.control && !summary.projects.is_empty() {
        println!(
            "A console over this home lists these projects at its next start; role bindings \
             are in control/, which this restore did not include."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_that_leaves_the_directory_is_refused() {
        for bad in [
            "../x",
            "data/../../x",
            "/etc/passwd",
            "data/a/../../../x",
            "",
        ] {
            assert!(safe_path(Path::new(bad)).is_err(), "{bad}");
        }
        assert_eq!(
            safe_path(Path::new("./data/default/shop/ekos.toml")).unwrap(),
            Path::new("data/default/shop/ekos.toml")
        );
    }

    #[test]
    fn a_manifest_from_a_newer_format_or_with_a_bad_name_is_refused() {
        let ok = serde_json::json!({
            "format": FORMAT, "created_at": "2026-10-13T10:00:00Z", "ekos_version": "1.0.3",
            "tenants": {"default": ["shop"]}, "control": false
        });
        assert!(read_manifest(ok.to_string().as_bytes()).is_ok());

        let mut newer = ok.clone();
        newer["format"] = serde_json::json!(FORMAT + 1);
        let err = read_manifest(newer.to_string().as_bytes())
            .unwrap_err()
            .to_string();
        assert!(err.contains("format"), "{err}");

        let mut bad = ok.clone();
        bad["tenants"] = serde_json::json!({"default": ["../escape"]});
        assert!(read_manifest(bad.to_string().as_bytes()).is_err());
        let mut bad = ok;
        bad["tenants"] = serde_json::json!({"Not A Tenant": ["shop"]});
        assert!(read_manifest(bad.to_string().as_bytes()).is_err());
        assert!(read_manifest(&b"not json"[..]).is_err());
    }
}
