//! Service C — the query gateway (RFC 0113 B4b).
//!
//! [`DistributedLedger`] implements [`KnowledgeStore`] by fanning reads across the [`QueryWorker`]s
//! the coordinator knows about and merging the results, so every existing `KnowledgeStore` caller
//! (Runtime, MCP, `docs-gen`) works unchanged against a cluster. It holds no partition data.
//! **Writes are rejected** — `ekos build` / `commit` in Distributed mode is Service A
//! (`ekos compile-worker`), and the gateway is read-only, matching the Runtime-is-read-only
//! invariant.
//!
//! v1 keeps no persistent connection pool: each call opens fresh coordinator + worker
//! connections (localhost / cluster-internal TCP, a few ms) and fans out **sequentially**. A
//! health-checked pool + parallel fan-out is a follow-on, tracked in RFC 0113.
//!
//! v1 also does not use the coordinator's `id → partitions` index to prune: an entity-scoped read
//! fans to *every* partition of the right class (object / relationship / event / evidence). This
//! is correct, just not minimal; pruning lands once Service A populates that index on write.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{DateTime, Utc};
use ekos_cluster::CoordinatorClient;
use ekos_kir::{KirEvent, KirEvidence, KirId, KirObject, KirRelationship};
use ekos_ledger::{KnowledgeStore, LedgerDiff, LedgerError};
use tokio::runtime::Handle;

use crate::DistributedError;
use crate::worker_client::QueryWorkerClient;

#[derive(PartialEq, Eq, Clone, Copy)]
enum PClass {
    Object,
    Rel,
    Event,
    Evidence,
}

fn classify(partition_id: &str) -> PClass {
    if partition_id.starts_with("rel:") {
        PClass::Rel
    } else if partition_id.starts_with("events/") {
        PClass::Event
    } else if partition_id.starts_with("evidence/") {
        PClass::Evidence
    } else {
        PClass::Object
    }
}

/// The time-bucket suffix — `"Table/2026-08"` → `"2026-08"`. Lexical order == chronological
/// (RFC 0111 §1), so the greatest bucket is the newest partition.
fn bucket(partition_id: &str) -> &str {
    partition_id
        .rsplit_once('/')
        .map_or(partition_id, |(_, b)| b)
}

pub struct DistributedLedger {
    coordinator_addr: String,
    worker_addrs: Vec<String>,
    next_worker: AtomicUsize,
}

/// Drive `fut` to completion from a **sync** context: reuse the ambient multi-threaded runtime via
/// `block_in_place` when there is one (the `#[tokio::main]` CLI, `ekos mcp serve`), otherwise spin
/// a transient current-thread runtime for this one call. Never *owns* a runtime — a stored
/// `Runtime` would panic if dropped while an async context is on the stack (which is exactly how
/// `open_store` is called).
fn block_on_sync<F: Future>(fut: F) -> F::Output {
    match Handle::try_current() {
        Ok(h) => tokio::task::block_in_place(|| h.block_on(fut)),
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build a transient tokio runtime")
            .block_on(fut),
    }
}

impl DistributedLedger {
    /// `worker_addrs` must be non-empty. Fails fast if the coordinator is unreachable.
    pub fn open(
        coordinator_addr: impl Into<String>,
        worker_addrs: Vec<String>,
    ) -> Result<Self, DistributedError> {
        if worker_addrs.is_empty() {
            return Err(DistributedError::Other(
                "a distributed gateway needs at least one query-worker address".into(),
            ));
        }
        let coordinator_addr = coordinator_addr.into();
        block_on_sync(CoordinatorClient::connect(&coordinator_addr))?;
        Ok(Self {
            coordinator_addr,
            worker_addrs,
            next_worker: AtomicUsize::new(0),
        })
    }

    fn block_on<F: Future>(&self, fut: F) -> F::Output {
        block_on_sync(fut)
    }

    async fn coordinator(&self) -> Result<CoordinatorClient, DistributedError> {
        Ok(CoordinatorClient::connect(&self.coordinator_addr).await?)
    }

    async fn any_worker(&self) -> Result<QueryWorkerClient, DistributedError> {
        let i = self.next_worker.fetch_add(1, Ordering::Relaxed) % self.worker_addrs.len();
        QueryWorkerClient::connect(&self.worker_addrs[i]).await
    }

    /// Catalogued partition ids of one class, newest bucket first.
    async fn partitions(&self, class: PClass) -> Result<Vec<String>, DistributedError> {
        let mut ids: Vec<String> = self
            .coordinator()
            .await?
            .catalog(None)
            .await?
            .into_iter()
            .map(|m| m.id)
            .filter(|id| classify(id) == class)
            .collect();
        ids.sort_by(|a, b| bucket(b).cmp(bucket(a)).then_with(|| b.cmp(a)));
        Ok(ids)
    }

    fn run<T>(
        &self,
        fut: impl Future<Output = Result<T, DistributedError>>,
    ) -> Result<T, LedgerError> {
        self.block_on(fut).map_err(de_to_le)
    }
}

fn de_to_le(e: DistributedError) -> LedgerError {
    match e {
        DistributedError::Ledger(le) => le,
        other => LedgerError::Io(std::io::Error::other(other.to_string())),
    }
}

fn write_rejected(what: &str) -> LedgerError {
    LedgerError::ReadOnly(format!(
        "distributed gateway is read-only — {what} must go through `ekos compile-worker` (Service A)"
    ))
}

impl KnowledgeStore for DistributedLedger {
    fn append_object(&self, _obj: &KirObject) -> Result<bool, LedgerError> {
        Err(write_rejected("append_object"))
    }
    fn append_evidence(&self, _ev: &KirEvidence) -> Result<(), LedgerError> {
        Err(write_rejected("append_evidence"))
    }
    fn append_relationship(&self, _rel: &KirRelationship) -> Result<bool, LedgerError> {
        Err(write_rejected("append_relationship"))
    }
    fn append_event(&self, _ev: &KirEvent) -> Result<(), LedgerError> {
        Err(write_rejected("append_event"))
    }

    fn get_object(&self, id: &KirId) -> Result<Option<KirObject>, LedgerError> {
        let id = *id;
        self.run(async move {
            for pid in self.partitions(PClass::Object).await? {
                let w = self.any_worker().await?;
                if let Some(o) = w.get_object(&pid, id).await? {
                    return Ok(Some(o));
                }
            }
            Ok(None)
        })
    }

    fn get_relationship(&self, id: &KirId) -> Result<Option<KirRelationship>, LedgerError> {
        let id = *id;
        self.run(async move {
            for pid in self.partitions(PClass::Rel).await? {
                let w = self.any_worker().await?;
                if let Some(r) = w.get_relationship(&pid, id).await? {
                    return Ok(Some(r));
                }
            }
            Ok(None)
        })
    }

    fn get_event(&self, id: &KirId) -> Result<Option<KirEvent>, LedgerError> {
        let id = *id;
        self.run(async move {
            for pid in self.partitions(PClass::Event).await? {
                let w = self.any_worker().await?;
                if let Some(e) = w.get_event(&pid, id).await? {
                    return Ok(Some(e));
                }
            }
            Ok(None)
        })
    }

    fn get_evidence(&self, id: &KirId) -> Result<Option<KirEvidence>, LedgerError> {
        let id = *id;
        self.run(async move {
            for pid in self.partitions(PClass::Evidence).await? {
                let w = self.any_worker().await?;
                if let Some(e) = w.get_evidence(&pid, id).await? {
                    return Ok(Some(e));
                }
            }
            Ok(None)
        })
    }

    fn all_objects(&self) -> Result<Vec<KirObject>, LedgerError> {
        self.run(async move {
            let mut by_id: std::collections::HashMap<KirId, KirObject> = Default::default();
            // oldest bucket first, so a newer partition's version overwrites
            let mut pids = self.partitions(PClass::Object).await?;
            pids.reverse();
            for pid in pids {
                let w = self.any_worker().await?;
                for o in w.all_objects(&pid).await? {
                    by_id.insert(o.id, o);
                }
            }
            Ok(by_id.into_values().collect())
        })
    }

    fn all_relationships(&self) -> Result<Vec<KirRelationship>, LedgerError> {
        self.run(async move {
            let mut by_id: std::collections::HashMap<KirId, KirRelationship> = Default::default();
            let mut pids = self.partitions(PClass::Rel).await?;
            pids.reverse();
            for pid in pids {
                let w = self.any_worker().await?;
                for r in w.all_relationships(&pid).await? {
                    by_id.insert(r.id, r);
                }
            }
            Ok(by_id.into_values().collect())
        })
    }

    fn relationships_for(&self, id: &KirId) -> Result<Vec<KirRelationship>, LedgerError> {
        let id = *id;
        self.run(async move {
            let mut by_id: std::collections::HashMap<KirId, KirRelationship> = Default::default();
            let mut pids = self.partitions(PClass::Rel).await?;
            pids.reverse();
            for pid in pids {
                let w = self.any_worker().await?;
                for r in w.relationships_for(&pid, id).await? {
                    by_id.insert(r.id, r);
                }
            }
            Ok(by_id.into_values().collect())
        })
    }

    fn object_at(&self, id: &KirId, at: DateTime<Utc>) -> Result<Option<KirObject>, LedgerError> {
        let id = *id;
        self.run(async move {
            // newest partition first; first version at-or-before `at` wins
            for pid in self.partitions(PClass::Object).await? {
                let w = self.any_worker().await?;
                if let Some(o) = w.object_at(&pid, id, at).await? {
                    return Ok(Some(o));
                }
            }
            Ok(None)
        })
    }

    fn relationships_at(
        &self,
        id: &KirId,
        at: DateTime<Utc>,
    ) -> Result<Vec<KirRelationship>, LedgerError> {
        let id = *id;
        self.run(async move {
            let mut by_id: std::collections::HashMap<KirId, KirRelationship> = Default::default();
            let mut pids = self.partitions(PClass::Rel).await?;
            pids.reverse();
            for pid in pids {
                let w = self.any_worker().await?;
                for r in w.relationships_at(&pid, id, at).await? {
                    by_id.insert(r.id, r);
                }
            }
            Ok(by_id.into_values().collect())
        })
    }

    fn all_objects_at(&self, at: DateTime<Utc>) -> Result<Vec<KirObject>, LedgerError> {
        self.run(async move {
            let mut by_id: std::collections::HashMap<KirId, KirObject> = Default::default();
            let mut pids = self.partitions(PClass::Object).await?;
            pids.reverse();
            for pid in pids {
                let w = self.any_worker().await?;
                for o in w.all_objects_at(&pid, at).await? {
                    by_id.insert(o.id, o);
                }
            }
            Ok(by_id.into_values().collect())
        })
    }

    fn all_relationships_at(&self, at: DateTime<Utc>) -> Result<Vec<KirRelationship>, LedgerError> {
        self.run(async move {
            let mut by_id: std::collections::HashMap<KirId, KirRelationship> = Default::default();
            let mut pids = self.partitions(PClass::Rel).await?;
            pids.reverse();
            for pid in pids {
                let w = self.any_worker().await?;
                for r in w.all_relationships_at(&pid, at).await? {
                    by_id.insert(r.id, r);
                }
            }
            Ok(by_id.into_values().collect())
        })
    }

    fn object_history(&self, id: &KirId) -> Result<Vec<KirObject>, LedgerError> {
        let id = *id;
        self.run(async move {
            let mut out = Vec::new();
            let mut pids = self.partitions(PClass::Object).await?;
            pids.reverse(); // oldest bucket first
            for pid in pids {
                let w = self.any_worker().await?;
                out.extend(w.object_history(&pid, id).await?);
            }
            Ok(out)
        })
    }

    fn relationship_history(&self, id: &KirId) -> Result<Vec<KirRelationship>, LedgerError> {
        let id = *id;
        self.run(async move {
            let mut out = Vec::new();
            let mut pids = self.partitions(PClass::Rel).await?;
            pids.reverse();
            for pid in pids {
                let w = self.any_worker().await?;
                out.extend(w.relationship_history(&pid, id).await?);
            }
            Ok(out)
        })
    }

    fn find_objects(&self, query: &str) -> Result<Vec<(KirId, String)>, LedgerError> {
        let query = query.to_string();
        self.run(async move {
            let mut out = Vec::new();
            for pid in self.partitions(PClass::Object).await? {
                let w = self.any_worker().await?;
                out.extend(w.find_objects(&pid, &query).await?);
            }
            Ok(out)
        })
    }

    fn entry_count(&self) -> Result<usize, LedgerError> {
        self.run(async move {
            let mut n = 0;
            let cat = self.coordinator().await?.catalog(None).await?;
            for m in cat {
                let w = self.any_worker().await?;
                n += w.entry_count(&m.id).await?;
            }
            Ok(n)
        })
    }

    fn object_count(&self) -> Result<usize, LedgerError> {
        self.run(async move {
            let mut n = 0;
            for pid in self.partitions(PClass::Object).await? {
                let w = self.any_worker().await?;
                n += w.object_count(&pid).await?;
            }
            Ok(n)
        })
    }

    fn relationship_count(&self) -> Result<usize, LedgerError> {
        self.run(async move {
            let mut n = 0;
            for pid in self.partitions(PClass::Rel).await? {
                let w = self.any_worker().await?;
                n += w.relationship_count(&pid).await?;
            }
            Ok(n)
        })
    }

    fn vacuum_into(&self, _dest: &std::path::Path) -> Result<(), LedgerError> {
        Err(write_rejected("vacuum_into"))
    }

    fn diff(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<LedgerDiff, LedgerError> {
        self.run(async move {
            let mut merged = LedgerDiff {
                added: Vec::new(),
                touched: Vec::new(),
                unchanged: 0,
            };
            let mut touched: std::collections::BTreeSet<String> = Default::default();
            for m in self.coordinator().await?.catalog(None).await? {
                let w = self.any_worker().await?;
                let d = w.diff(&m.id, from, to).await?;
                merged.added.extend(d.added);
                touched.extend(d.touched);
                merged.unchanged += d.unchanged;
            }
            merged.touched = touched.into_iter().collect();
            Ok(merged)
        })
    }
}
