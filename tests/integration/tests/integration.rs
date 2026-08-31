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
use ekos_runtime::Runtime;
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
}

async fn run_pipeline(config: &EkosConfig, dir: &Path) -> Result<()> {
    ekos::commands::build::run(config, dir).await?;
    ekos::commands::recover::run(config, dir, false).await?;
    ekos::commands::resolve::run(config, dir, false)?;
    ekos::commands::compile::run(config, dir).await?;
    ekos::commands::commit::run(config, dir, true).await?;
    Ok(())
}

fn table_count(runtime: &Runtime) -> Result<usize> {
    Ok(runtime
        .list_objects()?
        .iter()
        .filter(|o| o.kind.to_string() == "Table")
        .count())
}

#[tokio::test]
async fn ecommerce_pipeline_end_to_end() -> Result<()> {
    let dir = tempfile::tempdir()?;
    std::fs::create_dir_all(dir.path().join("schemas"))?;
    std::fs::copy(
        fixtures_dir().join("ecommerce.sql"),
        dir.path().join("schemas/ecommerce.sql"),
    )?;
    // sample_project/ gives FileObserver something else to observe alongside the schema.
    copy_dir(
        &fixtures_dir().join("sample_project"),
        &dir.path().join("sample_project"),
    )?;
    copy_dir(
        &fixtures_dir().join("sample_docs"),
        &dir.path().join("sample_docs"),
    )?;

    let config = EkosConfig::default();
    run_pipeline(&config, dir.path()).await?;

    let store = ekos::commands::store::open_store(&config, dir.path())?;
    let runtime = Runtime::over(&*store);

    assert_eq!(
        table_count(&runtime)?,
        6,
        "ecommerce schema has exactly 6 tables"
    );

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
    std::fs::copy(
        fixtures_dir().join("northwind.sql"),
        dir.path().join("schemas/northwind.sql"),
    )?;

    let config = EkosConfig::default();
    run_pipeline(&config, dir.path()).await?;

    let store = ekos::commands::store::open_store(&config, dir.path())?;
    let runtime = Runtime::over(&*store);

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
        .args([
            "clone",
            &bundle.to_string_lossy(),
            &dir.path().to_string_lossy(),
        ])
        .status()?;
    assert!(
        status.success(),
        "git clone of the vendored bundle must succeed"
    );

    let config = EkosConfig::default();
    ekos::commands::build::run(&config, dir.path()).await?;
    ekos::commands::recover::run(&config, dir.path(), false).await?;
    ekos::commands::compile::run(&config, dir.path()).await?;
    ekos::commands::commit::run(&config, dir.path(), true).await?;

    let store = ekos::commands::store::open_store(&config, dir.path())?;
    // The real Odoo `utm` module's initial commit alone touches 28 files together —
    // real coupling, not synthetic. Assert some relationship emerged from real history
    // rather than pinning an exact count (real commit history isn't a fixed number we
    // control).
    assert!(
        store.relationship_count()? > 0,
        "GitAnalyzerPass should find at least one CoupledWith relationship in real Odoo history"
    );
    assert!(
        store.object_count()? > 0,
        "at least the observed files/commits should be objects"
    );

    Ok(())
}

/// RFC 0113 — a `compile-worker` (Service A) runs the real pipeline under a coordinator
/// write-lease against a partitioned workspace, then registers its partitions and commits the
/// manifest generation.
#[tokio::test(flavor = "multi_thread")]
async fn compile_worker_runs_the_real_pipeline_under_a_lease() -> Result<()> {
    let dir = tempfile::tempdir()?;
    std::fs::create_dir_all(dir.path().join("schemas"))?;
    std::fs::copy(
        fixtures_dir().join("ecommerce.sql"),
        dir.path().join("schemas/ecommerce.sql"),
    )?;
    std::fs::write(
        dir.path().join("ekos.toml"),
        "[storage.partition]\ndimension = \"entity-kind\"\ntime-bucket = \"monthly\"\n",
    )?;

    let (coord_addr, _coord) = ekos_cluster::spawn_ephemeral("127.0.0.1:0", None).await.unwrap();
    let coord_s = coord_addr.to_string();

    ekos::commands::cluster::compile_worker_run(&coord_s, "main", dir.path(), false).await?;

    // The coordinator now knows this shard's partitions and a non-zero generation watermark.
    let client = ekos_cluster::CoordinatorClient::connect(&coord_s).await.unwrap();
    let catalog = client.catalog(None).await.unwrap();
    assert!(
        catalog.iter().any(|m| m.id.starts_with("Table/")),
        "a Table/<bucket> partition must be registered: {catalog:?}"
    );
    // The watermark is tracked per lease/shard name ("main", the scheduling unit `run_shard` was
    // called with), not per physical storage partition id — a pre-existing mismatch this
    // assertion used to paper over with an `||` against the (buggy) shard-name entity-index entry
    // removed below.
    assert!(client.watermark("main").await.unwrap() > 0);

    // And the workspace really was compiled — the partitioned store has the ecommerce tables.
    let config = EkosConfig::from_file_or_default(&dir.path().join("ekos.toml"));
    let store = ekos::commands::store::open_store(&config, dir.path())?;
    let runtime = Runtime::over(&*store);
    assert_eq!(table_count(&runtime)?, 6, "ecommerce schema has 6 tables");

    // RFC 0113 v1.1: compile-worker must populate the coordinator's `entity_id → partitions`
    // pruning index with each object's own real id (not a shard-name placeholder) — this is what
    // lets `DistributedLedger`'s id-scoped reads prune to the few partitions that actually hold an
    // id instead of fanning to every partition of the class.
    let some_table = runtime
        .list_objects()?
        .into_iter()
        .find(|o| o.kind.to_string() == "Table")
        .expect("at least one Table object");
    let indexed = client
        .partitions_for_entity(&some_table.id.to_string())
        .await
        .unwrap();
    assert!(
        indexed.iter().any(|p| p.starts_with("Table/")),
        "the object's own id must be indexed against its real Table/<bucket> partition: {indexed:?}"
    );

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
