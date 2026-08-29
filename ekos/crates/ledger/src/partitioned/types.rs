//! Value types for the partitioned ledger: routing keys, the persisted catalog, tiers, errors.

use crate::LedgerError;
use chrono::{DateTime, Utc};
use ekos_kir::KirId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PartitionError {
    #[error("ledger error in partition {key:?}: {source}")]
    Ledger {
        key: PartitionKey,
        #[source]
        source: LedgerError,
    },
    #[error("partition catalog I/O error at {path}: {source}")]
    Catalog {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("partition catalog at {path} is corrupt: {source}")]
    CatalogParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "SourceScope/Composite routing needs a source for entity {entity}, but the source \
         resolver returned None — set one with PartitionedLedger::with_source_resolver"
    )]
    UnresolvedSource { entity: KirId },
    #[error(
        "this ledger's partitions were created with {field} = {stored:?}, but it is being opened \
         with {requested:?} — routing/tiering config cannot change after partitions exist"
    )]
    DimensionMismatch {
        field: &'static str,
        stored: String,
        requested: String,
    },
}

/// RFC 0111 §1's routing dimension. All three variants route; `SourceScope` and `Composite`
/// require a source resolver ([`PartitionedLedger::with_source_resolver`]) since a `KirObject`
/// carries no explicit source field yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartitionDimension {
    /// Partition by `ObjectKind` — e.g. `Table`, `File`, `Custom("Risk")`.
    EntityKind,
    /// Partition by the object's originating source/connector — e.g. `sql`, `git`,
    /// `discord:#governance` — as returned by the source resolver.
    SourceScope,
    /// Partition by `source` + `kind` together (`"<source>\u{1f}<kind>"`); more partitions, so a
    /// scoped query prunes to an exact composite value (prefix scoping is a later refinement).
    Composite,
}

impl PartitionDimension {
    pub fn as_str(&self) -> &'static str {
        match self {
            PartitionDimension::EntityKind => "entity-kind",
            PartitionDimension::SourceScope => "source-scope",
            PartitionDimension::Composite => "composite",
        }
    }

    /// Parse the `ekos.toml` `[storage.partition] dimension` string.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "entity-kind" => Some(PartitionDimension::EntityKind),
            "source-scope" => Some(PartitionDimension::SourceScope),
            "composite" => Some(PartitionDimension::Composite),
            _ => None,
        }
    }
}

/// RFC 0111 §1's time-bucket granularity. Labels are chosen so that **lexical order equals
/// chronological order** — [`PartitionKey`]'s derived `Ord` relies on this to merge partitions in
/// the correct order in [`PartitionedLedger::object_history`] without a separate timestamp
/// comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TimeBucket {
    Daily,
    Weekly,
    #[default]
    Monthly,
}

impl TimeBucket {
    /// The bucket label for a timestamp — the string that becomes [`PartitionKey::time_bucket`].
    pub fn label(&self, at: DateTime<Utc>) -> String {
        match self {
            // ISO-8601 forms, all lexically == chronologically ordered.
            TimeBucket::Daily => at.format("%Y-%m-%d").to_string(),
            TimeBucket::Weekly => at.format("%G-W%V").to_string(),
            TimeBucket::Monthly => at.format("%Y-%m").to_string(),
        }
    }

    /// Parse the `ekos.toml` `[storage.partition] time-bucket` string. `None` for an unknown value
    /// (caller decides whether to warn-and-default or error).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "daily" => Some(TimeBucket::Daily),
            "weekly" => Some(TimeBucket::Weekly),
            "monthly" => Some(TimeBucket::Monthly),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TimeBucket::Daily => "daily",
            TimeBucket::Weekly => "weekly",
            TimeBucket::Monthly => "monthly",
        }
    }
}

/// `(time_bucket, dimension_value)` — field order matters: it makes the derived `Ord` sort
/// chronologically first, which [`PartitionedLedger::object_history`] relies on to merge partitions
/// in the correct order without needing a separate timestamp comparison.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PartitionKey {
    /// e.g. `"2026-08"` (monthly), `"2026-W35"` (weekly), `"2026-08-27"` (daily).
    pub time_bucket: String,
    /// e.g. `"Table"`, `"File"` (an `ObjectKind`'s `Display` output) for `EntityKind` routing.
    pub dimension_value: String,
}

/// RFC 0111 §3 hot/cold tier. **Hot**: kept in the open-handle cache, eligible for indexing, lives
/// on local disk. **Cold**: an aged-out, sealed partition — its handle is evicted and it is
/// flagged "eligible to relocate to cheaper storage"; reads still work (they open it transiently
/// and promote it back to Hot). Search-index drop + recompression (RFC §3) need `FactLedger`
/// support and land with the `KnowledgeStore`/`SegmentBackend` work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    #[default]
    Hot,
    Cold,
}

impl Tier {
    fn is_hot(&self) -> bool {
        matches!(self, Tier::Hot)
    }
}

/// One persisted catalog row: a partition's key, where its [`FactLedger`] lives on disk, and its
/// [`Tier`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PartitionEntry {
    pub key: PartitionKey,
    pub root: PathBuf,
    #[serde(default, skip_serializing_if = "Tier::is_hot")]
    pub tier: Tier,
}

/// RFC 0111 §5's `PartitionCatalog`, persisted as `<catalog_root>/catalog.json`. One entry per
/// partition (never per entity), kept sorted for a deterministic file. `dimension`/`time_bucket`
/// are recorded on first write and then frozen — reopening with a different value is an error
/// (`DimensionMismatch`), since the on-disk partition names encode both.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PartitionCatalog {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimension: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_bucket: Option<String>,
    pub partitions: Vec<PartitionEntry>,
}
