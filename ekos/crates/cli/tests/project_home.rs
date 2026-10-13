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

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git is needed for this test");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn repo(dir: &Path, marker: &str) {
    sources(dir, marker);
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", &format!("add {marker}")]);
}

/// Rows of `ekos ekl "<query>" --json`.
fn ekl(home: &Path, cwd: &Path, query: &str) -> Vec<serde_json::Value> {
    let out = ok(home, cwd, &["--project", "shop", "ekl", query, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
    v.as_array()
        .cloned()
        .expect("ekl --json prints an array of rows")
}

/// RFC 0178 Part 2: two repositories and a document folder compiled into one ledger.
#[test]
fn a_project_of_several_named_sources_keeps_them_apart_and_says_where_each_object_is_from() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let (api, web, docs) = (
        tmp.path().join("api-repo"),
        tmp.path().join("web-repo"),
        tmp.path().join("handbook"),
    );
    repo(&api, "apimarker");
    repo(&web, "webmarker");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("setup.md"),
        "# Setup\n\nHow to run the docsmarker service.\n",
    )
    .unwrap();
    let before = (snapshot(&api), snapshot(&web), snapshot(&docs));

    let spec = |name: &str, dir: &Path| format!("{name}={}", dir.display());
    ok(
        &home,
        &cwd,
        &[
            "project",
            "add",
            "shop",
            "--source",
            &spec("api", &api),
            "--source",
            &spec("web", &web),
            "--source",
            &spec("docs", &docs),
        ],
    );
    let listed = ok(&home, &cwd, &["--project", "shop", "source", "list"]);
    assert!(
        listed.contains("api  git") && listed.contains("docs  dir"),
        "{listed}"
    );

    pipeline(&home, &cwd, &["--project", "shop"]);
    assert_eq!((snapshot(&api), snapshot(&web), snapshot(&docs)), before);

    // Both repositories hold `src/lib.rs`; they are two objects, one per source.
    let files = ekl(
        &home,
        &cwd,
        "FIND Object WHERE kind = 'File' AND name = 'src/lib.rs' RETURN id, name, source",
    );
    let mut from: Vec<&str> = files.iter().filter_map(|r| r["source"].as_str()).collect();
    from.sort();
    assert_eq!(from, ["api", "web"], "{files:?}");
    assert_ne!(files[0]["id"], files[1]["id"]);

    // Every source has objects, and the source can be asked for.
    for name in ["api", "web", "docs"] {
        let rows = ekl(
            &home,
            &cwd,
            &format!("FIND Object WHERE source = '{name}' RETURN name, kind"),
        );
        assert!(!rows.is_empty(), "nothing attributed to {name}");
    }
    // Commit history was observed in both repositories, not just the first.
    for message in ["add apimarker", "add webmarker"] {
        let found = ok(
            &home,
            &cwd,
            &["--project", "shop", "query", "find", message],
        );
        assert!(found.contains("result(s) for"), "{message}: {found}");
    }

    // What a commit could not attribute is reported by kind, so a gap is visible.
    let summary = ok(&home, &cwd, &["--project", "shop", "commit", "--yes"]);
    assert!(summary.contains("Objects with a source:"), "{summary}");
    let none = summary
        .lines()
        .find(|l| l.contains("Objects with none:"))
        .unwrap_or("");
    // A commit author is not a file of any source. Any other kind here is a regression.
    assert!(
        none.is_empty() || none.trim().ends_with("(Person 1)"),
        "{summary}"
    );

    // One source rebuilt alone leaves the others' recorded files in place.
    std::fs::write(docs.join("more.md"), "# More\n").unwrap();
    ok(
        &home,
        &cwd,
        &["--project", "shop", "build", "--source", "docs"],
    );
    let manifest =
        std::fs::read_to_string(home.join("data/default/shop/state/source-manifest.json")).unwrap();
    for key in ["docs/more.md", "api/src/lib.rs", "web/src/lib.rs"] {
        assert!(manifest.contains(key), "{key} missing from the manifest");
    }
    assert!(
        refused(
            &home,
            &cwd,
            &["--project", "shop", "build", "--source", "nope"]
        )
        .contains("not a source")
    );
}
