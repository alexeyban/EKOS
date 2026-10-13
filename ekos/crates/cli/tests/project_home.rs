//! RFC 0178 end to end, through the real binary: a project in the EKOS home compiles sources
//! that stay untouched, and two tenants' projects of the same name do not see each other.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn ekos(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ekos"))
        .args(args)
        .current_dir(cwd)
        .env("EKOS_HOME", home)
        .env_remove("EKOS_PROJECT")
        .env_remove("EKOS_TENANT")
        .env_remove("EKOS_CONFIG")
        .env_remove("EKOS_WORKSPACE")
        .output()
        .expect("running the ekos binary")
}

fn ok(home: &Path, cwd: &Path, args: &[&str]) -> String {
    let out = ekos(home, cwd, args);
    assert!(
        out.status.success(),
        "ekos {args:?} failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn refused(home: &Path, cwd: &Path, args: &[&str]) -> String {
    let out = ekos(home, cwd, args);
    assert!(!out.status.success(), "ekos {args:?} should have failed");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Every file under `dir` with its bytes: the tree's whole content.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.insert(p.clone(), std::fs::read(&p).unwrap());
            }
        }
    }
    out
}

fn sources(dir: &Path, marker: &str) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        format!("/// {marker}\npub fn {marker}() -> u32 {{ 1 }}\n"),
    )
    .unwrap();
    std::fs::write(dir.join("README.md"), format!("# {marker}\n")).unwrap();
}

fn pipeline(home: &Path, cwd: &Path, select: &[&str]) {
    for stage in [
        vec!["build"],
        vec!["recover"],
        vec!["resolve"],
        vec!["compile"],
        vec!["commit", "--yes"],
    ] {
        let mut args: Vec<&str> = select.to_vec();
        args.extend(stage);
        ok(home, cwd, &args);
    }
}

#[test]
fn a_project_compiles_sources_it_never_writes_to() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let elsewhere = tmp.path().join("elsewhere");
    let src = tmp.path().join("shop-src");
    std::fs::create_dir_all(&elsewhere).unwrap();
    sources(&src, "shopmarker");
    let before = snapshot(&src);

    ok(
        &home,
        &elsewhere,
        &["project", "add", "shop", "--source", src.to_str().unwrap()],
    );
    // Run from a directory that is neither the sources nor the home.
    pipeline(&home, &elsewhere, &["--project", "shop"]);

    assert_eq!(snapshot(&src), before, "the source tree changed");
    assert!(
        snapshot(&elsewhere).is_empty(),
        "the caller's directory was written to"
    );
    let state = home.join("data/default/shop/state");
    assert!(state.join("ledger").is_dir() && state.join("artifacts").is_dir());

    let found = ok(
        &home,
        &elsewhere,
        &["--project", "shop", "query", "find", "shopmarker"],
    );
    assert!(found.contains("result(s) for"), "{found}");
    let doctor = ok(&home, &elsewhere, &["--project", "shop", "doctor"]);
    assert!(doctor.contains("default/shop"), "{doctor}");

    let listed = ok(&home, &elsewhere, &["project", "list"]);
    assert!(listed.contains("default/shop"), "{listed}");
}

#[test]
fn two_tenants_with_a_project_of_the_same_name_have_separate_ledgers() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    for (tenant, marker) in [("acme", "acmemarker"), ("globex", "globexmarker")] {
        let src = tmp.path().join(format!("{tenant}-src"));
        sources(&src, marker);
        ok(
            &home,
            &cwd,
            &[
                "project",
                "add",
                "shop",
                "--tenant",
                tenant,
                "--source",
                src.to_str().unwrap(),
            ],
        );
        pipeline(&home, &cwd, &["--tenant", tenant, "--project", "shop"]);
    }
    let find = |tenant: &str, text: &str| {
        ok(
            &home,
            &cwd,
            &[
                "--tenant",
                tenant,
                "--project",
                "shop",
                "query",
                "find",
                text,
            ],
        )
    };
    // The command echoes the query, so a hit is recognised by its result line.
    assert!(find("acme", "acmemarker").contains("result(s) for"));
    assert!(find("globex", "globexmarker").contains("result(s) for"));
    assert!(find("acme", "globexmarker").contains("No objects found"));
    assert!(find("globex", "acmemarker").contains("No objects found"));

    // The same selection through the environment.
    let out = Command::new(env!("CARGO_BIN_EXE_ekos"))
        .args(["query", "find", "acmemarker"])
        .current_dir(&cwd)
        .env("EKOS_HOME", &home)
        .env("EKOS_PROJECT", "shop")
        .env("EKOS_TENANT", "acme")
        .env_remove("EKOS_CONFIG")
        .env_remove("EKOS_WORKSPACE")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("result(s) for"));
}

#[test]
fn what_cannot_be_combined_with_a_project_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let src = tmp.path().join("src");
    sources(&src, "m");
    let cwd = tmp.path();
    ok(
        &home,
        cwd,
        &["project", "add", "shop", "--source", src.to_str().unwrap()],
    );
    let before = snapshot(&src);

    assert!(refused(&home, cwd, &["--project", "nope", "build"]).contains("ekos project add nope"));
    assert!(refused(&home, cwd, &["--project", "../shop", "build"]).contains("invalid project"));
    assert!(refused(&home, cwd, &["--tenant", "acme", "build"]).contains("--project"));
    assert!(
        refused(
            &home,
            cwd,
            &["--project", "shop", "--config", "x.toml", "build"]
        )
        .contains("--config")
    );
    assert!(refused(&home, cwd, &["--project", "shop", "init"]).contains("project add"));
    assert!(
        refused(
            &home,
            cwd,
            &["--project", "shop", "mcp", "serve", "--workspace", "."]
        )
        .contains("--workspace")
    );
    assert!(refused(&home, cwd, &["project", "remove", "shop", "--purge"]).contains("--yes"));
    assert_eq!(snapshot(&src), before);

    ok(&home, cwd, &["project", "remove", "shop"]);
    assert!(!home.join("data/default/shop").exists());
    assert_eq!(
        std::fs::read_dir(home.join("trash/default"))
            .unwrap()
            .count(),
        1
    );
}
