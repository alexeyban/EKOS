//! The Service B (query worker) RPC surface — one JSON [`WorkerRequest`] per line in, one
//! [`WorkerResponse`] per line out. Every request names the `partition` it targets; the worker
//! materialises + opens that partition on demand.

use chrono::{DateTime, Utc};
use ekos_kir::{KirEvent, KirEvidence, KirId, KirObject, KirRelationship};
use ekos_ledger::{LedgerDiff, LedgerEntryId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "op")]
pub enum WorkerRequest {
    Ping,
    GetObject {
        partition: String,
        id: KirId,
    },
    GetRelationship {
        partition: String,
        id: KirId,
    },
    GetEvent {
        partition: String,
        id: KirId,
    },
    GetEvidence {
        partition: String,
        id: KirId,
    },
    ObjectHistory {
        partition: String,
        id: KirId,
    },
    RelationshipHistory {
        partition: String,
        id: KirId,
    },
    RelationshipsFor {
        partition: String,
        id: KirId,
    },
    AllObjects {
        partition: String,
    },
    AllRelationships {
        partition: String,
    },
    ObjectAt {
        partition: String,
        id: KirId,
        at: DateTime<Utc>,
    },
    RelationshipsAt {
        partition: String,
        id: KirId,
        at: DateTime<Utc>,
    },
    AllObjectsAt {
        partition: String,
        at: DateTime<Utc>,
    },
    AllRelationshipsAt {
        partition: String,
        at: DateTime<Utc>,
    },
    FindObjects {
        partition: String,
        query: String,
    },
    /// RFC 0113 B5 — this shard's BM25 top-`k` for `query`, each hit with its (shard-local) score.
    FindObjectsScored {
        partition: String,
        query: String,
        k: usize,
    },
    Diff {
        partition: String,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    },
    ObjectCount {
        partition: String,
    },
    RelationshipCount {
        partition: String,
    },
    EntryCount {
        partition: String,
    },
    /// RFC 0136 Phase 7 — closes the one remaining `Err`-stubbed `KnowledgeStore` method on
    /// `DistributedLedger`; same shape as `ObjectCount`/`RelationshipCount`.
    EvidenceCount {
        partition: String,
    },
}

impl WorkerRequest {
    pub fn partition(&self) -> Option<&str> {
        match self {
            WorkerRequest::Ping => None,
            WorkerRequest::GetObject { partition, .. }
            | WorkerRequest::GetRelationship { partition, .. }
            | WorkerRequest::GetEvent { partition, .. }
            | WorkerRequest::GetEvidence { partition, .. }
            | WorkerRequest::ObjectHistory { partition, .. }
            | WorkerRequest::RelationshipHistory { partition, .. }
            | WorkerRequest::RelationshipsFor { partition, .. }
            | WorkerRequest::AllObjects { partition }
            | WorkerRequest::AllRelationships { partition }
            | WorkerRequest::ObjectAt { partition, .. }
            | WorkerRequest::RelationshipsAt { partition, .. }
            | WorkerRequest::AllObjectsAt { partition, .. }
            | WorkerRequest::AllRelationshipsAt { partition, .. }
            | WorkerRequest::FindObjects { partition, .. }
            | WorkerRequest::FindObjectsScored { partition, .. }
            | WorkerRequest::Diff { partition, .. }
            | WorkerRequest::ObjectCount { partition }
            | WorkerRequest::RelationshipCount { partition }
            | WorkerRequest::EntryCount { partition }
            | WorkerRequest::EvidenceCount { partition } => Some(partition),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "status", content = "data")]
pub enum WorkerResponse {
    Pong,
    Object(Option<Box<KirObject>>),
    Relationship(Option<Box<KirRelationship>>),
    Event(Option<Box<KirEvent>>),
    Evidence(Option<Box<KirEvidence>>),
    Objects(Vec<KirObject>),
    Relationships(Vec<KirRelationship>),
    FindHits(Vec<(KirId, String)>),
    ScoredHits(Vec<(KirId, String, f32)>),
    Diff(DiffWire),
    Count(usize),
    Error { message: String },
}

impl WorkerResponse {
    pub fn into_result(self) -> Result<Self, crate::DistributedError> {
        match self {
            WorkerResponse::Error { message } => Err(crate::DistributedError::Worker(message)),
            other => Ok(other),
        }
    }
}

/// Wire form of [`LedgerDiff`] (which isn't `Serialize`). `added` carries the raw version-row ids
/// as `i64` — only meaningful within one partition, so the gateway sums their count across
/// partitions rather than trying to globally order them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffWire {
    pub added: Vec<i64>,
    pub touched: Vec<String>,
    pub unchanged: usize,
}

impl From<LedgerDiff> for DiffWire {
    fn from(d: LedgerDiff) -> Self {
        Self {
            added: d.added.into_iter().map(|e| e.0).collect(),
            touched: d.touched,
            unchanged: d.unchanged,
        }
    }
}

impl From<DiffWire> for LedgerDiff {
    fn from(d: DiffWire) -> Self {
        LedgerDiff {
            added: d.added.into_iter().map(LedgerEntryId).collect(),
            touched: d.touched,
            unchanged: d.unchanged,
        }
    }
}
