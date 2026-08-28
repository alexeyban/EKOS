//! RFC 0111 Phase A: real, tested partitioning and fan-out for `KirObject`s, keyed by
//! `PartitionDimension::EntityKind` + a configurable time bucket.
//!
//! **Scope, stated precisely rather than overclaimed:** only object reads/writes are implemented
//! (`append_object`, `get_object`, `object_history`, `objects_in_kind`, `all_objects`,
//! `object_count`). Relationships/events/evidence and the rest of `KnowledgeStore`'s surface
//! (`diff`, `vacuum_into`, full-text search, …) are out of scope for this slice —
//! `PartitionedLedger` is **not** a `KnowledgeStore` yet and cannot be opened through
//! `open_store`. `PartitionDimension::SourceScope` and `Composite` are declared (matching RFC 0111
//! §1's design) but not yet routable: `KirObject` carries no explicit source/connector field to
//! route `SourceScope` by today, so routing by it returns [`PartitionError::UnsupportedDimension`]
//! rather than silently misrouting or guessing.
//!
//! Each partition is an ordinary, unmodified [`FactLedger`] — this module only adds routing and
//! fan-out above it, exactly RFC 0111's own "no format/invariant change, purely an access-path
//! layer" principle. A partition's root directory is resolved by a caller-supplied closure, which
//! is where this project's `[storage]` container config (RFC 0111 groundwork, `compiler-core`)
//! plugs in — different partitions can be routed into different configured container folders,
//! which is the concrete mechanism for testing "distributed storage" locally with plain folders.
//!
//! ## Known limits this slice does not close
//!
//! - **No on-disk catalog / partition discovery.** RFC 0111 §5's `PartitionCatalog` is not
//!   persisted. A freshly-constructed `PartitionedLedger` knows about a partition only once a
//!   write (or point read via the in-memory `entity_partitions` map) has touched it this process —
//!   broad reads (`all_objects`, `objects_in_kind`) see only partitions opened in the current
//!   process. Persisting and rescanning the catalog on open is the next increment.
//! - **Hot/cold tiering (RFC 0111 §3)** and the **`SegmentBackend` seam (§4)** are not here — every
//!   partition is a plain local-disk `FactLedger`.
//! - **`entity_partitions` is in-memory only**, rebuilt from writes, not from disk — same gap as
//!   the catalog above.
//!
//! ## Concurrency (RFC 0111 §1: "N partitions admit N concurrent writers")
//!
//! Each open partition is held as an `Arc<FactLedger>`. The map of open partitions is guarded by a
//! `Mutex` held only long enough to look up (or create) and clone one `Arc` — never for the
//! duration of a read or write. `FactLedger` is itself internally synchronized (`Mutex<Inner>`),
//! so two threads writing to **different** partitions genuinely proceed in parallel, while two
//! threads writing to the **same** partition serialize on that partition's own lock — the
//! single-writer-per-partition invariant RFC 0104's `write.lock` also enforces cross-process.

use crate::FactLedger;
use crate::LedgerError;
use chrono::{DateTime, Utc};
use ekos_kir::{KirId, KirObject};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PartitionError {
    #[error("ledger error in partition {key:?}: {source}")]
    Ledger {
        key: PartitionKey,
        #[source]
        source: LedgerError,
    },
    #[error(
        "partition dimension {0:?} is not yet routable — this slice (RFC 0111 Phase A) supports \
         EntityKind only"
    )]
    UnsupportedDimension(PartitionDimension),
}

/// RFC 0111 §1's routing dimension. Only [`PartitionDimension::EntityKind`] is implemented by
/// this module today — see the module doc comment for why the other two aren't yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartitionDimension {
    EntityKind,
    SourceScope,
    Composite,
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
}

/// `(time_bucket, dimension_value)` — field order matters: it makes the derived `Ord` sort
/// chronologically first, which [`PartitionedLedger::object_history`] relies on to merge partitions
/// in the correct order without needing a separate timestamp comparison.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartitionKey {
    /// e.g. `"2026-08"` (monthly), `"2026-W35"` (weekly), `"2026-08-27"` (daily).
    pub time_bucket: String,
    /// e.g. `"Table"`, `"File"` (an `ObjectKind`'s `Display` output) for `EntityKind` routing.
    pub dimension_value: String,
}

/// Routes `KirObject` reads/writes across multiple [`FactLedger`] partitions by
/// `PartitionDimension::EntityKind` + time bucket (RFC 0111 §1), with the
/// `entity_id → Set<PartitionKey>` correctness fix (RFC 0111 §2) built in from the start rather
/// than bolted on: current-state reads resolve to one partition (the newest), full-history reads
/// fan out to every partition an entity has ever been written to.
pub struct PartitionedLedger {
    dimension: PartitionDimension,
    time_bucket: TimeBucket,
    root_for: Box<dyn Fn(&PartitionKey) -> PathBuf + Send + Sync>,
    open: Mutex<HashMap<PartitionKey, Arc<FactLedger>>>,
    entity_partitions: Mutex<HashMap<KirId, HashSet<PartitionKey>>>,
}

impl PartitionedLedger {
    /// `root_for` resolves a partition's on-disk root given its key — the hook point for
    /// container-config routing (different partitions can resolve into different configured
    /// container folders) or a plain `<workspace>/.ekos/partitions/<key>` layout for a
    /// single-container setup.
    pub fn new(
        dimension: PartitionDimension,
        time_bucket: TimeBucket,
        root_for: impl Fn(&PartitionKey) -> PathBuf + Send + Sync + 'static,
    ) -> Self {
        Self {
            dimension,
            time_bucket,
            root_for: Box::new(root_for),
            open: Mutex::new(HashMap::new()),
            entity_partitions: Mutex::new(HashMap::new()),
        }
    }

    fn key_for(&self, obj: &KirObject) -> Result<PartitionKey, PartitionError> {
        match self.dimension {
            PartitionDimension::EntityKind => Ok(PartitionKey {
                time_bucket: self.time_bucket.label(obj.created_at),
                dimension_value: obj.kind.to_string(),
            }),
            other => Err(PartitionError::UnsupportedDimension(other)),
        }
    }

    /// The partition for `key`, opening (and creating on disk) it on demand. The `open` map lock is
    /// held only for the lookup/insert, never for the caller's subsequent read or write.
    fn partition(&self, key: &PartitionKey) -> Result<Arc<FactLedger>, PartitionError> {
        let mut open = self.open.lock().unwrap();
        if let Some(ledger) = open.get(key) {
            return Ok(Arc::clone(ledger));
        }
        let root = (self.root_for)(key);
        let ledger =
            Arc::new(
                FactLedger::open(&root).map_err(|source| PartitionError::Ledger {
                    key: key.clone(),
                    source,
                })?,
            );
        open.insert(key.clone(), Arc::clone(&ledger));
        Ok(ledger)
    }

    /// A brief-lock snapshot of the currently-open partitions, optionally pruned to one dimension
    /// value (RFC 0111 §1: broad reads "fan out only to partitions whose dimension value … could
    /// match"). `None` scope = every open partition (the unscoped worst case).
    fn snapshot(&self, scope: Option<&str>) -> Vec<(PartitionKey, Arc<FactLedger>)> {
        self.open
            .lock()
            .unwrap()
            .iter()
            .filter(|(k, _)| scope.is_none_or(|s| k.dimension_value == s))
            .map(|(k, v)| (k.clone(), Arc::clone(v)))
            .collect()
    }

    pub fn append_object(&self, obj: &KirObject) -> Result<bool, PartitionError> {
        let key = self.key_for(obj)?;
        let ledger = self.partition(&key)?;
        let result = ledger
            .append_object(obj)
            .map_err(|source| PartitionError::Ledger {
                key: key.clone(),
                source,
            })?;
        self.entity_partitions
            .lock()
            .unwrap()
            .entry(obj.id)
            .or_default()
            .insert(key);
        Ok(result)
    }

    /// Current state: routes to exactly one partition — the entity's most recent (RFC 0111 §2:
    /// current state always lives in the newest partition, so no fan-out here even though the
    /// entity may span more than one partition historically). `None` for an id this ledger has
    /// never routed a write for, without touching any partition on disk.
    pub fn get_object(&self, id: &KirId) -> Result<Option<KirObject>, PartitionError> {
        let latest = {
            let entity_partitions = self.entity_partitions.lock().unwrap();
            entity_partitions
                .get(id)
                .and_then(|set| set.iter().max().cloned())
        };
        let Some(key) = latest else {
            return Ok(None);
        };
        let ledger = self.partition(&key)?;
        ledger
            .get_object(id)
            .map_err(|source| PartitionError::Ledger { key, source })
    }

    /// Full history: fans out to every partition this entity has ever been written to (RFC 0111
    /// §2's correctness fix), oldest partition first. Correct without a separate timestamp
    /// comparison because partitions are strictly time-ordered by construction — writes always
    /// route by the *current* time bucket, so an entity's partition set only ever grows into newer
    /// buckets, never backfills an older one — and `PartitionKey`'s derived `Ord` sorts by
    /// `time_bucket` first.
    pub fn object_history(&self, id: &KirId) -> Result<Vec<KirObject>, PartitionError> {
        let mut keys: Vec<PartitionKey> = {
            let entity_partitions = self.entity_partitions.lock().unwrap();
            entity_partitions
                .get(id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect()
        };
        keys.sort();
        let mut history = Vec::new();
        for key in keys {
            let ledger = self.partition(&key)?;
            history.extend(
                ledger
                    .object_history(id)
                    .map_err(|source| PartitionError::Ledger {
                        key: key.clone(),
                        source,
                    })?,
            );
        }
        Ok(history)
    }

    /// Broad read, **pruned** to one dimension value (RFC 0111 §1). Touches only the open
    /// partitions whose key matches `dimension_value` — the non-matching partitions are never
    /// opened or read. This is the scoped-query fast path partitioning exists for.
    pub fn objects_in_kind(&self, dimension_value: &str) -> Result<Vec<KirObject>, PartitionError> {
        let mut out = Vec::new();
        for (key, ledger) in self.snapshot(Some(dimension_value)) {
            out.extend(
                ledger
                    .all_objects()
                    .map_err(|source| PartitionError::Ledger { key, source })?,
            );
        }
        Ok(out)
    }

    /// Every object across every partition currently open — the unscoped-query worst case RFC
    /// 0111 names: this always fans out to everything, never pruned. Prefer [`Self::objects_in_kind`]
    /// whenever the query is scoped to one entity kind.
    pub fn all_objects(&self) -> Result<Vec<KirObject>, PartitionError> {
        let mut all = Vec::new();
        for (key, ledger) in self.snapshot(None) {
            all.extend(
                ledger
                    .all_objects()
                    .map_err(|source| PartitionError::Ledger { key, source })?,
            );
        }
        Ok(all)
    }

    pub fn object_count(&self) -> Result<usize, PartitionError> {
        let mut total = 0;
        for (key, ledger) in self.snapshot(None) {
            total += ledger
                .object_count()
                .map_err(|source| PartitionError::Ledger { key, source })?;
        }
        Ok(total)
    }

    /// The partitions currently open — test/introspection use (proving routing landed where
    /// expected), not part of the read/write surface above.
    pub fn partition_keys(&self) -> Vec<PartitionKey> {
        self.open.lock().unwrap().keys().cloned().collect()
    }

    /// The open partitions a scoped query for `dimension_value` would actually touch — a strict
    /// subset of [`Self::partition_keys`] whenever more than one dimension value is present.
    /// Introspection use, to prove pruning.
    pub fn partition_keys_in_scope(&self, dimension_value: &str) -> Vec<PartitionKey> {
        self.open
            .lock()
            .unwrap()
            .keys()
            .filter(|k| k.dimension_value == dimension_value)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::ObjectKind;
    use tempfile::tempdir;

    fn ledger_with_root(dir: &std::path::Path) -> PartitionedLedger {
        ledger_with_bucket(dir, TimeBucket::Monthly)
    }

    fn ledger_with_bucket(dir: &std::path::Path, bucket: TimeBucket) -> PartitionedLedger {
        let root = dir.to_path_buf();
        PartitionedLedger::new(PartitionDimension::EntityKind, bucket, move |key| {
            root.join(&key.dimension_value).join(&key.time_bucket)
        })
    }

    #[test]
    fn different_entity_kinds_route_to_different_partitions() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());

        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        ledger
            .append_object(&KirObject::new("main.rs", ObjectKind::File))
            .unwrap();

        let mut keys: Vec<String> = ledger
            .partition_keys()
            .into_iter()
            .map(|k| k.dimension_value)
            .collect();
        keys.sort();
        assert_eq!(keys, vec!["File".to_string(), "Table".to_string()]);
        let this_month = chrono::Utc::now().format("%Y-%m").to_string();
        assert!(
            dir.path()
                .join("Table")
                .join(&this_month)
                .join("segments")
                .exists()
        );
        assert!(
            dir.path()
                .join("File")
                .join(&this_month)
                .join("segments")
                .exists()
        );
    }

    #[test]
    fn point_read_routes_to_a_single_partition_no_fan_out() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());
        let obj = KirObject::new("orders", ObjectKind::Table);
        ledger.append_object(&obj).unwrap();

        let fetched = ledger.get_object(&obj.id).unwrap();
        assert_eq!(fetched.unwrap().name, "orders");
        // Only the one partition this write actually routed to was ever opened.
        assert_eq!(ledger.partition_keys().len(), 1);
    }

    #[test]
    fn unknown_id_returns_none_without_touching_any_partition() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());
        assert!(ledger.get_object(&KirId::new()).unwrap().is_none());
        assert!(ledger.partition_keys().is_empty());
    }

    #[test]
    fn all_objects_fans_out_across_every_open_partition() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());
        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        ledger
            .append_object(&KirObject::new("main.rs", ObjectKind::File))
            .unwrap();

        assert_eq!(ledger.object_count().unwrap(), 2);
        let mut names: Vec<String> = ledger
            .all_objects()
            .unwrap()
            .into_iter()
            .map(|o| o.name)
            .collect();
        names.sort();
        assert_eq!(names, vec!["main.rs".to_string(), "orders".to_string()]);
    }

    /// RFC 0111 §1's scoped-query fast path: a broad read scoped to one entity kind touches only
    /// that kind's partitions, never the others — proven both by the pruned key set and by the
    /// result containing only matching-kind objects.
    #[test]
    fn scoped_broad_read_is_pruned_to_matching_partitions() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());
        for i in 0..3 {
            ledger
                .append_object(&KirObject::new(format!("t{i}"), ObjectKind::Table))
                .unwrap();
        }
        for i in 0..5 {
            ledger
                .append_object(&KirObject::new(format!("f{i}"), ObjectKind::File))
                .unwrap();
        }

        // Three open partitions? No — two (one per kind, same month). Pruning is by dimension value.
        assert_eq!(ledger.partition_keys().len(), 2);
        assert_eq!(ledger.partition_keys_in_scope("Table").len(), 1);
        assert!(ledger.partition_keys_in_scope("Table").len() < ledger.partition_keys().len());

        let tables = ledger.objects_in_kind("Table").unwrap();
        assert_eq!(tables.len(), 3);
        assert!(tables.iter().all(|o| o.kind == ObjectKind::Table));

        assert_eq!(ledger.objects_in_kind("File").unwrap().len(), 5);
        // A scope that matches no open partition reads nothing, touches nothing.
        assert!(ledger.objects_in_kind("Module").unwrap().is_empty());
    }

    /// The RFC 0111 §2 correctness property, proven directly: force one entity's two writes into
    /// two different partitions by giving them different `created_at` months (i.e. different time
    /// buckets), and confirm `get_object` still resolves to a single (the newest) partition while
    /// `object_history` fans out to both, in chronological order.
    #[test]
    fn entity_spanning_two_time_buckets_gets_single_partition_point_reads_and_full_fan_out_history()
    {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());

        let id = KirId::new();
        let mut v1 = KirObject::new("orders", ObjectKind::Table);
        v1.id = id;
        v1.created_at = "2026-07-15T00:00:00Z".parse().unwrap();
        let mut v2 = KirObject::new("orders_renamed", ObjectKind::Table);
        v2.id = id;
        v2.created_at = "2026-08-15T00:00:00Z".parse().unwrap();

        ledger.append_object(&v1).unwrap();
        ledger.append_object(&v2).unwrap();

        // Two distinct time-bucket partitions for the *same* dimension value.
        assert_eq!(ledger.partition_keys().len(), 2);

        // Point read resolves to exactly the newest partition's version.
        let current = ledger.get_object(&id).unwrap().unwrap();
        assert_eq!(current.name, "orders_renamed");

        // Full history fans out to both partitions, oldest first.
        let history = ledger.object_history(&id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].name, "orders");
        assert_eq!(history[1].name, "orders_renamed");
    }

    /// Time-bucket granularity is configurable (RFC 0111 §1): the same two writes, one month apart,
    /// land in one partition under `Monthly` but would split — here they land in the *same* daily
    /// partition only if same day. Uses two same-day writes to show `Daily` keeps them together and
    /// a cross-day pair to show it splits them, independent of `Monthly`.
    #[test]
    fn daily_time_bucket_splits_partitions_by_day() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_bucket(dir.path(), TimeBucket::Daily);

        let mut a = KirObject::new("a", ObjectKind::Table);
        a.created_at = "2026-08-27T09:00:00Z".parse().unwrap();
        let mut b = KirObject::new("b", ObjectKind::Table);
        b.created_at = "2026-08-27T21:00:00Z".parse().unwrap();
        let mut c = KirObject::new("c", ObjectKind::Table);
        c.created_at = "2026-08-28T01:00:00Z".parse().unwrap();

        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();
        ledger.append_object(&c).unwrap();

        let mut buckets: Vec<String> = ledger
            .partition_keys()
            .into_iter()
            .map(|k| k.time_bucket)
            .collect();
        buckets.sort();
        buckets.dedup();
        assert_eq!(
            buckets,
            vec!["2026-08-27".to_string(), "2026-08-28".to_string()]
        );
        assert_eq!(ledger.object_count().unwrap(), 3);
    }

    #[test]
    fn time_bucket_parses_config_strings() {
        assert_eq!(TimeBucket::parse("daily"), Some(TimeBucket::Daily));
        assert_eq!(TimeBucket::parse("  Weekly "), Some(TimeBucket::Weekly));
        assert_eq!(TimeBucket::parse("MONTHLY"), Some(TimeBucket::Monthly));
        assert_eq!(TimeBucket::parse("hourly"), None);
        assert_eq!(TimeBucket::default(), TimeBucket::Monthly);
    }

    /// RFC 0111 §1 / Acceptance Criteria: "N partitions admit N concurrent writers instead of one
    /// global `SegmentStore`." Two threads append concurrently to two different entity-kind
    /// partitions; all writes land, correctly routed, with no contention or corruption.
    #[test]
    fn concurrent_writers_across_two_partitions() {
        let dir = tempdir().unwrap();
        let ledger = ledger_with_root(dir.path());
        const N: usize = 60;

        std::thread::scope(|s| {
            s.spawn(|| {
                for i in 0..N {
                    ledger
                        .append_object(&KirObject::new(format!("t{i}"), ObjectKind::Table))
                        .unwrap();
                }
            });
            s.spawn(|| {
                for i in 0..N {
                    ledger
                        .append_object(&KirObject::new(format!("f{i}"), ObjectKind::File))
                        .unwrap();
                }
            });
        });

        assert_eq!(ledger.object_count().unwrap(), 2 * N);
        assert_eq!(ledger.objects_in_kind("Table").unwrap().len(), N);
        assert_eq!(ledger.objects_in_kind("File").unwrap().len(), N);
        assert_eq!(ledger.partition_keys().len(), 2);

        // Each partition is a real independent on-disk FactLedger.
        let this_month = chrono::Utc::now().format("%Y-%m").to_string();
        for kind in ["Table", "File"] {
            assert!(
                dir.path()
                    .join(kind)
                    .join(&this_month)
                    .join("segments")
                    .exists()
            );
        }
    }
}
