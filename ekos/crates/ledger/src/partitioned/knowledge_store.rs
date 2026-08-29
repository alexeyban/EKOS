//! `PartitionedLedger` as a drop-in [`KnowledgeStore`] (RFC 0111 amendment 2026-08-29 §4).
//!
//! `PartitionError` maps to `LedgerError` via `From`: a wrapped `Ledger` error is unwrapped;
//! anything else becomes `Corrupt`.

use super::*;

impl From<PartitionError> for LedgerError {
    fn from(e: PartitionError) -> Self {
        match e {
            PartitionError::Ledger { source, .. } => source,
            other => LedgerError::Corrupt(other.to_string()),
        }
    }
}

impl KnowledgeStore for PartitionedLedger {
    fn append_object(&self, obj: &KirObject) -> Result<bool, LedgerError> {
        Ok(PartitionedLedger::append_object(self, obj)?)
    }
    fn append_evidence(&self, ev: &KirEvidence) -> Result<(), LedgerError> {
        Ok(PartitionedLedger::append_evidence(self, ev)?)
    }
    fn append_relationship(&self, rel: &KirRelationship) -> Result<bool, LedgerError> {
        Ok(PartitionedLedger::append_relationship(self, rel)?)
    }
    fn append_event(&self, ev: &KirEvent) -> Result<(), LedgerError> {
        Ok(PartitionedLedger::append_event(self, ev)?)
    }
    fn get_object(&self, id: &KirId) -> Result<Option<KirObject>, LedgerError> {
        Ok(PartitionedLedger::get_object(self, id)?)
    }
    fn get_evidence(&self, id: &KirId) -> Result<Option<KirEvidence>, LedgerError> {
        Ok(PartitionedLedger::get_evidence(self, id)?)
    }
    fn get_relationship(&self, id: &KirId) -> Result<Option<KirRelationship>, LedgerError> {
        Ok(PartitionedLedger::get_relationship(self, id)?)
    }
    fn get_event(&self, id: &KirId) -> Result<Option<KirEvent>, LedgerError> {
        Ok(PartitionedLedger::get_event(self, id)?)
    }
    fn all_objects(&self) -> Result<Vec<KirObject>, LedgerError> {
        Ok(PartitionedLedger::all_objects(self)?)
    }
    fn all_relationships(&self) -> Result<Vec<KirRelationship>, LedgerError> {
        Ok(PartitionedLedger::all_relationships(self)?)
    }
    fn relationships_for(&self, id: &KirId) -> Result<Vec<KirRelationship>, LedgerError> {
        Ok(PartitionedLedger::relationships_for(self, id)?)
    }
    fn object_at(&self, id: &KirId, at: DateTime<Utc>) -> Result<Option<KirObject>, LedgerError> {
        Ok(PartitionedLedger::object_at(self, id, at)?)
    }
    fn relationships_at(
        &self,
        id: &KirId,
        at: DateTime<Utc>,
    ) -> Result<Vec<KirRelationship>, LedgerError> {
        Ok(PartitionedLedger::relationships_at(self, id, at)?)
    }
    fn all_objects_at(&self, at: DateTime<Utc>) -> Result<Vec<KirObject>, LedgerError> {
        Ok(PartitionedLedger::all_objects_at(self, at)?)
    }
    fn all_relationships_at(&self, at: DateTime<Utc>) -> Result<Vec<KirRelationship>, LedgerError> {
        Ok(PartitionedLedger::all_relationships_at(self, at)?)
    }
    fn object_history(&self, id: &KirId) -> Result<Vec<KirObject>, LedgerError> {
        Ok(PartitionedLedger::object_history(self, id)?)
    }
    fn relationship_history(&self, id: &KirId) -> Result<Vec<KirRelationship>, LedgerError> {
        Ok(PartitionedLedger::relationship_history(self, id)?)
    }
    fn find_objects(&self, query: &str) -> Result<Vec<(KirId, String)>, LedgerError> {
        Ok(PartitionedLedger::find_objects(self, query)?)
    }
    fn entry_count(&self) -> Result<usize, LedgerError> {
        Ok(PartitionedLedger::entry_count(self)?)
    }
    fn object_count(&self) -> Result<usize, LedgerError> {
        Ok(PartitionedLedger::object_count(self)?)
    }
    fn relationship_count(&self) -> Result<usize, LedgerError> {
        Ok(PartitionedLedger::relationship_count(self)?)
    }
    fn vacuum_into(&self, dest: &Path) -> Result<(), LedgerError> {
        Ok(PartitionedLedger::vacuum_into(self, dest)?)
    }
    fn diff(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<LedgerDiff, LedgerError> {
        Ok(PartitionedLedger::diff(self, from, to)?)
    }
}
