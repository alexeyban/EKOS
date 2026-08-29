//! `ekos coordinator` and `ekos compile-worker` — RFC 0113 B3 (Distributed mode).
//!
//! `coordinator serve` runs the full metadata service (partition catalog, write leases with
//! fencing tokens, per-partition tx watermarks) over newline-delimited JSON-RPC on TCP. It holds
//! no partition data — object storage (RFC 0113 B2) does — and persists its own small state to a
//! single JSON file.
//!
//! `compile-worker` here is the transport/lifecycle half of Service A: it can acquire a lease,
//! heartbeat it, commit a watermark, and release — the smoke path for a live coordinator. Binding
//! a lease to an actual shard-scoped `build → commit` run is RFC 0113 B4 (`DistributedLedger`),
//! not yet wired.

use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Duration;
use ekos_cluster::{
    CompileWorker, Coordinator, CoordinatorClient, PartitionLocation, WorkerError, serve,
};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

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

/// `ekos compile-worker run --coordinator <addr> --partition <id> --root <dir> [--hold-seconds N]
/// [--watermark N]` — acquire the shard lease, hold it (heartbeating) for `hold_seconds`, commit
/// `watermark`, release. The smoke path against a live coordinator; real pass execution is B4.
pub async fn worker_run(
    coordinator: &str,
    partition: &str,
    root: &str,
    hold_seconds: u64,
    watermark: u64,
) -> Result<()> {
    let client = Arc::new(
        CoordinatorClient::connect(coordinator)
            .await
            .with_context(|| format!("connecting to coordinator at {coordinator}"))?,
    );
    client
        .register_partition(
            partition,
            PartitionLocation::Local {
                root: root.to_string(),
            },
        )
        .await?;

    let id = format!("worker-{}", std::process::id());
    let worker = CompileWorker::new(client.clone(), id.clone())
        .with_heartbeat(std::time::Duration::from_secs(hold_seconds.max(3) / 3));

    println!("{id}: acquiring lease on {partition}");
    worker
        .run_shard(partition, |guard| async move {
            println!("  lease held, token {}", guard.token());
            tokio::time::sleep(std::time::Duration::from_secs(hold_seconds)).await;
            guard.commit(watermark).await?;
            println!("  committed watermark {watermark}");
            Ok::<(), WorkerError>(())
        })
        .await
        .with_context(|| format!("running shard {partition}"))?;
    println!("{id}: released {partition}");
    Ok(())
}
