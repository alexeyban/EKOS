//! A newline-delimited-JSON client for the coordinator. One TCP connection, held open, one
//! request/response round-trip per call.

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

use crate::ClusterError;
use crate::catalog::{PartitionLocation, PartitionMeta};
use crate::lease::Lease;
use crate::protocol::{PartitionId, Request, Response};

pub struct CoordinatorClient {
    write: Mutex<OwnedWriteHalf>,
    read: Mutex<tokio::io::Lines<BufReader<OwnedReadHalf>>>,
}

impl CoordinatorClient {
    pub async fn connect(addr: impl tokio::net::ToSocketAddrs) -> Result<Self, ClusterError> {
        let stream = TcpStream::connect(addr).await?;
        stream.set_nodelay(true).ok();
        let (read, write) = stream.into_split();
        Ok(Self {
            write: Mutex::new(write),
            read: Mutex::new(BufReader::new(read).lines()),
        })
    }

    /// Send one request, read one response. Calls are serialised by the two `Mutex`es so
    /// concurrent callers can't interleave frames on the shared connection.
    pub async fn call(&self, req: &Request) -> Result<Response, ClusterError> {
        let mut line = serde_json::to_vec(req)?;
        line.push(b'\n');
        {
            let mut w = self.write.lock().await;
            w.write_all(&line).await?;
            w.flush().await?;
        }
        let mut r = self.read.lock().await;
        let resp = r.next_line().await?.ok_or(ClusterError::Closed)?;
        Ok(serde_json::from_str(&resp)?)
    }

    // ── typed helpers ──────────────────────────────────────────────────────

    pub async fn register_partition(
        &self,
        id: &str,
        location: PartitionLocation,
    ) -> Result<(), ClusterError> {
        self.call(&Request::CatalogRegister {
            meta: PartitionMeta {
                id: id.to_string(),
                location,
                cold: false,
            },
        })
        .await?
        .ok()?;
        Ok(())
    }

    pub async fn catalog(&self, prefix: Option<&str>) -> Result<Vec<PartitionMeta>, ClusterError> {
        match self
            .call(&Request::CatalogGet {
                prefix: prefix.map(str::to_string),
            })
            .await?
            .ok()?
        {
            Response::Catalog { partitions } => Ok(partitions),
            other => Err(ClusterError::Coordinator(format!("unexpected {other:?}"))),
        }
    }

    pub async fn lease_acquire(
        &self,
        partition: &str,
        holder: &str,
    ) -> Result<Lease, ClusterError> {
        match self
            .call(&Request::LeaseAcquire {
                partition: partition.to_string(),
                holder: holder.to_string(),
            })
            .await?
            .ok()?
        {
            Response::Lease { lease } => Ok(lease),
            other => Err(ClusterError::Coordinator(format!("unexpected {other:?}"))),
        }
    }

    pub async fn lease_renew(
        &self,
        partition: &str,
        holder: &str,
        token: u64,
    ) -> Result<Lease, ClusterError> {
        match self
            .call(&Request::LeaseRenew {
                partition: partition.to_string(),
                holder: holder.to_string(),
                token,
            })
            .await?
            .ok()?
        {
            Response::Lease { lease } => Ok(lease),
            other => Err(ClusterError::Coordinator(format!("unexpected {other:?}"))),
        }
    }

    pub async fn lease_release(
        &self,
        partition: &str,
        holder: &str,
        token: u64,
    ) -> Result<(), ClusterError> {
        self.call(&Request::LeaseRelease {
            partition: partition.to_string(),
            holder: holder.to_string(),
            token,
        })
        .await?
        .ok()?;
        Ok(())
    }

    /// Advance the committed watermark. Returns `Err` (fenced) if `token` is stale.
    pub async fn manifest_commit(
        &self,
        partition: &str,
        holder: &str,
        token: u64,
        watermark: u64,
    ) -> Result<(), ClusterError> {
        self.call(&Request::ManifestCommit {
            partition: partition.to_string(),
            holder: holder.to_string(),
            token,
            watermark,
        })
        .await?
        .ok()?;
        Ok(())
    }

    pub async fn record_entity_partitions(
        &self,
        entity: &str,
        partitions: &[PartitionId],
    ) -> Result<(), ClusterError> {
        self.call(&Request::RecordEntityPartitions {
            entity: entity.to_string(),
            partitions: partitions.to_vec(),
        })
        .await?
        .ok()?;
        Ok(())
    }

    pub async fn partitions_for_entity(
        &self,
        entity: &str,
    ) -> Result<Vec<PartitionId>, ClusterError> {
        match self
            .call(&Request::PartitionsForEntity {
                entity: entity.to_string(),
            })
            .await?
            .ok()?
        {
            Response::Partitions { partitions } => Ok(partitions),
            other => Err(ClusterError::Coordinator(format!("unexpected {other:?}"))),
        }
    }

    pub async fn watermark(&self, partition: &str) -> Result<u64, ClusterError> {
        match self
            .call(&Request::Watermark {
                partition: partition.to_string(),
            })
            .await?
            .ok()?
        {
            Response::Watermark { watermark } => Ok(watermark),
            other => Err(ClusterError::Coordinator(format!("unexpected {other:?}"))),
        }
    }
}
