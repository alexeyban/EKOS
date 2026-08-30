//! RFC 0113 B4 — the Distributed-mode **read path**.
//!
//! Two things live here:
//!
//! * [`QueryWorker`] (Service B) — a stateless process that, given a partition id, materialises
//!   that partition into a bounded local cache (object storage → local files, or a co-located
//!   local dir used in place), opens it as a **read-only** [`ekos_ledger::FactLedger`], and serves
//!   `KnowledgeStore` reads for it over newline-delimited JSON-RPC. Any worker can serve any
//!   partition — sealed segments are immutable and object storage is the one durable copy, so
//!   there is no owned/replica set.
//! * [`DistributedLedger`] (Service C, added in B4b) — a `KnowledgeStore` implementation that
//!   fans reads across the [`QueryWorker`]s named by the coordinator and merges them, so every
//!   existing `KnowledgeStore` caller (Runtime, MCP, `docs-gen`) works unchanged against a
//!   cluster. Writes are rejected — those go through Service A (`ekos compile-worker`).
//!
//! Transport is the same newline-delimited JSON-RPC over TCP the coordinator (`ekos-cluster`) and
//! `ekos mcp serve` use — no gRPC.

mod cache;
mod gateway;
mod protocol;
mod worker;
mod worker_client;

pub use cache::{PartitionCache, partition_id};
pub use gateway::DistributedLedger;
pub use protocol::{DiffWire, WorkerRequest, WorkerResponse};
pub use worker::{QueryWorker, serve as serve_worker, spawn_ephemeral_worker};
pub use worker_client::QueryWorkerClient;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DistributedError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol: {0}")]
    Protocol(#[from] serde_json::Error),
    #[error("ledger: {0}")]
    Ledger(#[from] ekos_ledger::LedgerError),
    #[error("cluster: {0}")]
    Cluster(#[from] ekos_cluster::ClusterError),
    #[error("segment backend: {0}")]
    Backend(#[from] ekos_segment_backend::BackendError),
    #[error("partition {0} is not in the coordinator catalog")]
    UnknownPartition(String),
    #[error("query worker returned an error: {0}")]
    Worker(String),
    #[error("connection closed before a response was received")]
    Closed,
    #[error("{0}")]
    Other(String),
}
