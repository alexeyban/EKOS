//! Service C — the query gateway (RFC 0113 B4b).
//!
//! [`DistributedLedger`] implements [`KnowledgeStore`] by fanning reads across the [`QueryWorker`]s
//! the coordinator knows about and merging the results, so every existing `KnowledgeStore` caller
//! (Runtime, MCP, `docs-gen`) works unchanged against a cluster. It holds no partition data.
//! **Writes are rejected** — `ekos build` / `commit` in Distributed mode is Service A
//! (`ekos compile-worker`), and the gateway is read-only, matching the Runtime-is-read-only
//! invariant.
//!
//! v1.1: a small connection pool (one cached connection per coordinator/worker address, dropped
//! and reconnected on an I/O error) replaces v1's fresh-connect-per-call; multi-partition fan-out
//! (`all_objects`, `diff`, `search`, id-scoped lookups, …) dispatches to every partition
//! **concurrently** instead of sequentially; and id-scoped reads (`get_object`, `object_history`,
//! …) prune to the partitions the coordinator's `entity_id → partitions` index names for that id,
//! set by `ekos compile-worker` after each compile, falling back to a full class scan when the
//! index has nothing for an id (unindexed classes — events/evidence — or a not-yet-recompiled
//! workspace). Broad reads with no id to prune by (`all_objects`, `relationships_for`, `diff`, …)
//! still fan to every partition of the right class — that's inherent to what they're asking for,
//! not a pruning gap.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{DateTime, Utc};
use ekos_cluster::{ClusterError, CoordinatorClient};
use ekos_kir::{KirEvent, KirEvidence, KirId, KirObject, KirRelationship};
use ekos_ledger::{
    ArmSet, KnowledgeStore, LedgerDiff, LedgerError, RRF_K, RankedResults, RetrievalRequest,
    ScoredCandidate, SignalSource, exact_name_matches, rrf_fuse,
};
use futures::future::{join_all, try_join_all};
use tokio::runtime::Handle;
use tokio::sync::Mutex as AsyncMutex;

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

/// Sort partition ids newest-bucket-first — the order every id-scoped "first match wins" read
/// (`get_object`, `object_at`, …) depends on.
fn sort_newest_first(ids: &mut [String]) {
    ids.sort_by(|a, b| bucket(b).cmp(bucket(a)).then_with(|| b.cmp(a)));
}

/// A connection that's reconnected on demand rather than held forever — cheap for a
/// cluster-internal TCP connection, and simpler than a real health-checked pool for v1.1.
struct ConnSlot<C>(AsyncMutex<Option<Arc<C>>>);

impl<C> ConnSlot<C> {
    fn empty() -> Self {
        Self(AsyncMutex::new(None))
    }

    async fn get(&self) -> Option<Arc<C>> {
        self.0.lock().await.clone()
    }

    async fn set(&self, c: Arc<C>) {
        *self.0.lock().await = Some(c);
    }

    async fn clear(&self) {
        *self.0.lock().await = None;
    }
}

pub struct DistributedLedger {
    coordinator_addr: String,
    worker_addrs: Vec<String>,
    next_worker: AtomicUsize,
    coordinator_conn: ConnSlot<CoordinatorClient>,
    worker_conns: Vec<ConnSlot<QueryWorkerClient>>,
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

fn is_conn_error(e: &ClusterError) -> bool {
    matches!(e, ClusterError::Io(_) | ClusterError::Closed)
}

fn is_worker_conn_error(e: &DistributedError) -> bool {
    matches!(e, DistributedError::Io(_) | DistributedError::Closed)
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
        let worker_conns = worker_addrs.iter().map(|_| ConnSlot::empty()).collect();
        Ok(Self {
            coordinator_addr,
            worker_addrs,
            next_worker: AtomicUsize::new(0),
            coordinator_conn: ConnSlot::empty(),
            worker_conns,
        })
    }

    fn block_on<F: Future>(&self, fut: F) -> F::Output {
        block_on_sync(fut)
    }

    // ── connection pool ────────────────────────────────────────────────────

    async fn reconnect_coordinator(&self) -> Result<Arc<CoordinatorClient>, DistributedError> {
        let c = Arc::new(CoordinatorClient::connect(&self.coordinator_addr).await?);
        self.coordinator_conn.set(Arc::clone(&c)).await;
        Ok(c)
    }

    async fn coordinator(&self) -> Result<Arc<CoordinatorClient>, DistributedError> {
        match self.coordinator_conn.get().await {
            Some(c) => Ok(c),
            None => self.reconnect_coordinator().await,
        }
    }

    /// Run `f` against the pooled coordinator connection, reconnecting and retrying once if the
    /// pooled connection turned out to be dead (the peer closed it, a prior call's I/O failed).
    async fn call_coordinator<T, F, Fut>(&self, f: F) -> Result<T, DistributedError>
    where
        F: Fn(Arc<CoordinatorClient>) -> Fut,
        Fut: Future<Output = Result<T, ClusterError>>,
    {
        let c = self.coordinator().await?;
        match f(c).await {
            Err(e) if is_conn_error(&e) => {
                self.coordinator_conn.clear().await;
                let c = self.reconnect_coordinator().await?;
                Ok(f(c).await?)
            }
            Ok(v) => Ok(v),
            Err(e) => Err(e.into()),
        }
    }

    fn next_index(&self) -> usize {
        self.next_worker.fetch_add(1, Ordering::Relaxed) % self.worker_addrs.len()
    }

    async fn reconnect_worker(
        &self,
        idx: usize,
    ) -> Result<Arc<QueryWorkerClient>, DistributedError> {
        let c = Arc::new(QueryWorkerClient::connect(&self.worker_addrs[idx]).await?);
        self.worker_conns[idx].set(Arc::clone(&c)).await;
        Ok(c)
    }

    async fn worker(&self, idx: usize) -> Result<Arc<QueryWorkerClient>, DistributedError> {
        match self.worker_conns[idx].get().await {
            Some(c) => Ok(c),
            None => self.reconnect_worker(idx).await,
        }
    }

    /// Run `f` against the pooled connection for worker `idx`, reconnecting and retrying once on a
    /// dead pooled connection — the same one-retry contract as [`Self::call_coordinator`].
    async fn call_worker<T, F, Fut>(&self, idx: usize, f: F) -> Result<T, DistributedError>
    where
        F: Fn(Arc<QueryWorkerClient>) -> Fut,
        Fut: Future<Output = Result<T, DistributedError>>,
    {
        let w = self.worker(idx).await?;
        match f(w).await {
            Err(e) if is_worker_conn_error(&e) => {
                let w = self.reconnect_worker(idx).await?;
                f(w).await
            }
            other => other,
        }
    }

    /// Like [`Self::call_worker`], but on a **connection** failure (worker process down, peer
    /// closed, connect refused) move on to the next worker in rotation and try there — every query
    /// worker can materialise and serve any partition (RFC 0113 B4), so a dead worker is not a
    /// dead partition. Only connection errors trigger failover; a real ledger/protocol error from
    /// a reachable worker is returned as-is. Fails only when *no* worker could serve the call.
    async fn call_worker_failover<T, F, Fut>(
        &self,
        start: usize,
        f: F,
    ) -> Result<T, DistributedError>
    where
        F: Fn(Arc<QueryWorkerClient>) -> Fut,
        Fut: Future<Output = Result<T, DistributedError>>,
    {
        let n = self.worker_addrs.len();
        let mut last: Option<DistributedError> = None;
        for offset in 0..n {
            let idx = (start + offset) % n;
            match self.call_worker(idx, &f).await {
                Err(e) if is_worker_conn_error(&e) => {
                    self.worker_conns[idx].clear().await;
                    if offset + 1 < n {
                        tracing::warn!(worker = %self.worker_addrs[idx], %e, "query worker unreachable — failing over");
                    }
                    last = Some(e);
                }
                other => return other,
            }
        }
        Err(last.unwrap_or_else(|| {
            DistributedError::Other("no query workers reachable for this call".into())
        }))
    }

    // ── partition discovery / pruning ──────────────────────────────────────

    /// Catalogued partition ids of one class, newest bucket first.
    async fn partitions(&self, class: PClass) -> Result<Vec<String>, DistributedError> {
        let mut ids: Vec<String> = self
            .call_coordinator(|c| async move { c.catalog(None).await })
            .await?
            .into_iter()
            .map(|m| m.id)
            .filter(|id| classify(id) == class)
            .collect();
        sort_newest_first(&mut ids);
        Ok(ids)
    }

    /// Partitions worth asking for `id`: the coordinator's `entity_id → partitions` index (RFC
    /// 0113 v1.1, populated by `ekos compile-worker` for objects/relationships) narrowed to
    /// `class`, or — if the index has nothing (an unindexed class, or a workspace compiled before
    /// this landed) — every partition of `class`, same as v1.
    async fn candidate_partitions(
        &self,
        class: PClass,
        id: KirId,
    ) -> Result<Vec<String>, DistributedError> {
        let entity = id.to_string();
        let indexed = self
            .call_coordinator(move |c| {
                let entity = entity.clone();
                async move { c.partitions_for_entity(&entity).await }
            })
            .await?;
        let mut indexed: Vec<String> = indexed
            .into_iter()
            .filter(|p| classify(p) == class)
            .collect();
        if indexed.is_empty() {
            return self.partitions(class).await;
        }
        sort_newest_first(&mut indexed);
        Ok(indexed)
    }

    // ── concurrent fan-out ──────────────────────────────────────────────────

    /// Dispatch `call` to every partition in `pids` concurrently (round-robin across workers),
    /// returning their results in the same order as `pids`. Any single failure fails the whole
    /// fan-out, same as the sequential loop it replaces.
    async fn fan_out<T, F, Fut>(&self, pids: &[String], call: F) -> Result<Vec<T>, DistributedError>
    where
        F: Fn(Arc<QueryWorkerClient>, String) -> Fut,
        Fut: Future<Output = Result<T, DistributedError>>,
    {
        let call = &call;
        let futs = pids.iter().map(|pid| {
            let idx = self.next_index();
            let pid = pid.clone();
            async move {
                self.call_worker_failover(idx, |w| call(w, pid.clone()))
                    .await
            }
        });
        try_join_all(futs).await
    }

    /// Fan `call` (an id-scoped `Option<T>` lookup) to every candidate partition for `id`
    /// concurrently, then return the first `Some` in `candidate_partitions`' priority order
    /// (newest-first for objects; whatever order the index/class-scan produced otherwise) — same
    /// "first match wins" semantics as the old sequential loop, just not serialised over the wire.
    async fn first_present<T, F, Fut>(
        &self,
        class: PClass,
        id: KirId,
        call: F,
    ) -> Result<Option<T>, DistributedError>
    where
        F: Fn(Arc<QueryWorkerClient>, String) -> Fut,
        Fut: Future<Output = Result<Option<T>, DistributedError>>,
    {
        let pids = self.candidate_partitions(class, id).await?;
        let call = &call;
        let futs = pids.iter().map(|pid| {
            let idx = self.next_index();
            let pid = pid.clone();
            async move {
                self.call_worker_failover(idx, |w| call(w, pid.clone()))
                    .await
            }
        });
        for res in join_all(futs).await {
            if let Some(v) = res? {
                return Ok(Some(v));
            }
        }
        Ok(None)
    }

    /// RFC 0113 B5 + RFC 0120 — distributed search. Fans each object partition's BM25 **top-`k`**
    /// to a query worker, then **RRF-merges** the per-shard ranked lists (plus an `ExactName`
    /// list over the union) into one global order. RRF ranks rather than comparing the
    /// per-partition (shard-local IDF) BM25 magnitudes the old merge sorted on — a defensible
    /// improvement, still not a corpus-global ranking (RFC 0111 §7).
    pub fn search(&self, query: &str, k: usize) -> Result<Vec<(KirId, String, f32)>, LedgerError> {
        Ok(self
            .search_ranked(query, query, k)?
            .hits
            .into_iter()
            .map(|h| (h.id, h.name, h.score))
            .collect())
    }

    /// Shared body of [`Self::search`] and the `retrieve` trait impl. `bm25_q` drives the
    /// per-partition fan-out; `exact_q` is matched (case-insensitively, trimmed) for the
    /// `ExactName` arm.
    fn search_ranked(
        &self,
        bm25_q: &str,
        exact_q: &str,
        k: usize,
    ) -> Result<RankedResults, LedgerError> {
        let bm25_q = bm25_q.to_string();
        let exact_q = exact_q.to_string();
        self.run(async move {
            let pids = self.partitions(PClass::Object).await?;
            let per_partition = self
                .fan_out(&pids, |w, pid| {
                    let q = bm25_q.clone();
                    async move { w.find_objects_scored(&pid, &q, k).await }
                })
                .await?;
            let union: Vec<ScoredCandidate> = per_partition
                .iter()
                .flatten()
                .map(|(id, name, score)| ScoredCandidate::new(*id, name.clone(), *score))
                .collect();
            let mut lists: Vec<(SignalSource, Vec<ScoredCandidate>)> = per_partition
                .into_iter()
                .map(|hits| {
                    (
                        SignalSource::Bm25,
                        hits.into_iter()
                            .map(|(id, name, score)| ScoredCandidate::new(id, name, score))
                            .collect(),
                    )
                })
                .collect();
            lists.push((
                SignalSource::ExactName,
                exact_name_matches(&exact_q, &union),
            ));
            Ok::<_, DistributedError>(RankedResults {
                hits: rrf_fuse(&lists, RRF_K, k),
                arms_run: ArmSet::LEXICAL,
            })
        })
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
        self.run(
            self.first_present(PClass::Object, id, move |w, pid| async move {
                w.get_object(&pid, id).await
            }),
        )
    }

    fn get_relationship(&self, id: &KirId) -> Result<Option<KirRelationship>, LedgerError> {
        let id = *id;
        self.run(
            self.first_present(PClass::Rel, id, move |w, pid| async move {
                w.get_relationship(&pid, id).await
            }),
        )
    }

    fn get_event(&self, id: &KirId) -> Result<Option<KirEvent>, LedgerError> {
        let id = *id;
        self.run(
            self.first_present(PClass::Event, id, move |w, pid| async move {
                w.get_event(&pid, id).await
            }),
        )
    }

    fn get_evidence(&self, id: &KirId) -> Result<Option<KirEvidence>, LedgerError> {
        let id = *id;
        self.run(
            self.first_present(PClass::Evidence, id, move |w, pid| async move {
                w.get_evidence(&pid, id).await
            }),
        )
    }

    fn all_objects(&self) -> Result<Vec<KirObject>, LedgerError> {
        self.run(async move {
            let mut by_id: std::collections::HashMap<KirId, KirObject> = Default::default();
            // oldest bucket first, so a newer partition's version overwrites
            let mut pids = self.partitions(PClass::Object).await?;
            pids.reverse();
            let per_partition = self
                .fan_out(&pids, |w, pid| async move { w.all_objects(&pid).await })
                .await?;
            for objs in per_partition {
                for o in objs {
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
            let per_partition = self
                .fan_out(
                    &pids,
                    |w, pid| async move { w.all_relationships(&pid).await },
                )
                .await?;
            for rels in per_partition {
                for r in rels {
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
            // No endpoint index exists (RFC 0113 v1.1 only indexes an entity's own id) — a
            // relationship referencing `id` as an endpoint can live in any rel-kind partition, so
            // this always fans to every one, just concurrently now instead of sequentially.
            let mut pids = self.partitions(PClass::Rel).await?;
            pids.reverse();
            let per_partition = self
                .fan_out(&pids, move |w, pid| async move {
                    w.relationships_for(&pid, id).await
                })
                .await?;
            for rels in per_partition {
                for r in rels {
                    by_id.insert(r.id, r);
                }
            }
            Ok(by_id.into_values().collect())
        })
    }

    fn object_at(&self, id: &KirId, at: DateTime<Utc>) -> Result<Option<KirObject>, LedgerError> {
        let id = *id;
        self.run(
            self.first_present(PClass::Object, id, move |w, pid| async move {
                w.object_at(&pid, id, at).await
            }),
        )
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
            let per_partition = self
                .fan_out(&pids, move |w, pid| async move {
                    w.relationships_at(&pid, id, at).await
                })
                .await?;
            for rels in per_partition {
                for r in rels {
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
            let per_partition = self
                .fan_out(&pids, move |w, pid| async move {
                    w.all_objects_at(&pid, at).await
                })
                .await?;
            for objs in per_partition {
                for o in objs {
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
            let per_partition = self
                .fan_out(&pids, move |w, pid| async move {
                    w.all_relationships_at(&pid, at).await
                })
                .await?;
            for rels in per_partition {
                for r in rels {
                    by_id.insert(r.id, r);
                }
            }
            Ok(by_id.into_values().collect())
        })
    }

    fn object_history(&self, id: &KirId) -> Result<Vec<KirObject>, LedgerError> {
        let id = *id;
        self.run(async move {
            let mut pids = self.candidate_partitions(PClass::Object, id).await?;
            pids.reverse(); // oldest bucket first
            let per_partition = self
                .fan_out(&pids, move |w, pid| async move {
                    w.object_history(&pid, id).await
                })
                .await?;
            Ok(per_partition.into_iter().flatten().collect())
        })
    }

    fn relationship_history(&self, id: &KirId) -> Result<Vec<KirRelationship>, LedgerError> {
        let id = *id;
        self.run(async move {
            let mut pids = self.candidate_partitions(PClass::Rel, id).await?;
            pids.reverse();
            let per_partition = self
                .fan_out(&pids, move |w, pid| async move {
                    w.relationship_history(&pid, id).await
                })
                .await?;
            Ok(per_partition.into_iter().flatten().collect())
        })
    }

    fn find_objects(&self, query: &str) -> Result<Vec<(KirId, String)>, LedgerError> {
        Ok(self
            .search(query, 50)?
            .into_iter()
            .map(|(id, name, _)| (id, name))
            .collect())
    }

    fn retrieve(&self, req: &RetrievalRequest) -> Result<RankedResults, LedgerError> {
        // RFC 0120: BM25 fan-out + ExactName, RRF-merged across shards.
        self.search_ranked(req.bm25_query(), &req.raw, req.limit)
    }

    fn entry_count(&self) -> Result<usize, LedgerError> {
        self.run(async move {
            let cat = self
                .call_coordinator(|c| async move { c.catalog(None).await })
                .await?;
            let pids: Vec<String> = cat.into_iter().map(|m| m.id).collect();
            let counts = self
                .fan_out(&pids, |w, pid| async move { w.entry_count(&pid).await })
                .await?;
            Ok(counts.into_iter().sum())
        })
    }

    fn object_count(&self) -> Result<usize, LedgerError> {
        self.run(async move {
            let pids = self.partitions(PClass::Object).await?;
            let counts = self
                .fan_out(&pids, |w, pid| async move { w.object_count(&pid).await })
                .await?;
            Ok(counts.into_iter().sum())
        })
    }

    fn relationship_count(&self) -> Result<usize, LedgerError> {
        self.run(async move {
            let pids = self.partitions(PClass::Rel).await?;
            let counts = self
                .fan_out(
                    &pids,
                    |w, pid| async move { w.relationship_count(&pid).await },
                )
                .await?;
            Ok(counts.into_iter().sum())
        })
    }

    fn vacuum_into(&self, _dest: &std::path::Path) -> Result<(), LedgerError> {
        Err(write_rejected("vacuum_into"))
    }

    fn diff(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<LedgerDiff, LedgerError> {
        self.run(async move {
            let cat = self
                .call_coordinator(|c| async move { c.catalog(None).await })
                .await?;
            let pids: Vec<String> = cat.into_iter().map(|m| m.id).collect();
            let diffs = self
                .fan_out(
                    &pids,
                    move |w, pid| async move { w.diff(&pid, from, to).await },
                )
                .await?;
            let mut merged = LedgerDiff {
                added: Vec::new(),
                touched: Vec::new(),
                unchanged: 0,
            };
            let mut touched: std::collections::BTreeSet<String> = Default::default();
            for d in diffs {
                merged.added.extend(d.added);
                touched.extend(d.touched);
                merged.unchanged += d.unchanged;
            }
            merged.touched = touched.into_iter().collect();
            Ok(merged)
        })
    }
}
