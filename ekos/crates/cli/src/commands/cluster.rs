//! `ekos coordinator` and `ekos compile-worker` — RFC 0113 B3 (Distributed mode).
//!
//! `coordinator serve` runs the full metadata service (partition catalog, write leases with
//! fencing tokens, per-partition tx watermarks) over newline-delimited JSON-RPC on TCP. It holds
//! no partition data — object storage (RFC 0113 B2) does — and persists its own small state to a
//! single JSON file.
//!
//! `compile-worker run` is Service A: under a coordinator write-lease (heartbeated,
//! fencing-tokened) it runs the **real** `build → recover → resolve → compile → commit` pipeline
//! against the local partitioned workspace, then registers every partition it wrote with the
//! coordinator and commits the new manifest generation — fenced, so a stale ex-lease-holder's
//! late commit is rejected and the next worker resumes from the recorded watermark.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Duration;
use ekos_cluster::{
    CompileWorker, Coordinator, CoordinatorClient, PartitionLocation, WorkerError, serve,
};
use ekos_compiler_core::EkosConfig;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::commands::store;

/// `ekos coordinator serve --listen <addr> [--state <path>] [--ttl-seconds N]`.
pub async fn serve_coordinator(
    listen: &str,
    state: Option<&std::path::Path>,
    ttl_seconds: Option<i64>,
) -> Result<()> {
    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("binding coordinator on {listen}"))?;
    let bound = listener.local_addr()?;

    let mut coordinator = match state {
        Some(p) => {
            Coordinator::open(p).with_context(|| format!("opening coordinator state {p:?}"))?
        }
        None => Coordinator::ephemeral(),
    };
    if let Some(secs) = ttl_seconds {
        coordinator = coordinator.with_ttl(Duration::seconds(secs));
    }

    match state {
        Some(p) => tracing::info!(%bound, state = %p.display(), "coordinator serving"),
        None => tracing::info!(%bound, "coordinator serving (ephemeral, no persistence)"),
    }
    println!("coordinator listening on {bound}");

    serve(Arc::new(Mutex::new(coordinator)), listener).await;
    Ok(())
}

/// `ekos cluster status --coordinator <addr>` — dump the catalog + watermarks.
pub async fn status(coordinator: &str) -> Result<()> {
    let client = CoordinatorClient::connect(coordinator)
        .await
        .with_context(|| format!("connecting to coordinator at {coordinator}"))?;
    let catalog = client.catalog(None).await?;
    if catalog.is_empty() {
        println!("no partitions registered");
        return Ok(());
    }
    println!(
        "{:<40}  {:>10}  {:>6}  location",
        "partition", "watermark", "cold"
    );
    for meta in &catalog {
        let wm = client.watermark(&meta.id).await?;
        let loc = match &meta.location {
            PartitionLocation::Local { root } => format!("local:{root}"),
            PartitionLocation::ObjectStore { url, prefix } => format!("{url}/{prefix}"),
        };
        println!("{:<40}  {:>10}  {:>6}  {}", meta.id, wm, meta.cold, loc);
    }
    Ok(())
}

/// `ekos compile-worker run --coordinator <addr> --shard <name> [--workspace <dir>] [--parallel]`
/// — Service A. Acquires the `shard` write-lease (heartbeated), runs the real
/// `build → recover → resolve → compile → commit` pipeline against the local partitioned
/// workspace, registers every partition it wrote with the coordinator, and commits the new
/// manifest generation (the store's monotonic entry count) with its fencing token. If the lease
/// is lost mid-run, the fenced commit fails and the run reports `LostLease` — the pipeline's own
/// per-`FactLedger` `write.lock` (RFC 0104) is the second safety net against a concurrent writer.
///
/// The workspace must be a **Local** partitioned workspace (`[storage.partition]`), not
/// `[storage.distributed]` (that config is the read-only gateway). Its partition roots must live
/// on storage the query workers can also reach (a shared filesystem for v1, until
/// `PartitionedLedger` writes through `SegmentBackend` directly).
pub async fn compile_worker_run(
    coordinator: &str,
    shard: &str,
    workspace: &Path,
    parallel_recover: bool,
) -> Result<()> {
    let config_path = workspace.join("ekos.toml");
    let config = EkosConfig::from_file_or_default(&config_path);
    if config.storage.distributed.is_enabled() {
        anyhow::bail!(
            "this workspace has [storage.distributed] set — that is the read-only gateway config. \
             A compile worker writes locally; point --workspace at a Local partitioned workspace \
             whose partition roots the query workers can also reach."
        );
    }
    if !store::uses_partitioned(&config, workspace) {
        anyhow::bail!(
            "compile-worker needs a partitioned workspace ([storage.partition] with a \
             partitioned/ store); {} is not one",
            workspace.display()
        );
    }

    let client = Arc::new(
        CoordinatorClient::connect(coordinator)
            .await
            .with_context(|| format!("connecting to coordinator at {coordinator}"))?,
    );
    let id = format!("compile-worker-{}", std::process::id());
    let worker = CompileWorker::new(client.clone(), id.clone());

    println!("{id}: acquiring lease on shard '{shard}'");
    let ws = workspace.to_path_buf();
    let cfg = config.clone();
    let client_w = client.clone();
    let shard_w = shard.to_string();

    worker
        .run_shard(shard, move |guard| async move {
            println!(
                "  lease held (token {}); running build → recover → resolve → compile → commit",
                guard.token()
            );

            // The whole pipeline runs on a blocking thread with its own runtime, so the worker's
            // executor stays free to heartbeat the lease through a multi-minute compile.
            let ws2 = ws.clone();
            let cfg2 = cfg.clone();
            tokio::task::spawn_blocking(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())?
                    .block_on(run_pipeline(&cfg2, &ws2, parallel_recover))
                    .map_err(|e| format!("{e:#}"))
            })
            .await
            .map_err(|e| WorkerError::Work(format!("pipeline task panicked: {e}")))?
            .map_err(WorkerError::Work)?;

            // Push each partition's search index to its backend (no-op for a local backend) so a
            // query worker can serve `find_objects` for object-storage partitions.
            {
                let cfg3 = cfg.clone();
                let ws3 = ws.clone();
                tokio::task::spawn_blocking(move || {
                    store::build_partitioned(&cfg3, &ws3, true)
                        .and_then(|pl| Ok(pl.publish_search_indexes()?))
                        .map_err(|e| format!("{e:#}"))
                })
                .await
                .map_err(|e| WorkerError::Work(format!("search-publish task panicked: {e}")))?
                .map_err(WorkerError::Work)?;
            }

            // Publish what we produced: every partition + the new generation watermark.
            let (partitions, watermark) =
                collect_partitions(&cfg, &ws).map_err(|e| WorkerError::Work(format!("{e:#}")))?;
            for (pid, location) in &partitions {
                client_w.register_partition(pid, location.clone()).await?;
            }
            let ids: Vec<String> = partitions.iter().map(|(p, _)| p.clone()).collect();
            client_w.record_entity_partitions(&shard_w, &ids).await?;
            guard.commit(watermark).await?;
            println!(
                "  generation {watermark} committed; {} partitions registered",
                partitions.len()
            );
            Ok::<(), WorkerError>(())
        })
        .await
        .with_context(|| format!("compiling shard '{shard}'"))?;
    println!("{id}: released shard '{shard}'");
    Ok(())
}

async fn run_pipeline(config: &EkosConfig, cwd: &Path, parallel_recover: bool) -> Result<()> {
    crate::commands::build::run(config, cwd).await?;
    crate::commands::recover::run(config, cwd, parallel_recover).await?;
    crate::commands::resolve::run(config, cwd, false)?;
    crate::commands::compile::run(config, cwd).await?;
    crate::commands::commit::run(config, cwd, true).await?;
    Ok(())
}

/// `(partition-id, location)` for every partition in the freshly-compiled workspace, plus the
/// store's monotonic entry count as the manifest-generation watermark. When
/// `[storage.partition] segment-backend-url` is set the location is that object store scoped to
/// the partition id (so a query worker pulls the sealed segments straight from there); otherwise
/// it's the partition's local root (shared-filesystem deployments).
fn collect_partitions(
    config: &EkosConfig,
    cwd: &Path,
) -> Result<(Vec<(String, PartitionLocation)>, u64)> {
    let pl = store::build_partitioned(config, cwd, true)?;
    let seg_url = config.storage.partition.segment_backend_url.clone();
    let partitions = pl
        .catalog_partition_keys()
        .into_iter()
        .filter_map(|k| {
            let pid = ekos_distributed::partition_id(&k);
            let root = pl.partition_root(&k)?;
            let location = match &seg_url {
                Some(base) => PartitionLocation::ObjectStore {
                    url: format!("{}/{pid}", base.trim_end_matches('/')),
                    prefix: String::new(),
                },
                None => PartitionLocation::Local {
                    root: root.to_string_lossy().into_owned(),
                },
            };
            Some((pid, location))
        })
        .collect();
    let watermark = pl.entry_count().map(|n| n as u64).unwrap_or(0);
    Ok((partitions, watermark))
}

/// `ekos query-worker serve --coordinator <addr> --listen <addr> [--cache <dir>]` — RFC 0113 B4
/// Service B. Materialises partitions on demand and serves `KnowledgeStore` reads for them over
/// newline-delimited JSON-RPC. Object-storage partitions need a build with `--features distributed`.
pub async fn serve_query_worker(
    coordinator: &str,
    listen: &str,
    cache: &std::path::Path,
) -> Result<()> {
    std::fs::create_dir_all(cache)
        .with_context(|| format!("creating query-worker cache dir {cache:?}"))?;
    let (bound, handle) =
        ekos_distributed::spawn_ephemeral_worker(listen, coordinator, cache.to_path_buf())
            .await
            .with_context(|| format!("starting query worker on {listen}"))?;
    tracing::info!(%bound, %coordinator, cache = %cache.display(), "query worker serving");
    println!("query worker listening on {bound} (coordinator {coordinator})");
    handle.await.ok();
    Ok(())
}
