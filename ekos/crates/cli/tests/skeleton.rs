//! End-to-end skeleton test: init → build → query
//!
//! This test stays green through Phases 2–10 as each skeleton stub is replaced
//! by its real implementation. It is the canary proving the pipeline never
//! breaks while individual layers are widened.

use std::path::Path;
use tempfile::TempDir;

fn setup_workspace(dir: &Path) {
    // Create a small fixture project tree
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.rs"), b"fn main() {}").unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        b"pub fn add(a: i32, b: i32) -> i32 { a + b }",
    )
    .unwrap();
    std::fs::write(dir.join("README.md"), b"# Sample Project").unwrap();

    // Write a minimal ekos.toml
    std::fs::write(
        dir.join("ekos.toml"),
        b"[workspace]\nroot = \".\"\n\n[observe]\npaths = [\"src\"]\nignore-patterns = [\".ekos\"]\n",
    )
    .unwrap();
}

fn load_config(dir: &Path) -> ekos_compiler_core::EkosConfig {
    ekos_compiler_core::EkosConfig::from_file(&dir.join("ekos.toml")).unwrap()
}

#[test]
fn init_creates_ekos_directory() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_workspace(dir);

    let config = load_config(dir);
    ekos::commands::init::run(&config, dir).unwrap();

    assert!(dir.join(".ekos").exists(), ".ekos/ was not created");
    assert!(dir.join(".ekos/artifacts").exists(), "artifacts/ missing");
    assert!(dir.join(".ekos/ledger").exists(), "ledger/ missing");
}

#[tokio::test]
async fn build_observes_files_and_writes_ledger() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_workspace(dir);

    let config = load_config(dir);
    ekos::commands::init::run(&config, dir).unwrap();
    ekos::commands::build::run(&config, dir).await.unwrap();

    // Ledger must exist and have entries
    let ledger = ekos::commands::store::open_store(&config, dir).unwrap();
    let count = ledger.object_count().unwrap();
    assert!(count >= 2, "expected at least 2 file objects, got {count}");
}

#[tokio::test]
async fn query_object_returns_known_file() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_workspace(dir);

    let config = load_config(dir);
    ekos::commands::init::run(&config, dir).unwrap();
    ekos::commands::build::run(&config, dir).await.unwrap();

    let ledger = ekos::commands::store::open_store(&config, dir).unwrap();
    let results = ledger.find_objects("main*").unwrap();
    assert!(!results.is_empty(), "expected to find main.rs object");

    let (id, name) = &results[0];
    assert!(name.contains("main.rs"));

    let obj = ledger.get_object(id).unwrap().unwrap();
    assert_eq!(obj.name, results[0].1);
}

#[tokio::test]
async fn build_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_workspace(dir);

    let config = load_config(dir);
    ekos::commands::init::run(&config, dir).unwrap();
    ekos::commands::build::run(&config, dir).await.unwrap();

    // Each count is read through its own store handle, dropped before the next `build::run` —
    // the fact engine's tantivy index takes an exclusive writer lock per open handle (real
    // single-writer enforcement, not a bug), so holding one open across a second `build::run`
    // call (which opens its own handle internally) would deadlock. No real CLI invocation holds
    // a handle open across separate commands either — each process opens fresh and exits.
    let count_after_first = {
        let ledger = ekos::commands::store::open_store(&config, dir).unwrap();
        ledger.object_count().unwrap()
    };

    // Second build — should not duplicate entries
    ekos::commands::build::run(&config, dir).await.unwrap();
    let count_after_second = {
        let ledger = ekos::commands::store::open_store(&config, dir).unwrap();
        ledger.object_count().unwrap()
    };

    assert_eq!(
        count_after_first, count_after_second,
        "second build added duplicate entries"
    );
}

#[test]
fn clean_removes_artifacts_not_ledger() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_workspace(dir);

    let config = load_config(dir);
    ekos::commands::init::run(&config, dir).unwrap();

    // Put a dummy artifact file in the cache
    let artifact_dir = config.artifact_dir(dir);
    std::fs::write(artifact_dir.join("dummy.json"), b"{}").unwrap();
    assert!(artifact_dir.join("dummy.json").exists());

    ekos::commands::clean::run(&config, dir).unwrap();

    assert!(
        !artifact_dir.join("dummy.json").exists(),
        "artifact not cleaned"
    );
    assert!(
        dir.join(".ekos/ledger").exists(),
        "ledger should survive clean"
    );
}

// ── RFC 0043 — global secrets/PII redaction ─────────────────────────────────

#[tokio::test]
async fn build_redacts_a_fake_secret_from_the_observed_excerpt() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src/config.rs"),
        b"const AWS_KEY: &str = \"AKIAABCDEFGHIJKLMNOP\";\nfn main() {}",
    )
    .unwrap();
    std::fs::write(
        dir.join("ekos.toml"),
        b"[workspace]\nroot = \".\"\n\n[observe]\npaths = [\"src\"]\nignore-patterns = [\".ekos\"]\n",
    )
    .unwrap();

    let config = load_config(dir);
    ekos::commands::init::run(&config, dir).unwrap();
    ekos::commands::build::run(&config, dir).await.unwrap();

    let ledger = ekos::commands::store::open_store(&config, dir).unwrap();
    let results = ledger.find_objects("config*").unwrap();
    assert!(!results.is_empty(), "expected to find config.rs object");
    let obj = ledger.get_object(&results[0].0).unwrap().unwrap();

    let excerpt = obj.properties.get("excerpt").and_then(|v| v.as_str());
    if let Some(excerpt) = excerpt {
        assert!(
            !excerpt.contains("AKIAABCDEFGHIJKLMNOP"),
            "fake AWS key leaked into the ledger unredacted: {excerpt}"
        );
        assert!(
            excerpt.contains("[REDACTED:aws-access-key-id]"),
            "expected a redaction placeholder in the excerpt: {excerpt}"
        );
    }
}

#[tokio::test]
async fn build_excludes_dotenv_files_entirely() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.rs"), b"fn main() {}").unwrap();
    std::fs::write(dir.join("src/.env"), b"DATABASE_PASSWORD=hunter2fake\n").unwrap();
    std::fs::write(
        dir.join("ekos.toml"),
        b"[workspace]\nroot = \".\"\n\n[observe]\npaths = [\"src\"]\nignore-patterns = [\".ekos\"]\n",
    )
    .unwrap();

    let config = load_config(dir);
    ekos::commands::init::run(&config, dir).unwrap();
    ekos::commands::build::run(&config, dir).await.unwrap();

    let ledger = ekos::commands::store::open_store(&config, dir).unwrap();
    let results = ledger.find_objects(".env").unwrap();
    assert!(
        results.is_empty(),
        ".env file must never be observed into the ledger, found: {results:?}"
    );

    // The sibling main.rs must still be observed normally — exclusion is per-file, not
    // per-directory.
    let main_results = ledger.find_objects("main*").unwrap();
    assert!(
        !main_results.is_empty(),
        "sibling main.rs must still be observed"
    );
}

/// `recover.rs`'s CI/CD workflow file collection (RFC 0042) reads `.github/workflows/*.yml`
/// directly, bypassing `build.rs`'s artifact-store redaction pass entirely — this exercises that
/// separate direct-read call site's own redaction wiring (RFC 0043).
#[tokio::test]
async fn recover_redacts_a_fake_secret_from_a_cicd_workflow_step() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join(".github/workflows")).unwrap();
    std::fs::write(
        dir.join(".github/workflows/ci.yml"),
        b"name: CI\non: push\njobs:\n  deploy:\n    steps:\n      - run: |\n          curl -H AKIAABCDEFGHIJKLMNOP https://example.com\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("ekos.toml"),
        b"[workspace]\nroot = \".\"\n\n[observe]\npaths = [\".\"]\nignore-patterns = [\".ekos\", \".git\"]\n",
    )
    .unwrap();

    let config = load_config(dir);
    ekos::commands::init::run(&config, dir).unwrap();
    ekos::commands::build::run(&config, dir).await.unwrap();
    ekos::commands::recover::run(&config, dir, false)
        .await
        .unwrap();
    ekos::commands::compile::run(&config, dir).await.unwrap();
    ekos::commands::commit::run(&config, dir, true)
        .await
        .unwrap();

    let ledger = ekos::commands::store::open_store(&config, dir).unwrap();
    let results = ledger.find_objects("CI").unwrap();
    assert!(
        !results.is_empty(),
        "expected to find the CI Pipeline object"
    );
    let obj = ledger.get_object(&results[0].0).unwrap().unwrap();
    let jobs = obj.properties.get("jobs").unwrap().to_string();

    assert!(
        !jobs.contains("AKIAABCDEFGHIJKLMNOP"),
        "fake secret leaked into the committed Pipeline object's jobs property: {jobs}"
    );
    assert!(
        jobs.contains("[REDACTED:aws-access-key-id]"),
        "expected a redaction placeholder in the jobs property: {jobs}"
    );
}

// ── RFC 0044 Phase 1 — project-qualified object identity ───────────────────

/// Regression test for a real bug found designing multi-project/estate-scale support: two
/// unrelated projects that each happen to have a same-relatively-named file used to silently
/// collide into one merged `KirObject`, since `build.rs` hashed the bare within-project relative
/// path with no project component. `[observe] paths` with more than one entry must now keep them
/// distinct.
#[tokio::test]
async fn build_keeps_same_named_files_in_different_projects_distinct() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join("proj-a/src")).unwrap();
    std::fs::create_dir_all(dir.join("proj-b/src")).unwrap();
    std::fs::write(
        dir.join("proj-a/src/main.rs"),
        b"fn main() { /* project A */ }",
    )
    .unwrap();
    std::fs::write(
        dir.join("proj-b/src/main.rs"),
        b"fn main() { /* project B */ }",
    )
    .unwrap();
    std::fs::write(
        dir.join("ekos.toml"),
        b"[workspace]\nroot = \".\"\n\n[observe]\npaths = [\"proj-a\", \"proj-b\"]\nignore-patterns = [\".ekos\"]\n",
    )
    .unwrap();

    let config = load_config(dir);
    ekos::commands::init::run(&config, dir).unwrap();
    ekos::commands::build::run(&config, dir).await.unwrap();

    let ledger = ekos::commands::store::open_store(&config, dir).unwrap();
    let results = ledger.find_objects("main*").unwrap();
    assert_eq!(
        results.len(),
        2,
        "expected two distinct main.rs objects (one per project), got: {results:?}"
    );

    let mut projects: Vec<String> = results
        .iter()
        .map(|(id, _)| {
            let obj = ledger.get_object(id).unwrap().unwrap();
            obj.properties
                .get("project")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect();
    projects.sort();
    assert_eq!(
        projects,
        vec!["proj-a".to_string(), "proj-b".to_string()],
        "each object's `project` property must identify which observe path it came from"
    );
}

/// The common case (`paths = ["."]`, the config every other test in this file uses) must produce
/// byte-identical ids to before this fix — no `project` property, no migration for existing
/// single-project ledgers.
#[tokio::test]
async fn build_single_project_workspace_has_no_project_property() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_workspace(dir);

    let config = load_config(dir);
    ekos::commands::init::run(&config, dir).unwrap();
    ekos::commands::build::run(&config, dir).await.unwrap();

    let ledger = ekos::commands::store::open_store(&config, dir).unwrap();
    let results = ledger.find_objects("main*").unwrap();
    assert!(!results.is_empty(), "expected to find main.rs object");
    let obj = ledger.get_object(&results[0].0).unwrap().unwrap();
    assert!(
        obj.properties.get("project").is_none(),
        "single-path workspaces must not gain a `project` property"
    );
}
