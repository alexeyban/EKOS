//! The partition catalog the coordinator owns (RFC 0113 B3 — same shape as Phase A's
//! `PartitionCatalog`, plus `PartitionLocation` = object storage).

use serde::{Deserialize, Serialize};

use crate::protocol::PartitionId;

/// Where a partition's sealed objects physically live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum PartitionLocation {
    /// Local disk (single-machine / dev).
    Local { root: String },
    /// Object storage — `url` is the `object_store` URL, `prefix` scopes keys within it.
    ObjectStore { url: String, prefix: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionMeta {
    pub id: PartitionId,
    pub location: PartitionLocation,
    /// `true` once demoted to cold (RFC 0111 §3 — in Distributed mode a caching hint, not a
    /// placement change).
    #[serde(default)]
    pub cold: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub partitions: Vec<PartitionMeta>,
}

impl Catalog {
    pub fn get(&self, id: &str) -> Option<&PartitionMeta> {
        self.partitions.iter().find(|p| p.id == id)
    }

    /// Register a partition on first write. Idempotent — re-registering the same id with the same
    /// location is a no-op; a *different* location is rejected (the on-disk names encode routing).
    pub fn register(&mut self, meta: PartitionMeta) -> Result<(), String> {
        match self.partitions.iter().find(|p| p.id == meta.id) {
            Some(existing) if existing.location == meta.location => Ok(()),
            Some(existing) => Err(format!(
                "partition {} already registered at {:?}, refusing to move to {:?}",
                meta.id, existing.location, meta.location
            )),
            None => {
                self.partitions.push(meta);
                self.partitions.sort_by(|a, b| a.id.cmp(&b.id));
                Ok(())
            }
        }
    }

    /// Partition ids matching `predicate` — the pruned set a scoped query fans out to.
    pub fn matching(&self, predicate: impl Fn(&str) -> bool) -> Vec<PartitionId> {
        self.partitions
            .iter()
            .map(|p| p.id.clone())
            .filter(|id| predicate(id))
            .collect()
    }
}
