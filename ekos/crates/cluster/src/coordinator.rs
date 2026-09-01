//! The coordinator: in-memory state (catalog, leases, watermarks, entity index) + JSON
//! persistence + a newline-delimited-JSON TCP server.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::ClusterError;
use crate::catalog::Catalog;
use crate::lease::LeaseTable;
use crate::protocol::{PartitionId, Request, Response};

/// Write-lease TTL and the coordinator's clock skew grace. Workers heartbeat at ~1/3 the TTL.
pub const LEASE_TTL: Duration = Duration::seconds(30);

#[derive(Default, Serialize, Deserialize)]
struct PersistedState {
    catalog: Catalog,
    watermarks: HashMap<PartitionId, u64>,
    entity_partitions: HashMap<String, Vec<PartitionId>>,
}

pub struct Coordinator {
    state_path: Option<PathBuf>,
    catalog: Catalog,
    leases: LeaseTable,
    watermarks: HashMap<PartitionId, u64>,
    entity_partitions: HashMap<String, Vec<PartitionId>>,
    ttl: Duration,
}

impl Coordinator {
    /// Open a coordinator, loading `<state_path>` if it exists. Leases are **not** persisted —
    /// they are TTL-bounded and a coordinator restart correctly invalidates every outstanding one.
    pub fn open(state_path: impl Into<PathBuf>) -> Result<Self, ClusterError> {
        let state_path = state_path.into();
        let persisted: PersistedState = if state_path.exists() {
            serde_json::from_slice(&std::fs::read(&state_path)?)?
        } else {
            PersistedState::default()
        };
        Ok(Self {
            state_path: Some(state_path),
            catalog: persisted.catalog,
            leases: LeaseTable::default(),
            watermarks: persisted.watermarks,
            entity_partitions: persisted.entity_partitions,
            ttl: LEASE_TTL,
        })
    }

    /// An ephemeral, non-persisting coordinator — for tests and `--in-memory`.
    pub fn ephemeral() -> Self {
        Self {
            state_path: None,
            catalog: Catalog::default(),
            leases: LeaseTable::default(),
            watermarks: HashMap::new(),
            entity_partitions: HashMap::new(),
            ttl: LEASE_TTL,
        }
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }
    pub fn watermark(&self, partition: &str) -> u64 {
        self.watermarks.get(partition).copied().unwrap_or(0)
    }
    pub fn partitions_for_entity(&self, entity: &str) -> Vec<PartitionId> {
        self.entity_partitions
            .get(entity)
            .cloned()
            .unwrap_or_default()
    }

    fn persist(&self) -> Result<(), ClusterError> {
        let Some(path) = &self.state_path else {
            return Ok(());
        };
        let state = PersistedState {
            catalog: self.catalog.clone(),
            watermarks: self.watermarks.clone(),
            entity_partitions: self.entity_partitions.clone(),
        };
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&state)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Handle one request. The single place every mutation goes through, so persistence is in one
    /// place too.
    pub fn handle(&mut self, req: Request) -> Response {
        let now = Utc::now();
        match req {
            Request::CatalogRegister { meta } => match self.catalog.register(meta) {
                Ok(()) => self.persisted(Response::Ok),
                Err(message) => Response::Error { message },
            },
            Request::CatalogGet { prefix } => {
                let partitions = self
                    .catalog
                    .partitions
                    .iter()
                    .filter(|p| prefix.as_deref().is_none_or(|pre| p.id.starts_with(pre)))
                    .cloned()
                    .collect();
                Response::Catalog { partitions }
            }
            Request::LeaseAcquire { partition, holder } => {
                match self.leases.acquire(&partition, &holder, now, self.ttl) {
                    Ok(lease) => Response::Lease { lease },
                    Err(error) => Response::LeaseError { error },
                }
            }
            Request::LeaseRenew {
                partition,
                holder,
                token,
            } => match self.leases.renew(&partition, &holder, token, now, self.ttl) {
                Ok(lease) => Response::Lease { lease },
                Err(error) => Response::LeaseError { error },
            },
            Request::LeaseRelease {
                partition,
                holder,
                token,
            } => match self.leases.release(&partition, &holder, token, now) {
                Ok(()) => Response::Ok,
                Err(error) => Response::LeaseError { error },
            },
            Request::ManifestCommit {
                partition,
                holder,
                token,
                watermark,
            } => match self.leases.check(&partition, &holder, token, now) {
                Ok(()) => {
                    let entry = self.watermarks.entry(partition).or_insert(0);
                    *entry = (*entry).max(watermark);
                    self.persisted(Response::Ok)
                }
                Err(error) => Response::LeaseError { error },
            },
            Request::RecordEntityPartitions { entity, partitions } => {
                let set = self.entity_partitions.entry(entity).or_default();
                for p in partitions {
                    if !set.contains(&p) {
                        set.push(p);
                    }
                }
                set.sort();
                self.persisted(Response::Ok)
            }
            Request::PartitionsForEntity { entity } => Response::Partitions {
                partitions: self.partitions_for_entity(&entity),
            },
            Request::Watermark { partition } => Response::Watermark {
                watermark: self.watermark(&partition),
            },
            Request::Watermarks => Response::WatermarkMap {
                watermarks: self
                    .watermarks
                    .iter()
                    .map(|(k, v)| (k.clone(), *v))
                    .collect(),
            },
        }
    }

    fn persisted(&self, ok: Response) -> Response {
        if let Err(e) = self.persist() {
            return Response::Error {
                message: format!("coordinator persistence failed: {e}"),
            };
        }
        ok
    }
}

/// Serve `coordinator` over newline-delimited JSON on `listener` until the process ends. Each
/// connection is one framed request/response stream; many connections are served concurrently, all
/// against the one `Mutex<Coordinator>` (every `handle` call is short).
pub async fn serve(coordinator: Arc<Mutex<Coordinator>>, listener: TcpListener) {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                tracing::warn!(%e, "coordinator accept failed");
                continue;
            }
        };
        let co = coordinator.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_conn(co, stream).await {
                tracing::debug!(%peer, %e, "coordinator connection ended");
            }
        });
    }
}

async fn serve_conn(
    coordinator: Arc<Mutex<Coordinator>>,
    stream: TcpStream,
) -> Result<(), ClusterError> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<Request>(&line) {
            Ok(req) => coordinator.lock().await.handle(req),
            Err(e) => Response::Error {
                message: format!("bad request: {e}"),
            },
        };
        let mut buf = serde_json::to_vec(&resp)?;
        buf.push(b'\n');
        write.write_all(&buf).await?;
    }
    Ok(())
}

/// Convenience for `main` / tests: bind an ephemeral coordinator on `addr`, returning the bound
/// address and a handle to the serve task.
pub async fn spawn_ephemeral(
    addr: &str,
    ttl: Option<Duration>,
) -> Result<(std::net::SocketAddr, tokio::task::JoinHandle<()>), ClusterError> {
    let listener = TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    let mut co = Coordinator::ephemeral();
    if let Some(t) = ttl {
        co = co.with_ttl(t);
    }
    let co = Arc::new(Mutex::new(co));
    let handle = tokio::spawn(serve(co, listener));
    Ok((bound, handle))
}
