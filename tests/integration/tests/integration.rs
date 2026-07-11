//! End-to-end integration tests running the full EKOS pipeline (build → recover →
//! resolve → compile → commit → query) against near-real, open-source fixture data.
//! No external services — everything is either bundled in `tests/fixtures/` or, for
//! the git fixture, materialized offline from a vendored `git bundle`.
//!
//! Scope note: this covers one comprehensive end-to-end test per fixture dataset
//! through the pipeline phases named in TODO.md's "Integration test harness" item.
//! It does not attempt one test per every phase 0–14 validation criterion — that is a
//! separately-scoped effort this pass does not claim to satisfy.

use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_ledger::Ledger;
use ekos_runtime::Runtime;
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
}

async fn run_pipeline(config: &EkosConfig, dir: &Path) -> Result<()> {
    ekos::commands::build::run(config, dir).await?;
    ekos::commands::recover::run(config, dir, false).await?;
    ekos::commands::resolve::run(config, dir)?;
    ekos::commands::compile::run(config, dir).await?;
    ekos::commands::commit::run(config, dir)?;
    Ok(())
}

fn table_count(runtime: &Runtime) -> Result<usize> {
    Ok(runtime.list_objects()?.iter().filter(|o| o.kind.to_string() == "Table").count())
}

#[tokio::test]
async fn ecommerce_pipeline_end_to_end() -> Result<()> {
    let dir = tempfile::tempdir()?;
    std::fs::create_dir_all(dir.path().join("schemas"))?;
    std::fs::copy(fixtures_dir().join("ecommerce.sql"), dir.path().join("schemas/ecommerce.sql"))?;
    // sample_project/ gives FileObserver something else to observe alongside the schema.
    copy_dir(&fixtures_dir().join("sample_project"), &dir.path().join("sample_project"))?;
    copy_dir(&fixtures_dir().join("sample_docs"), &dir.path().join("sample_docs"))?;

    let config = EkosConfig::default();
    run_pipeline(&config, dir.path()).await?;

    let ledger = Ledger::open(&config.ledger_path(dir.path()))?;
    let runtime = Runtime::new(&ledger);

    assert_eq!(table_count(&runtime)?, 6, "ecommerce schema has exactly 6 tables");

    let (customers_id, _) = runtime
        .find_objects("customers")?
        .into_iter()
        .next()
        .expect("customers table must be findable via FTS");
    assert!(runtime.load_object(&customers_id)?.is_some());

    let neighborhood = runtime.load_neighborhood(&customers_id, 1)?;
    assert!(
        !neighborhood.relationships.is_empty(),
        "customers should have at least one FK neighbor (orders → customers)"
    );

    Ok(())
}

#[tokio::test]
async fn northwind_pipeline_end_to_end() -> Result<()> {
    let dir = tempfile::tempdir()?;
    std::fs::create_dir_all(dir.path().join("schemas"))?;
    std::fs::copy(fixtures_dir().join("northwind.sql"), dir.path().join("schemas/northwind.sql"))?;

    let config = EkosConfig::default();
    run_pipeline(&config, dir.path()).await?;

    let ledger = Ledger::open(&config.ledger_path(dir.path()))?;
    let runtime = Runtime::new(&ledger);

    // Northwind is externally sourced — assert a realistic floor, not an exact count
    // pinned to this fixture's incidental details.
    assert!(
        table_count(&runtime)? >= 13,
        "northwind schema has 13 real tables; expected at least that many Table objects"
    );

    let (orders_id, _) = runtime
        .find_objects("orders")?
        .into_iter()
        .next()
        .expect("Orders table must be findable via FTS");
    let neighborhood = runtime.load_neighborhood(&orders_id, 1)?;
    assert!(
        neighborhood.relationships.len() >= 3,
        "Orders has real FKs to Customers, Employees, and Shippers"
    );

    Ok(())
}

#[tokio::test]
async fn odoo_git_fixture_pipeline_end_to_end() -> Result<()> {
    // Materialize a real working repo from the vendored bundle — no network involved,
    // this is the whole point of vendoring it as a bundle (see git_fixture/NOTICE.md).
    let dir = tempfile::tempdir()?;
    let bundle = fixtures_dir().join("git_fixture/odoo_utm.bundle");
    let status = std::process::Command::new("git")
        .args(["clone", &bundle.to_string_lossy(), &dir.path().to_string_lossy()])
        .status()?;
    assert!(status.success(), "git clone of the vendored bundle must succeed");

    let config = EkosConfig::default();
    ekos::commands::build::run(&config, dir.path()).await?;
    ekos::commands::recover::run(&config, dir.path(), false).await?;
    ekos::commands::compile::run(&config, dir.path()).await?;
    ekos::commands::commit::run(&config, dir.path())?;

    let ledger = Ledger::open(&config.ledger_path(dir.path()))?;
    // The real Odoo `utm` module's initial commit alone touches 28 files together —
    // real coupling, not synthetic. Assert some relationship emerged from real history
    // rather than pinning an exact count (real commit history isn't a fixed number we
    // control).
    assert!(
        ledger.relationship_count()? > 0,
        "GitAnalyzerPass should find at least one CoupledWith relationship in real Odoo history"
    );
    assert!(ledger.object_count()? > 0, "at least the observed files/commits should be objects");

    Ok(())
}

/// Recursively copy a fixture directory into a tempdir workspace.
fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}
