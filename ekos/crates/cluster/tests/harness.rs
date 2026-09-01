//! RFC 0113 B3 — multi-service local harness.
//!
//! Spins up a real coordinator over TCP and several `CompileWorker`s / raw `CoordinatorClient`s
//! and exercises the acceptance items from RFC 0113 §B3:
//!
//! * two workers on disjoint shards commit concurrently, both land;
//! * two workers race the *same* shard — exactly one wins, the loser gets a clear "already
//!   leased" error;
//! * a worker that stops heartbeating loses its shard on TTL; its late `manifest_commit` is
//!   **fenced** (stale token), and the next worker resumes from the last committed watermark —
//!   bounded loss, never a corrupted or lost manifest;
//! * coordinator state (catalog + watermarks) survives a restart; leases do not.

use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::Duration;
use ekos_cluster::{
    CompileWorker, Coordinator, CoordinatorClient, PartitionLocation, WorkerError, serve,
    spawn_ephemeral,
};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

fn local(root: &str) -> PartitionLocation {
    PartitionLocation::Local {
        root: root.to_string(),
    }
}

#[tokio::test]
async fn two_workers_commit_disjoint_shards_concurrently() {
    let (addr, _srv) = spawn_ephemeral("127.0.0.1:0", None).await.unwrap();

    let run_one = |shard: &'static str, wm: u64| async move {
        let client = Arc::new(CoordinatorClient::connect(addr).await.unwrap());
        client
            .register_partition(shard, local(shard))
            .await
            .unwrap();
        let worker = CompileWorker::new(client.clone(), format!("w-{shard}"))
            .with_heartbeat(StdDuration::from_millis(50));
        worker
            .run_shard(shard, |guard| async move {
                guard.commit(wm).await?;
                Ok::<(), WorkerError>(())
            })
            .await
            .unwrap();
        client
    };

    let (c1, c2) = tokio::join!(
        run_one("kind=table/2026-08", 7),
        run_one("kind=view/2026-08", 4)
    );

    assert_eq!(c1.watermark("kind=table/2026-08").await.unwrap(), 7);
    assert_eq!(c2.watermark("kind=view/2026-08").await.unwrap(), 4);
    assert_eq!(c1.catalog(None).await.unwrap().len(), 2);
}

#[tokio::test]
async fn two_workers_race_one_shard_exactly_one_wins() {
    let (addr, _srv) = spawn_ephemeral("127.0.0.1:0", None).await.unwrap();
    let shard = "kind=table/2026-08";

    let ca = Arc::new(CoordinatorClient::connect(addr).await.unwrap());
    ca.register_partition(shard, local(shard)).await.unwrap();
    let cb = Arc::new(CoordinatorClient::connect(addr).await.unwrap());

    // A grabs the lease and holds it inside the work closure until B has had its turn.
    let (release_a_tx, release_a_rx) = tokio::sync::oneshot::channel::<()>();
    let wa =
        CompileWorker::new(ca.clone(), "worker-a").with_heartbeat(StdDuration::from_millis(50));
    let a_task = tokio::spawn(async move {
        wa.run_shard(shard, |guard| async move {
            release_a_rx.await.ok();
            guard.commit(5).await?;
            Ok::<(), WorkerError>(())
        })
        .await
    });

    // Give A time to take the lease, then B tries the same shard and must be turned away.
    tokio::time::sleep(StdDuration::from_millis(100)).await;
    let wb = CompileWorker::new(cb.clone(), "worker-b");
    let b_result = wb
        .run_shard(shard, |guard| async move {
            guard.commit(99).await?;
            Ok::<(), WorkerError>(())
        })
        .await;

    assert!(b_result.is_err(), "B must not get the held shard");
    assert!(
        format!("{}", b_result.unwrap_err()).contains("already leased"),
        "B's error should name the cause",
    );

    release_a_tx.send(()).unwrap();
    a_task.await.unwrap().unwrap();
    assert_eq!(ca.watermark(shard).await.unwrap(), 5);
}

#[tokio::test]
async fn expired_lease_is_fenced_and_next_worker_resumes_from_watermark() {
    // Short TTL so we can let a lease lapse without waiting.
    let (addr, _srv) = spawn_ephemeral("127.0.0.1:0", Some(Duration::milliseconds(150)))
        .await
        .unwrap();
    let shard = "kind=table/2026-08";

    let a = CoordinatorClient::connect(addr).await.unwrap();
    a.register_partition(shard, local(shard)).await.unwrap();

    // A acquires, commits a partial watermark, then "crashes" (stops renewing).
    let lease_a = a.lease_acquire(shard, "worker-a").await.unwrap();
    a.manifest_commit(shard, "worker-a", lease_a.token, 3)
        .await
        .unwrap();

    tokio::time::sleep(StdDuration::from_millis(250)).await;

    // B takes over the now-expired lease — strictly higher token.
    let b = CoordinatorClient::connect(addr).await.unwrap();
    let lease_b = b.lease_acquire(shard, "worker-b").await.unwrap();
    assert!(lease_b.token > lease_a.token);

    // B resumes from the last committed watermark, not from zero.
    assert_eq!(b.watermark(shard).await.unwrap(), 3);

    // A wakes up and tries to finish its old work — fenced, no effect.
    let stale = a.manifest_commit(shard, "worker-a", lease_a.token, 4).await;
    assert!(stale.is_err(), "stale-token commit must be rejected");
    assert_eq!(
        b.watermark(shard).await.unwrap(),
        3,
        "no partial/lost write"
    );

    // B commits cleanly on top.
    b.manifest_commit(shard, "worker-b", lease_b.token, 9)
        .await
        .unwrap();
    assert_eq!(b.watermark(shard).await.unwrap(), 9);
}

#[tokio::test]
async fn coordinator_state_survives_restart_but_leases_do_not() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("coordinator.json");
    let shard = "kind=table/2026-08";

    // First incarnation: register a partition, take a lease, commit a watermark.
    let token = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bound = listener.local_addr().unwrap();
        let co = Arc::new(Mutex::new(Coordinator::open(&state).unwrap()));
        let srv = tokio::spawn(serve(co, listener));

        let c = CoordinatorClient::connect(bound).await.unwrap();
        c.register_partition(shard, local(shard)).await.unwrap();
        let lease = c.lease_acquire(shard, "worker-a").await.unwrap();
        c.manifest_commit(shard, "worker-a", lease.token, 12)
            .await
            .unwrap();
        c.record_entity_partitions("entity-1", &[shard.to_string()])
            .await
            .unwrap();
        srv.abort();
        lease.token
    };

    // Second incarnation from the same state file.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound = listener.local_addr().unwrap();
    let co = Arc::new(Mutex::new(Coordinator::open(&state).unwrap()));
    let _srv = tokio::spawn(serve(co, listener));
    let c = CoordinatorClient::connect(bound).await.unwrap();

    // Durable: catalog, watermark, entity index.
    assert_eq!(c.catalog(None).await.unwrap().len(), 1);
    assert_eq!(c.watermark(shard).await.unwrap(), 12);
    assert_eq!(
        c.partitions_for_entity("entity-1").await.unwrap(),
        vec![shard.to_string()]
    );

    // Not durable: the old lease is gone, so a fresh acquire succeeds with a *fresh* token
    // sequence (the coordinator restarted, so the monotone counter restarts too — acceptable
    // because every pre-restart lease is definitionally dead).
    let fresh = c.lease_acquire(shard, "worker-b").await.unwrap();
    assert_eq!(fresh.token, 1);
    let _ = token;
}

/// Regression (found live 2026-09-01, `--ttl-seconds 8`): `CompileWorker`'s heartbeat interval is
/// derived from the lease's real TTL, not the fixed 10s default — so a coordinator with a short
/// TTL doesn't silently expire every worker's lease between beats mid-pipeline.
#[tokio::test]
async fn heartbeat_adapts_to_a_short_ttl_so_long_work_keeps_its_lease() {
    // 1s TTL — far below the 10s default heartbeat.
    let (addr, _srv) = spawn_ephemeral("127.0.0.1:0", Some(Duration::milliseconds(1000)))
        .await
        .unwrap();
    let client = Arc::new(CoordinatorClient::connect(addr).await.unwrap());
    client
        .register_partition("s/2026-08", local("s"))
        .await
        .unwrap();

    // DEFAULT heartbeat (no .with_heartbeat) — the production path.
    let worker = CompileWorker::new(client.clone(), "slow-worker");
    let res = worker
        .run_shard("s/2026-08", |guard| async move {
            // Work that outlasts several TTLs. Pre-fix the lease would be long expired here.
            tokio::time::sleep(StdDuration::from_secs(4)).await;
            guard.commit(9).await?; // fenced (LostLease) pre-fix; succeeds post-fix
            Ok::<(), WorkerError>(())
        })
        .await;
    assert!(
        res.is_ok(),
        "worker kept its lease through 4s of work under a 1s TTL: {res:?}"
    );
    assert_eq!(client.watermark("s/2026-08").await.unwrap(), 9);
}
