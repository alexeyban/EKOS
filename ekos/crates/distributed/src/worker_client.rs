//! A newline-delimited-JSON client for a Service B query worker. One held-open TCP connection,
//! one request/response round-trip per call, calls serialised by two `Mutex`es.

use chrono::{DateTime, Utc};
use ekos_kir::{KirEvent, KirEvidence, KirId, KirObject, KirRelationship};
use ekos_ledger::LedgerDiff;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

use crate::DistributedError;
use crate::protocol::{WorkerRequest, WorkerResponse};

pub struct QueryWorkerClient {
    write: Mutex<OwnedWriteHalf>,
    read: Mutex<tokio::io::Lines<BufReader<OwnedReadHalf>>>,
}

macro_rules! expect {
    ($resp:expr, $pat:pat => $out:expr) => {
        match $resp {
            $pat => Ok($out),
            other => Err(DistributedError::Worker(format!("unexpected {other:?}"))),
        }
    };
}

impl QueryWorkerClient {
    pub async fn connect(addr: impl tokio::net::ToSocketAddrs) -> Result<Self, DistributedError> {
        let stream = TcpStream::connect(addr).await?;
        stream.set_nodelay(true).ok();
        let (read, write) = stream.into_split();
        Ok(Self {
            write: Mutex::new(write),
            read: Mutex::new(BufReader::new(read).lines()),
        })
    }

    /// Hold the write lock across the whole round-trip (write **and** read). The gateway fans a
    /// query to many partitions concurrently over one pooled connection per worker; separate
    /// write/read mutexes would let those concurrent calls read each other's response frame.
    pub async fn call(&self, req: &WorkerRequest) -> Result<WorkerResponse, DistributedError> {
        let mut line = serde_json::to_vec(req)?;
        line.push(b'\n');
        let mut w = self.write.lock().await;
        w.write_all(&line).await?;
        w.flush().await?;
        let mut r = self.read.lock().await;
        let resp = r.next_line().await?.ok_or(DistributedError::Closed)?;
        serde_json::from_str::<WorkerResponse>(&resp)?.into_result()
    }

    pub async fn ping(&self) -> Result<(), DistributedError> {
        expect!(self.call(&WorkerRequest::Ping).await?, WorkerResponse::Pong => ())
    }

    pub async fn get_object(
        &self,
        partition: &str,
        id: KirId,
    ) -> Result<Option<KirObject>, DistributedError> {
        expect!(
            self.call(&WorkerRequest::GetObject { partition: partition.into(), id }).await?,
            WorkerResponse::Object(o) => o.map(|b| *b)
        )
    }

    pub async fn get_relationship(
        &self,
        partition: &str,
        id: KirId,
    ) -> Result<Option<KirRelationship>, DistributedError> {
        expect!(
            self.call(&WorkerRequest::GetRelationship { partition: partition.into(), id }).await?,
            WorkerResponse::Relationship(o) => o.map(|b| *b)
        )
    }

    pub async fn get_event(
        &self,
        partition: &str,
        id: KirId,
    ) -> Result<Option<KirEvent>, DistributedError> {
        expect!(
            self.call(&WorkerRequest::GetEvent { partition: partition.into(), id }).await?,
            WorkerResponse::Event(o) => o.map(|b| *b)
        )
    }

    pub async fn get_evidence(
        &self,
        partition: &str,
        id: KirId,
    ) -> Result<Option<KirEvidence>, DistributedError> {
        expect!(
            self.call(&WorkerRequest::GetEvidence { partition: partition.into(), id }).await?,
            WorkerResponse::Evidence(o) => o.map(|b| *b)
        )
    }

    pub async fn object_history(
        &self,
        partition: &str,
        id: KirId,
    ) -> Result<Vec<KirObject>, DistributedError> {
        expect!(
            self.call(&WorkerRequest::ObjectHistory { partition: partition.into(), id }).await?,
            WorkerResponse::Objects(v) => v
        )
    }

    pub async fn relationship_history(
        &self,
        partition: &str,
        id: KirId,
    ) -> Result<Vec<KirRelationship>, DistributedError> {
        expect!(
            self.call(&WorkerRequest::RelationshipHistory { partition: partition.into(), id }).await?,
            WorkerResponse::Relationships(v) => v
        )
    }

    pub async fn relationships_for(
        &self,
        partition: &str,
        id: KirId,
    ) -> Result<Vec<KirRelationship>, DistributedError> {
        expect!(
            self.call(&WorkerRequest::RelationshipsFor { partition: partition.into(), id }).await?,
            WorkerResponse::Relationships(v) => v
        )
    }

    pub async fn all_objects(&self, partition: &str) -> Result<Vec<KirObject>, DistributedError> {
        expect!(
            self.call(&WorkerRequest::AllObjects { partition: partition.into() }).await?,
            WorkerResponse::Objects(v) => v
        )
    }

    pub async fn all_relationships(
        &self,
        partition: &str,
    ) -> Result<Vec<KirRelationship>, DistributedError> {
        expect!(
            self.call(&WorkerRequest::AllRelationships { partition: partition.into() }).await?,
            WorkerResponse::Relationships(v) => v
        )
    }

    pub async fn object_at(
        &self,
        partition: &str,
        id: KirId,
        at: DateTime<Utc>,
    ) -> Result<Option<KirObject>, DistributedError> {
        expect!(
            self.call(&WorkerRequest::ObjectAt { partition: partition.into(), id, at }).await?,
            WorkerResponse::Object(o) => o.map(|b| *b)
        )
    }

    pub async fn relationships_at(
        &self,
        partition: &str,
        id: KirId,
        at: DateTime<Utc>,
    ) -> Result<Vec<KirRelationship>, DistributedError> {
        expect!(
            self.call(&WorkerRequest::RelationshipsAt { partition: partition.into(), id, at }).await?,
            WorkerResponse::Relationships(v) => v
        )
    }

    pub async fn all_objects_at(
        &self,
        partition: &str,
        at: DateTime<Utc>,
    ) -> Result<Vec<KirObject>, DistributedError> {
        expect!(
            self.call(&WorkerRequest::AllObjectsAt { partition: partition.into(), at }).await?,
            WorkerResponse::Objects(v) => v
        )
    }

    pub async fn all_relationships_at(
        &self,
        partition: &str,
        at: DateTime<Utc>,
    ) -> Result<Vec<KirRelationship>, DistributedError> {
        expect!(
            self.call(&WorkerRequest::AllRelationshipsAt { partition: partition.into(), at }).await?,
            WorkerResponse::Relationships(v) => v
        )
    }

    pub async fn find_objects(
        &self,
        partition: &str,
        query: &str,
    ) -> Result<Vec<(KirId, String)>, DistributedError> {
        expect!(
            self.call(&WorkerRequest::FindObjects { partition: partition.into(), query: query.into() }).await?,
            WorkerResponse::FindHits(v) => v
        )
    }

    /// This shard's BM25 top-`k` for `query`, each hit with its shard-local score (RFC 0113 B5).
    pub async fn find_objects_scored(
        &self,
        partition: &str,
        query: &str,
        k: usize,
    ) -> Result<Vec<(KirId, String, f32)>, DistributedError> {
        expect!(
            self.call(&WorkerRequest::FindObjectsScored { partition: partition.into(), query: query.into(), k }).await?,
            WorkerResponse::ScoredHits(v) => v
        )
    }

    pub async fn diff(
        &self,
        partition: &str,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<LedgerDiff, DistributedError> {
        expect!(
            self.call(&WorkerRequest::Diff { partition: partition.into(), from, to }).await?,
            WorkerResponse::Diff(d) => d.into()
        )
    }

    pub async fn object_count(&self, partition: &str) -> Result<usize, DistributedError> {
        expect!(
            self.call(&WorkerRequest::ObjectCount { partition: partition.into() }).await?,
            WorkerResponse::Count(n) => n
        )
    }

    pub async fn relationship_count(&self, partition: &str) -> Result<usize, DistributedError> {
        expect!(
            self.call(&WorkerRequest::RelationshipCount { partition: partition.into() }).await?,
            WorkerResponse::Count(n) => n
        )
    }

    pub async fn entry_count(&self, partition: &str) -> Result<usize, DistributedError> {
        expect!(
            self.call(&WorkerRequest::EntryCount { partition: partition.into() }).await?,
            WorkerResponse::Count(n) => n
        )
    }
}
