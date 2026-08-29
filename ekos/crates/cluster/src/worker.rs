//! Service A — the compile/ingest worker (RFC 0113 B3).
//!
//! A worker leases a partition-scoped shard from the coordinator, becomes its sole writer, runs
//! the (existing, unmodified) compiler passes for that shard's inputs, and commits manifest
//! generations back through the coordinator with its fencing token. A lease is heartbeated for the
//! duration of the work; on any failure — including a lost lease — the shard is released and the
//! next worker resumes from the last committed watermark.
//!
//! This module is transport + lifecycle only. *What* a shard's work is (which passes, which
//! inputs) is the caller's closure — in `ekos compile-worker serve` it is the
//! `build/recover/resolve/compile/commit` pipeline scoped to the shard; in tests it is a stub that
//! appends batches to a `SegmentStore`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::ClusterError;
use crate::client::CoordinatorClient;

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error(transparent)]
    Cluster(#[from] ClusterError),
    #[error("shard work failed: {0}")]
    Work(String),
    #[error("lost the write lease for {partition} mid-work (fenced or expired)")]
    LostLease { partition: String },
}

/// Handed to the work closure: the live fencing token + a `commit` that advances the watermark
/// through the coordinator (fenced if the lease was lost). Owned (not borrowed) so the closure's
/// future may hold it across `.await` points freely.
pub struct LeaseGuard {
    client: Arc<CoordinatorClient>,
    partition: String,
    holder: String,
    token: u64,
    lost: Arc<AtomicBool>,
}

impl LeaseGuard {
    pub fn token(&self) -> u64 {
        self.token
    }
    pub fn partition(&self) -> &str {
        &self.partition
    }

    /// Commit a manifest generation (a sealed segment landed): advance the coordinator's
    /// watermark. `Err(LostLease)` if the lease was fenced/expired — the caller must stop.
    pub async fn commit(&self, watermark: u64) -> Result<(), WorkerError> {
        match self
            .client
            .manifest_commit(&self.partition, &self.holder, self.token, watermark)
            .await
        {
            Ok(()) => Ok(()),
            Err(ClusterError::Coordinator(_)) => {
                self.lost.store(true, Ordering::SeqCst);
                Err(WorkerError::LostLease {
                    partition: self.partition.clone(),
                })
            }
            Err(e) => Err(e.into()),
        }
    }
}

pub struct CompileWorker {
    client: Arc<CoordinatorClient>,
    id: String,
    heartbeat: Duration,
}

impl CompileWorker {
    pub fn new(client: Arc<CoordinatorClient>, id: impl Into<String>) -> Self {
        Self {
            client,
            id: id.into(),
            heartbeat: Duration::from_secs(10),
        }
    }

    /// Test/tuning hook — production uses ~TTL/3.
    pub fn with_heartbeat(mut self, d: Duration) -> Self {
        self.heartbeat = d;
        self
    }

    /// Lease `partition`, run `work`, release. Fails fast with [`WorkerError::LostLease`] if the
    /// lease is fenced/expired at any point (the caller's cue that another worker took the shard).
    pub async fn run_shard<F, Fut>(&self, partition: &str, work: F) -> Result<(), WorkerError>
    where
        F: FnOnce(LeaseGuard) -> Fut,
        Fut: std::future::Future<Output = Result<(), WorkerError>>,
    {
        let lease = self.client.lease_acquire(partition, &self.id).await?;
        let lost = Arc::new(AtomicBool::new(false));

        // Heartbeat until the guard is dropped.
        let hb_client = self.client.clone();
        let hb_id = self.id.clone();
        let hb_partition = partition.to_string();
        let hb_token = lease.token;
        let hb_lost = lost.clone();
        let hb_interval = self.heartbeat;
        let hb = tokio::spawn(async move {
            let mut tick = tokio::time::interval(hb_interval);
            tick.tick().await; // consume the immediate first tick
            loop {
                tick.tick().await;
                if hb_lost.load(Ordering::SeqCst) {
                    return;
                }
                match hb_client.lease_renew(&hb_partition, &hb_id, hb_token).await {
                    Ok(_) => {}
                    Err(_) => {
                        hb_lost.store(true, Ordering::SeqCst);
                        return;
                    }
                }
            }
        });

        let guard = LeaseGuard {
            client: self.client.clone(),
            partition: partition.to_string(),
            holder: self.id.clone(),
            token: lease.token,
            lost: lost.clone(),
        };
        let result = work(guard).await;

        hb.abort();
        // Best-effort release — a stale token here is harmless (a newer holder already owns it).
        let _ = self
            .client
            .lease_release(partition, &self.id, lease.token)
            .await;

        match result {
            Ok(()) if lost.load(Ordering::SeqCst) => Err(WorkerError::LostLease {
                partition: partition.to_string(),
            }),
            other => other,
        }
    }
}
