//! The coordinator RPC surface (RFC 0113 B3). One JSON [`Request`] per line in, one [`Response`]
//! per line out.

use serde::{Deserialize, Serialize};

use crate::catalog::PartitionMeta;
use crate::lease::{Lease, LeaseError};

/// Opaque partition identifier — `"<dimension_value>/<time_bucket>"`.
pub type PartitionId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "op")]
pub enum Request {
    /// Register a partition on first write.
    CatalogRegister { meta: PartitionMeta },
    /// The full catalog, optionally pruned to ids starting with `prefix`.
    CatalogGet { prefix: Option<String> },
    /// Acquire (or take over an expired) write lease.
    LeaseAcquire {
        partition: PartitionId,
        holder: String,
    },
    LeaseRenew {
        partition: PartitionId,
        holder: String,
        token: u64,
    },
    LeaseRelease {
        partition: PartitionId,
        holder: String,
        token: u64,
    },
    /// Advance a partition's committed tx watermark. Fenced if `token` is stale.
    ManifestCommit {
        partition: PartitionId,
        holder: String,
        token: u64,
        /// The new manifest generation / highest committed tx.
        watermark: u64,
    },
    /// Record `(entity_id, partition_id)` membership pairs (the run-file index, served centrally
    /// in Distributed mode).
    RecordEntityPartitions {
        entity: String,
        partitions: Vec<PartitionId>,
    },
    /// The partitions an entity has a version in.
    PartitionsForEntity { entity: String },
    /// The committed watermark for a partition (`0` if none).
    Watermark { partition: PartitionId },
    /// Every committed watermark the coordinator holds, keyed by the lease name it was committed
    /// under (the shard, e.g. `"main"`, for entity-kind partitioning). Used by
    /// `ekos coordinator status` to show real generation numbers instead of the per-partition-id
    /// `Watermark` lookup, which is always `0` because commits are keyed by shard, not partition.
    Watermarks,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "status")]
pub enum Response {
    Ok,
    Catalog {
        partitions: Vec<PartitionMeta>,
    },
    Lease {
        lease: Lease,
    },
    LeaseError {
        error: LeaseError,
    },
    Partitions {
        partitions: Vec<PartitionId>,
    },
    Watermark {
        watermark: u64,
    },
    /// Every committed watermark, keyed by lease/shard name.
    WatermarkMap {
        watermarks: std::collections::BTreeMap<PartitionId, u64>,
    },
    /// A register/commit/other error the coordinator couldn't express as a `LeaseError`.
    Error {
        message: String,
    },
}

impl Response {
    pub fn ok(self) -> Result<Self, crate::ClusterError> {
        match self {
            Response::Error { message } => Err(crate::ClusterError::Coordinator(message)),
            Response::LeaseError { error } => {
                Err(crate::ClusterError::Coordinator(error.to_string()))
            }
            other => Ok(other),
        }
    }
}
