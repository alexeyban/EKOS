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
    ClusterError, CompileWorker, Coordinator, CoordinatorClient, PartitionLocation, WorkerError,
    serve,
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
    println!("{:<40}  {:>6}  location", "partition", "cold");
    for meta in &catalog {
        let loc = match &meta.location {
            PartitionLocation::Local { root } => format!("local:{root}"),
            PartitionLocation::ObjectStore { url, prefix } if prefix.is_empty() => url.clone(),
            PartitionLocation::ObjectStore { url, prefix } => format!("{url}/{prefix}"),
        };
        println!("{:<40}  {:>6}  {}", meta.id, meta.cold, loc);
    }

    // Committed watermarks (generation numbers) are keyed by the lease/shard name a compile
    // worker committed under, not by partition id — show them as their own section so the numbers
    // are actually visible (the old per-partition column always read 0).
    let watermarks = client.watermarks().await?;
    println!();
    if watermarks.is_empty() {
        println!("no committed generation watermarks yet");
    } else {
        println!("{:<40}  {:>12}", "shard", "generation");
        for (shard, wm) in &watermarks {
            println!("{shard:<40}  {wm:>12}");
        }
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
    force_resolve: bool,
    retry_lease_seconds: u64,
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

    // Open follow-on from RFC 0113 B3's live-testing notes: `run_shard` fails immediately if the
    // shard's lease is already held (a deliberate, tested contract — see
    // `crates/cluster/tests/harness.rs`'s "B must not get the held shard" case — so this retry
    // lives here, not inside `run_shard` itself). Retries *only* an "already leased" conflict —
    // the one error `lease_acquire` can produce before any work has run, so retrying it can never
    // re-run a pipeline that already started. Any other error (bad coordinator address, a
    // genuinely failed pipeline, a lease lost mid-run) still fails immediately regardless of this
    // flag, matching the pre-existing behavior when `retry_lease_seconds` is 0 (the default).
    const LEASE_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);
    let deadline = (retry_lease_seconds > 0)
        .then(|| std::time::Instant::now() + std::time::Duration::from_secs(retry_lease_seconds));
    let mut attempt = 0u32;

    let run_result = loop {
        attempt += 1;
        let ws = workspace.to_path_buf();
        let cfg = config.clone();
        let client_w = client.clone();

        let result = worker
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
                    .block_on(run_pipeline(&cfg2, &ws2, parallel_recover, force_resolve))
                    .map_err(|e| format!("{e:#}"))
            })
            .await
            .map_err(|e| WorkerError::Work(format!("pipeline task panicked: {e}")))?
            .map_err(WorkerError::Work)?;

            // Publish each partition's search index to its backend (no-op for a local backend),
            // collect what we produced (partitions + the new watermark), and collect every
            // object/relationship id's home partition for the coordinator's pruning index — one
            // `PartitionedLedger` open for all three (RFC 0113 v1.1).
            let cfg3 = cfg.clone();
            let ws3 = ws.clone();
            let (partitions, watermark, entity_ids) = tokio::task::spawn_blocking(move || {
                finalize_partitions(&cfg3, &ws3).map_err(|e| format!("{e:#}"))
            })
            .await
            .map_err(|e| WorkerError::Work(format!("finalize-partitions task panicked: {e}")))?
            .map_err(WorkerError::Work)?;

            for (pid, location) in &partitions {
                client_w.register_partition(pid, location.clone()).await?;
            }
            // An id can (rarely) span more than one partition across recompiles, so group before
            // recording — one `RecordEntityPartitions` call per distinct id, not per (id, partition)
            // pair.
            let mut by_entity: std::collections::HashMap<String, Vec<String>> =
                std::collections::HashMap::new();
            for (id, pid) in entity_ids {
                by_entity.entry(id).or_default().push(pid);
            }
            for (id, pids) in &by_entity {
                client_w.record_entity_partitions(id, pids).await?;
            }
            guard.commit(watermark).await?;
            println!(
                "  generation {watermark} committed; {} partitions registered",
                partitions.len()
            );
            Ok::<(), WorkerError>(())
        })
        .await;

        let is_lease_conflict = matches!(
            &result,
            Err(WorkerError::Cluster(ClusterError::Coordinator(msg))) if msg.contains("already leased")
        );
        let still_within_deadline = deadline.is_some_and(|d| std::time::Instant::now() < d);
        if is_lease_conflict && still_within_deadline {
            println!(
                "{id}: shard '{shard}' already leased (attempt {attempt}), retrying in \
                 {LEASE_RETRY_INTERVAL:?}…"
            );
            tokio::time::sleep(LEASE_RETRY_INTERVAL).await;
            continue;
        }
        break result;
    };
    run_result.with_context(|| format!("compiling shard '{shard}'"))?;
    println!("{id}: released shard '{shard}'");
    Ok(())
}

async fn run_pipeline(
    config: &EkosConfig,
    cwd: &Path,
    parallel_recover: bool,
    force_resolve: bool,
) -> Result<()> {
    crate::commands::build::run(config, cwd).await?;
    crate::commands::recover::run(config, cwd, parallel_recover).await?;
    crate::commands::resolve::run(config, cwd, force_resolve)?;
    crate::commands::compile::run(config, cwd).await?;
    crate::commands::commit::run(config, cwd, true).await?;
    Ok(())
}

/// `(partition-id, location)` pairs, the manifest-generation watermark, and `(id, home-partition)`
/// pairs for the coordinator's `entity_id → partitions` pruning index — [`finalize_partitions`]'s
/// result.
type FinalizedPartitions = (Vec<(String, PartitionLocation)>, u64, Vec<(String, String)>);

/// Runs after a compile: publish each partition's search index to its backend (a no-op for a
/// local backend), collect `(partition-id, location)` for every partition plus the store's
/// monotonic entry count as the manifest-generation watermark, and collect every object/
/// relationship id's home partition for the coordinator's `entity_id → partitions` pruning index
/// (RFC 0113 v1.1 — `DistributedLedger` uses it to prune id-scoped reads to the few partitions
/// that actually hold an id). One `PartitionedLedger` open for all three, rather than re-opening
/// the freshly-compiled workspace per concern. When `[storage.partition] segment-backend-url` is
/// set a partition's location is that object store scoped to the partition id (so a query worker
/// pulls the sealed segments straight from there); otherwise it's the partition's local root
/// (shared-filesystem deployments).
fn finalize_partitions(config: &EkosConfig, cwd: &Path) -> Result<FinalizedPartitions> {
    let pl = store::build_partitioned(config, cwd, true)?;
    pl.publish_search_indexes()?;
    // RFC 0113 B4 follow-on: also push the active (unsealed) segment + HEAD, so a query worker
    // serving this partition from object storage alone sees committed-but-unsealed rows — which,
    // with fine-grained partitioning where few partitions ever reach the 8 MiB seal threshold, is
    // most of the data.
    pl.publish_active_segments()?;

    let seg_url = config.storage.partition.segment_backend_url.clone();
    let mut partitions = Vec::new();
    let mut entity_ids = Vec::new();
    for key in pl.catalog_partition_keys() {
        let pid = ekos_distributed::partition_id(&key);
        if let Some(root) = pl.partition_root(&key) {
            let location = match &seg_url {
                Some(base) => PartitionLocation::ObjectStore {
                    url: format!("{}/{pid}", base.trim_end_matches('/')),
                    prefix: String::new(),
                },
                None => PartitionLocation::Local {
                    root: root.to_string_lossy().into_owned(),
                },
            };
            partitions.push((pid.clone(), location));
        }
        for id in pl.partition_entity_ids(&key)? {
            entity_ids.push((id.to_string(), pid.clone()));
        }
    }
    let watermark = pl.entry_count().map(|n| n as u64).unwrap_or(0);
    Ok((partitions, watermark, entity_ids))
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
