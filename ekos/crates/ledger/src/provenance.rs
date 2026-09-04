//! RFC 0135 Part B — per-entry write provenance.
//!
//! `LedgerEntry` records *when* a write happened (`written_at`) but never *which pipeline run*
//! produced it. RFC 0004's original design called for a `source_artifact_id` on every entry plus
//! an `audit_trail(id)` reader; it was never built.
//!
//! This module adds the minimum that closes it without a storage-format version bump:
//!
//! - [`WriteContext`] is set on a [`crate::KnowledgeStore`] handle
//!   ([`crate::KnowledgeStore::set_write_context`]) and stamped onto every subsequent write until
//!   changed. `ekos build` / `ekos commit` set it once per stage.
//! - The SQLite backend records it in three nullable `entries` columns added on open
//!   (`ALTER TABLE … ADD COLUMN`); the fact engine appends it to a `provenance.jsonl` sidecar
//!   keyed by transaction id. Both are purely additive — an old store opens unchanged and reads
//!   back `None` provenance for pre-0135 entries.
//! - [`crate::KnowledgeStore::audit_trail`] returns the write history of one entity with the
//!   provenance of each write.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A fresh run id for one `ekos build` / `ekos commit` invocation: `run-<unix-seconds>-<8 hex>`.
/// Sortable by time, unique enough for a single machine's pipeline runs.
pub fn new_run_id() -> String {
    format!(
        "run-{}-{}",
        Utc::now().timestamp(),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    )
}

/// Who is writing — set once per `ekos build` / `ekos commit` stage, stamped onto every entry
/// that stage appends.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WriteContext {
    /// One id per top-level CLI invocation (`ekos build` / `ekos commit`). The command mints it;
    /// every stage of that command shares it, so `audit_trail` can group a run's writes.
    pub run_id: String,
    /// The pipeline stage: `"build"`, `"commit"`, `"commit:rollup"`, `"commit:lineage"`,
    /// `"commit:llm-description"`, `"identity-review"`, `"architecture-review"`, `"test"`, …
    pub stage: String,
    /// The artifact that produced this write, where the caller knows it: the observation
    /// `ArtifactId` for `build`'s `File` objects, the CKM content hash for `commit`. `None` when
    /// the write isn't traceable to a single artifact (per-`KnowledgeArtifact` propagation
    /// through `compile` is a follow-up — RFC 0135 §B scope line).
    pub source_artifact_id: Option<String>,
}

/// One write in an entity's history, with its provenance. Oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    pub written_at: DateTime<Utc>,
    /// `run_id` of the [`WriteContext`] active when this entry was written; `None` for a
    /// pre-0135 entry or a write with no context set.
    pub run_id: Option<String>,
    pub stage: Option<String>,
    pub source_artifact_id: Option<String>,
}
