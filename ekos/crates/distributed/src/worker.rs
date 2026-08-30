//! Service B — the query / EAV-assembly worker (RFC 0113 B4).
//!
//! Stateless compute over cached, immutable partitions. On the first request for a partition the
//! worker asks the coordinator where it lives, materialises it into a local cache
//! ([`PartitionCache`]), and opens it as a **read-only** [`FactLedger`]; subsequent requests hit
//! the open handle. The existing `FactIndexes` fold / tantivy search run unchanged against the
//! cached copy. Every ledger call runs on a blocking thread (`spawn_blocking`) — RFC 0001's
//! sync-pipeline decision is untouched; async lives only at the RPC edge.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ekos_cluster::CoordinatorClient;
use ekos_ledger::FactLedger;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use crate::DistributedError;
use crate::cache::PartitionCache;
use crate::protocol::{WorkerRequest, WorkerResponse};

pub struct QueryWorker {
    coordinator: Arc<CoordinatorClient>,
    cache: Arc<PartitionCache>,
    open: Mutex<HashMap<String, Arc<FactLedger>>>,
}

impl QueryWorker {
    /// Connect to the coordinator at `coordinator_addr`; cache partitions under `cache_root`.
    pub async fn connect(
        coordinator_addr: impl tokio::net::ToSocketAddrs,
        cache_root: impl Into<PathBuf>,
    ) -> Result<Self, DistributedError> {
        let coordinator = CoordinatorClient::connect(coordinator_addr).await?;
        Ok(Self {
            coordinator: Arc::new(coordinator),
            cache: Arc::new(PartitionCache::new(cache_root)),
            open: Mutex::new(HashMap::new()),
        })
    }

    /// Test/embedding constructor — takes an already-connected coordinator client.
    pub fn with_coordinator(
        coordinator: Arc<CoordinatorClient>,
        cache_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            coordinator,
            cache: Arc::new(PartitionCache::new(cache_root)),
            open: Mutex::new(HashMap::new()),
        }
    }

    async fn ledger_for(&self, partition: &str) -> Result<Arc<FactLedger>, DistributedError> {
        if let Some(l) = self.open.lock().unwrap().get(partition) {
            return Ok(l.clone());
        }
        let metas = self.coordinator.catalog(Some(partition)).await?;
        let meta = metas
            .into_iter()
            .find(|m| m.id == partition)
            .ok_or_else(|| DistributedError::UnknownPartition(partition.to_string()))?;

        // Materialising (a possibly-remote download) and opening the segment store are sync,
        // mmap-heavy, and — for an object-storage backend — spin their own current-thread runtime,
        // so they must run off the async executor.
        let cache = self.cache.clone();
        let partition_owned = partition.to_string();
        let ledger = tokio::task::spawn_blocking(move || {
            let dir = cache.materialize(&partition_owned, &meta.location)?;
            Ok::<_, DistributedError>(Arc::new(FactLedger::open_read_only(&dir)?))
        })
        .await
        .map_err(|e| DistributedError::Other(format!("materialise task panicked: {e}")))??;

        self.open
            .lock()
            .unwrap()
            .insert(partition.to_string(), ledger.clone());
        Ok(ledger)
    }

    /// Handle one request end to end.
    pub async fn dispatch(&self, req: WorkerRequest) -> WorkerResponse {
        if matches!(req, WorkerRequest::Ping) {
            return WorkerResponse::Pong;
        }
        let Some(partition) = req.partition().map(str::to_string) else {
            return WorkerResponse::Error {
                message: "request has no partition".into(),
            };
        };
        let ledger = match self.ledger_for(&partition).await {
            Ok(l) => l,
            Err(e) => {
                return WorkerResponse::Error {
                    message: e.to_string(),
                };
            }
        };
        match tokio::task::spawn_blocking(move || run(&ledger, req)).await {
            Ok(resp) => resp,
            Err(join) => WorkerResponse::Error {
                message: format!("worker task panicked: {join}"),
            },
        }
    }
}

fn run(ledger: &FactLedger, req: WorkerRequest) -> WorkerResponse {
    let r: Result<WorkerResponse, ekos_ledger::LedgerError> = (|| {
        Ok(match req {
            WorkerRequest::Ping => WorkerResponse::Pong,
            WorkerRequest::GetObject { id, .. } => {
                WorkerResponse::Object(ledger.get_object(&id)?.map(Box::new))
            }
            WorkerRequest::GetRelationship { id, .. } => {
                WorkerResponse::Relationship(ledger.get_relationship(&id)?.map(Box::new))
            }
            WorkerRequest::GetEvent { id, .. } => {
                WorkerResponse::Event(ledger.get_event(&id)?.map(Box::new))
            }
            WorkerRequest::GetEvidence { id, .. } => {
                WorkerResponse::Evidence(ledger.get_evidence(&id)?.map(Box::new))
            }
            WorkerRequest::ObjectHistory { id, .. } => {
                WorkerResponse::Objects(ledger.object_history(&id)?)
            }
            WorkerRequest::RelationshipHistory { id, .. } => {
                WorkerResponse::Relationships(ledger.relationship_history(&id)?)
            }
            WorkerRequest::RelationshipsFor { id, .. } => {
                WorkerResponse::Relationships(ledger.relationships_for(&id)?)
            }
            WorkerRequest::AllObjects { .. } => WorkerResponse::Objects(ledger.all_objects()?),
            WorkerRequest::AllRelationships { .. } => {
                WorkerResponse::Relationships(ledger.all_relationships()?)
            }
            WorkerRequest::ObjectAt { id, at, .. } => {
                WorkerResponse::Object(ledger.object_at(&id, at)?.map(Box::new))
            }
            WorkerRequest::RelationshipsAt { id, at, .. } => {
                WorkerResponse::Relationships(ledger.relationships_at(&id, at)?)
            }
            WorkerRequest::AllObjectsAt { at, .. } => {
                WorkerResponse::Objects(ledger.all_objects_at(at)?)
            }
            WorkerRequest::AllRelationshipsAt { at, .. } => {
                WorkerResponse::Relationships(ledger.all_relationships_at(at)?)
            }
            WorkerRequest::FindObjects { query, .. } => {
                WorkerResponse::FindHits(ledger.find_objects(&query)?)
            }
            WorkerRequest::Diff { from, to, .. } => {
                WorkerResponse::Diff(ledger.diff(from, to)?.into())
            }
            WorkerRequest::ObjectCount { .. } => WorkerResponse::Count(ledger.object_count()?),
            WorkerRequest::RelationshipCount { .. } => {
                WorkerResponse::Count(ledger.relationship_count()?)
            }
            WorkerRequest::EntryCount { .. } => WorkerResponse::Count(ledger.entry_count()?),
        })
    })();
    r.unwrap_or_else(|e| WorkerResponse::Error {
        message: e.to_string(),
    })
}

/// Serve `worker` over newline-delimited JSON on `listener` until the process ends.
pub async fn serve(worker: Arc<QueryWorker>, listener: TcpListener) {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                tracing::warn!(%e, "query-worker accept failed");
                continue;
            }
        };
        let w = worker.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_conn(w, stream).await {
                tracing::debug!(%peer, %e, "query-worker connection ended");
            }
        });
    }
}

async fn serve_conn(worker: Arc<QueryWorker>, stream: TcpStream) -> Result<(), DistributedError> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<WorkerRequest>(&line) {
            Ok(req) => worker.dispatch(req).await,
            Err(e) => WorkerResponse::Error {
                message: format!("bad request: {e}"),
            },
        };
        let mut buf = serde_json::to_vec(&resp)?;
        buf.push(b'\n');
        write.write_all(&buf).await?;
    }
    Ok(())
}

/// Bind a query worker on `listen_addr`, wired to the coordinator at `coordinator_addr`, caching
/// under `cache_root`. Returns the bound address + the serve task handle.
pub async fn spawn_ephemeral_worker(
    listen_addr: &str,
    coordinator_addr: &str,
    cache_root: impl Into<PathBuf>,
) -> Result<(SocketAddr, tokio::task::JoinHandle<()>), DistributedError> {
    let listener = TcpListener::bind(listen_addr).await?;
    let bound = listener.local_addr()?;
    let worker = Arc::new(QueryWorker::connect(coordinator_addr, cache_root).await?);
    let handle = tokio::spawn(serve(worker, listener));
    Ok((bound, handle))
}
