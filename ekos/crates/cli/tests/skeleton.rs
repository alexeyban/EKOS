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
    let ledger = ekos_ledger::Ledger::open(&config.ledger_path(dir)).unwrap();
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

    let ledger = ekos_ledger::Ledger::open(&config.ledger_path(dir)).unwrap();
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

    let ledger = ekos_ledger::Ledger::open(&config.ledger_path(dir)).unwrap();
    let count_after_first = ledger.object_count().unwrap();

    // Second build — should not duplicate entries
    ekos::commands::build::run(&config, dir).await.unwrap();
    let count_after_second = ledger.object_count().unwrap();

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
