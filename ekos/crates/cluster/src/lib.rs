//! RFC 0113 B3 — the Distributed-mode **coordinator** and **compile-worker** (Service A).
//!
//! The coordinator is a small, centralized metadata service in front of object storage (the model
//! Delta Lake / Iceberg / Hive Metastore use): it owns the partition catalog, hands out
//! short-lived **write leases** (fencing-tokened), and records the committed **tx watermark** per
//! partition. Object storage (RFC 0113 B2) holds the real data; the coordinator holds none of it.
//!
//! **v1 transport is newline-delimited JSON-RPC over TCP** — the same pattern `ekos mcp serve`
//! already uses — not gRPC/tonic. Rationale: no protobuf codegen, no new heavy dependency, one
//! well-understood framing. Mutual TLS (RFC 0113's stated v1 default) is a transport-level
//! follow-on; today the coordinator is expected to run on a trusted cluster network or localhost.
//!
//! **v1 coordinator is a single process — an acknowledged SPOF** (RFC 0111 §9). State persists to
//! one JSON file (atomic temp+rename); Raft-replicated metadata is the named v2.
//!
//! `PartitionId` is an opaque string here (`"<dimension_value>/<time_bucket>"`) — the coordinator
//! never needs to parse it, which keeps this crate free of an `ekos-ledger` dependency.

mod catalog;
mod client;
mod coordinator;
mod lease;
mod protocol;
mod worker;

pub use catalog::{Catalog, PartitionLocation, PartitionMeta};
pub use client::CoordinatorClient;
pub use coordinator::{Coordinator, LEASE_TTL, serve, spawn_ephemeral};
pub use lease::{Lease, LeaseError, LeaseTable};
pub use protocol::{PartitionId, Request, Response};
pub use worker::{CompileWorker, LeaseGuard, WorkerError};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClusterError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol: {0}")]
    Protocol(#[from] serde_json::Error),
    #[error("coordinator returned an error: {0}")]
    Coordinator(String),
    #[error("connection closed before a response was received")]
    Closed,
}
