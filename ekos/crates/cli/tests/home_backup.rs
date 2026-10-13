//! RFC 0183 end to end, through the real binary: a backup of the EKOS home restores into an
//! empty home and answers the same queries; a project being written is refused; nothing that
//! exists is replaced without `--force`, and then it is kept in the trash; an archive that
//! would write outside the home is refused.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn ekos(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ekos"))
        .args(args)
        .current_dir(home)
        .env("EKOS_HOME", home)
        .env_remove("EKOS_PROJECT")
        .env_remove("EKOS_TENANT")
        .env_remove("EKOS_CONFIG")
        .env_remove("EKOS_WORKSPACE")
        .env_remove("EKOS_READ_SCOPE")
        .output()
        .expect("running the ekos binary")
}

fn ok(home: &Path, args: &[&str]) -> String {
    let out = ekos(home, args);
    assert!(
        out.status.success(),
        "ekos {args:?} failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn refused(home: &Path, args: &[&str]) -> String {
    let out = ekos(home, args);
    assert!(!out.status.success(), "ekos {args:?} should have failed");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn sources(dir: &Path, table: &str) {
    std::fs::create_dir_all(dir.join("db")).unwrap();
    std::fs::write(
        dir.join("db/schema.sql"),
        format!("CREATE TABLE {table} (id SERIAL PRIMARY KEY, total NUMERIC);\n"),
    )
    .unwrap();
}

/// A compiled project in `home`, for `tenant`.
fn project(home: &Path, root: &Path, tenant: &str, name: &str, table: &str) {
    let src = root.join(format!("src-{tenant}-{name}"));
    sources(&src, table);
    std::fs::create_dir_all(home).unwrap();
    let src = src.to_str().unwrap();
    ok(
        home,
        &[
            "--tenant", tenant, "project", "add", name, "--source", src, "--detect",
        ],
    );
    for stage in [
        vec!["build"],
        vec!["recover"],
        vec!["resolve"],
        vec!["compile"],
        vec!["commit", "--yes"],
    ] {
        let mut args = vec!["--tenant", tenant, "--project", name];
        args.extend(stage);
        ok(home, &args);
    }
}

/// Every object's id and name, as `ekos ekl` prints them.
fn objects(home: &Path, tenant: &str, name: &str) -> BTreeSet<(String, String)> {
    let out = ok(
        home,
        &[
            "--tenant",
            tenant,
            "--project",
            name,
            "ekl",
            "FIND Object",
            "--json",
        ],
    );
    let rows: serde_json::Value = serde_json::from_str(&out).expect("EKL JSON");
    rows.as_array()
        .expect("an array of rows")
        .iter()
        .map(|r| {
            (
                r["id"].as_str().unwrap().to_string(),
                r["name"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn archive_entries(archive: &Path) -> Vec<String> {
    let file = std::fs::File::open(archive).unwrap();
    let mut tar = tar::Archive::new(zstd::Decoder::new(file).unwrap());
    tar.entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_backup_restores_into_an_empty_home_and_answers_the_same() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    project(&home, tmp.path(), "default", "shop", "orders_ibex");
    project(&home, tmp.path(), "acme", "erp", "ledger_okapi");
    // A console database, as the console would leave it.
    std::fs::create_dir_all(home.join("control/runs")).unwrap();
    let db = rusqlite::Connection::open(home.join("control/console.db")).unwrap();
    db.execute_batch("CREATE TABLE workspace (id TEXT); INSERT INTO workspace VALUES ('shop');")
        .unwrap();
    drop(db);
    std::fs::write(home.join("control/runs/r1.log"), "ran\n").unwrap();
    // Things a backup leaves out.
    std::fs::create_dir_all(home.join("bin")).unwrap();
    std::fs::write(home.join("bin/ekos"), "binary").unwrap();
    std::fs::create_dir_all(home.join("trash/default/old-1")).unwrap();
    std::fs::write(home.join("trash/default/old-1/ekos.toml"), "").unwrap();

    let before_shop = objects(&home, "default", "shop");
    let before_erp = objects(&home, "acme", "erp");
    assert!(before_shop.iter().any(|(_, n)| n == "orders_ibex"));

    let archive = tmp.path().join("all.tar.zst");
    let printed = ok(
        &home,
        &["home", "backup", "--out", archive.to_str().unwrap()],
    );
    assert!(printed.contains("2 project(s) in 2 tenant(s)"), "{printed}");

    let entries = archive_entries(&archive);
    assert_eq!(entries[0], "ekos-backup.json");
    assert!(entries.contains(&"data/default/shop/ekos.toml".to_string()));
    assert!(entries.contains(&"data/acme/erp/ekos.toml".to_string()));
    assert!(entries.contains(&"control/console.db".to_string()));
    assert!(entries.contains(&"control/runs/r1.log".to_string()));
    assert!(
        !entries
            .iter()
            .any(|e| e.starts_with("bin") || e.starts_with("trash") || e.ends_with("write.lock")),
        "{entries:?}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&archive).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    // A backup is never overwritten.
    let again = refused(
        &home,
        &["home", "backup", "--out", archive.to_str().unwrap()],
    );
    assert!(again.contains("never overwritten"), "{again}");

    // Into a home that does not exist yet.
    let fresh = tmp.path().join("fresh");
    std::fs::create_dir_all(&fresh).unwrap();
    let printed = ok(&fresh, &["home", "restore", archive.to_str().unwrap()]);
    assert!(
        printed.contains("project acme/erp") && printed.contains("project default/shop"),
        "{printed}"
    );
    assert_eq!(objects(&fresh, "default", "shop"), before_shop);
    assert_eq!(objects(&fresh, "acme", "erp"), before_erp);
    let db = rusqlite::Connection::open(fresh.join("control/console.db")).unwrap();
    let id: String = db
        .query_row("SELECT id FROM workspace", [], |r| r.get(0))
        .unwrap();
    assert_eq!(id, "shop");
    assert_eq!(
        std::fs::read_to_string(fresh.join("control/runs/r1.log")).unwrap(),
        "ran\n"
    );
    // No staging directory is left behind.
    let leftovers: Vec<PathBuf> = std::fs::read_dir(&fresh)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".restore")
        })
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    // The restored project still compiles further: its ledger is writable.
    ok(&fresh, &["--project", "shop", "status"]);
}

#[test]
fn nothing_that_exists_is_replaced_without_force() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    project(&home, tmp.path(), "default", "shop", "orders_ibex");
    project(&home, tmp.path(), "default", "wiki", "pages_tapir");
    let archive = tmp.path().join("b.tar.zst");
    ok(
        &home,
        &["home", "backup", "--out", archive.to_str().unwrap()],
    );
    let at_backup = objects(&home, "default", "shop");

    // Remove one project; the other still exists.
    ok(&home, &["project", "remove", "wiki", "--purge", "--yes"]);
    let err = refused(&home, &["home", "restore", archive.to_str().unwrap()]);
    assert!(
        err.contains("default/shop") && err.contains("Nothing was changed"),
        "{err}"
    );
    // All or nothing: the missing project was not restored either.
    assert!(!home.join("data/default/wiki").exists());

    // Change the existing project, then force.
    std::fs::write(home.join("data/default/shop/marker.txt"), "newer").unwrap();
    let printed = ok(
        &home,
        &["home", "restore", archive.to_str().unwrap(), "--force"],
    );
    assert!(printed.contains("kept in"), "{printed}");
    assert_eq!(objects(&home, "default", "shop"), at_backup);
    assert!(home.join("data/default/wiki/ekos.toml").is_file());
    assert!(!home.join("data/default/shop/marker.txt").exists());
    // The replaced project is in the trash with what it held.
    let kept: Vec<PathBuf> = std::fs::read_dir(home.join("trash/default"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(
        std::fs::read_to_string(kept[0].join("marker.txt")).unwrap(),
        "newer"
    );
}

#[test]
fn one_tenant_can_be_backed_up_and_restored_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    project(&home, tmp.path(), "default", "shop", "orders_ibex");
    project(&home, tmp.path(), "acme", "erp", "ledger_okapi");
    std::fs::create_dir_all(home.join("control")).unwrap();
    rusqlite::Connection::open(home.join("control/console.db"))
        .unwrap()
        .execute_batch("CREATE TABLE t (x)")
        .unwrap();

    let printed = ok(&home, &["--tenant", "acme", "home", "backup"]);
    let path = printed
        .lines()
        .find(|l| l.trim().ends_with(".tar.zst"))
        .expect("the path of the backup")
        .trim()
        .to_string();
    assert!(
        path.contains("/backups/") && path.ends_with("-acme.tar.zst"),
        "{path}"
    );
    let entries = archive_entries(Path::new(&path));
    assert!(entries.iter().any(|e| e.starts_with("data/acme/erp/")));
    assert!(
        !entries
            .iter()
            .any(|e| e.starts_with("data/default") || e.starts_with("control")),
        "{entries:?}"
    );

    // A whole-home archive, restored for one tenant only.
    let all = tmp.path().join("all.tar.zst");
    ok(&home, &["home", "backup", "--out", all.to_str().unwrap()]);
    // The home's own backups directory is not inside the next backup.
    assert!(!archive_entries(&all).iter().any(|e| e.contains("backups")));
    let fresh = tmp.path().join("fresh");
    std::fs::create_dir_all(&fresh).unwrap();
    ok(
        &fresh,
        &["--tenant", "acme", "home", "restore", all.to_str().unwrap()],
    );
    assert!(fresh.join("data/acme/erp/ekos.toml").is_file());
    assert!(!fresh.join("data/default").exists());
    assert!(!fresh.join("control").exists());

    let none = refused(
        &fresh,
        &[
            "--tenant",
            "nobody",
            "home",
            "restore",
            all.to_str().unwrap(),
        ],
    );
    assert!(none.contains("no project of tenant nobody"), "{none}");
    let none = refused(&home, &["--tenant", "nobody", "home", "backup"]);
    assert!(none.contains("has no projects"), "{none}");
    let wrong = refused(&home, &["--project", "shop", "home", "backup"]);
    assert!(wrong.contains("--project does not"), "{wrong}");
}

#[test]
fn a_project_being_written_is_refused_and_no_archive_is_left() {
    use fs4::FileExt;

    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    project(&home, tmp.path(), "default", "shop", "orders_ibex");

    // What a running `ekos commit` holds.
    let lock_path = home.join("data/default/shop/state/ledger/facts/write.lock");
    let holder = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    FileExt::lock_exclusive(&holder).unwrap();

    let archive = tmp.path().join("b.tar.zst");
    let err = refused(
        &home,
        &["home", "backup", "--out", archive.to_str().unwrap()],
    );
    assert!(err.contains("default/shop is being written"), "{err}");
    assert!(!archive.exists());
    assert!(!archive.with_extension("partial").exists());

    FileExt::unlock(&holder).unwrap();
    drop(holder);
    ok(
        &home,
        &["home", "backup", "--out", archive.to_str().unwrap()],
    );
    assert!(archive.is_file());
}

/// An archive made by hand, with one entry of our choosing after a valid manifest.
fn crafted(
    path: &Path,
    add: impl FnOnce(&mut tar::Builder<zstd::Encoder<'static, std::fs::File>>),
) {
    let file = std::fs::File::create(path).unwrap();
    let mut tar = tar::Builder::new(zstd::Encoder::new(file, 3).unwrap());
    let manifest = serde_json::json!({
        "format": 1, "created_at": "2026-10-13T10:00:00Z", "ekos_version": "1.0.3",
        "tenants": {"default": ["shop"]}, "control": false
    })
    .to_string();
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o600);
    header.set_cksum();
    tar.append_data(&mut header, "ekos-backup.json", manifest.as_bytes())
        .unwrap();
    add(&mut tar);
    tar.into_inner().unwrap().finish().unwrap();
}

fn file_header(len: usize) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    header.set_size(len as u64);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::Regular);
    header
}

#[test]
fn an_archive_that_would_write_outside_the_home_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let outside = tmp.path().join("outside.txt");

    // `..` in a path. The tar crate refuses to *write* such a path through its safe API, so
    // the name is put into the header bytes directly, as a hostile archive would have it.
    let dotdot = tmp.path().join("dotdot.tar.zst");
    crafted(&dotdot, |tar| {
        let mut header = file_header(4);
        let name = b"data/default/../../../outside.txt";
        header.as_old_mut().name[..name.len()].copy_from_slice(name);
        header.set_cksum();
        tar.append(&header, &b"evil"[..]).unwrap();
    });
    let err = refused(&home, &["home", "restore", dotdot.to_str().unwrap()]);
    assert!(err.contains("would leave the directory"), "{err}");

    // A symlink out of the home, then a file "through" it.
    let link = tmp.path().join("link.tar.zst");
    crafted(&link, |tar| {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        tar.append_link(&mut header, "data/default/shop", tmp.path())
            .unwrap();
    });
    let err = refused(&home, &["home", "restore", link.to_str().unwrap()]);
    assert!(err.contains("neither a file nor a directory"), "{err}");

    // A file that is not part of a backup's layout.
    let stray = tmp.path().join("stray.tar.zst");
    crafted(&stray, |tar| {
        let mut header = file_header(1);
        tar.append_data(&mut header, "bin/ekos", &b"x"[..]).unwrap();
    });
    let err = refused(&home, &["home", "restore", stray.to_str().unwrap()]);
    assert!(err.contains("not part of a backup"), "{err}");

    // A manifest that names a project the archive does not hold.
    let hollow = tmp.path().join("hollow.tar.zst");
    crafted(&hollow, |_| {});
    let err = refused(&home, &["home", "restore", hollow.to_str().unwrap()]);
    assert!(err.contains("does not hold it"), "{err}");

    // Not an archive at all.
    let junk = tmp.path().join("junk.tar.zst");
    std::fs::write(&junk, "hello").unwrap();
    refused(&home, &["home", "restore", junk.to_str().unwrap()]);

    // After all of that: nothing outside, nothing inside, no staging directory.
    assert!(!outside.exists());
    let left: Vec<String> = std::fs::read_dir(&home)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(left.is_empty(), "{left:?}");
}
