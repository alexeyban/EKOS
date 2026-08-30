//! Tests for the partitioned ledger.

use super::*;
use ekos_kir::{ObjectKind, RelationshipKind};
use tempfile::tempdir;

fn rel(from: KirId, to: KirId, kind: RelationshipKind) -> KirRelationship {
    KirRelationship::new(kind, from, to)
}

fn ledger_with_root(dir: &Path) -> PartitionedLedger {
    ledger_with_bucket(dir, TimeBucket::Monthly)
}

fn ledger_with_bucket(dir: &Path, bucket: TimeBucket) -> PartitionedLedger {
    let root = dir.to_path_buf();
    let part_root = root.clone();
    PartitionedLedger::new(root, PartitionDimension::EntityKind, bucket, move |key| {
        part_root.join(&key.dimension_value).join(&key.time_bucket)
    })
    .unwrap()
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
fn all_objects_fans_out_across_every_partition() {
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
/// that kind's partitions, never the others.
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

    assert_eq!(ledger.catalog_partition_keys().len(), 2);
    assert_eq!(ledger.partition_keys_in_scope("Table").len(), 1);
    assert!(ledger.partition_keys_in_scope("Table").len() < ledger.catalog_partition_keys().len());

    let tables = ledger.objects_in_kind("Table").unwrap();
    assert_eq!(tables.len(), 3);
    assert!(tables.iter().all(|o| o.kind == ObjectKind::Table));

    assert_eq!(ledger.objects_in_kind("File").unwrap().len(), 5);
    // A scope that matches no partition reads nothing.
    assert!(ledger.objects_in_kind("Module").unwrap().is_empty());
}

/// The RFC 0111 §2 correctness property: force one entity's two writes into two different
/// time-bucket partitions and confirm `get_object` still resolves to a single (the newest)
/// partition while `object_history` fans out to both, in chronological order.
#[test]
fn entity_spanning_two_time_buckets_gets_single_partition_point_reads_and_full_fan_out_history() {
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

    assert_eq!(ledger.catalog_partition_keys().len(), 2);

    let current = ledger.get_object(&id).unwrap().unwrap();
    assert_eq!(current.name, "orders_renamed");

    let history = ledger.object_history(&id).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].name, "orders");
    assert_eq!(history[1].name, "orders_renamed");
}

/// Time-bucket granularity is configurable (RFC 0111 §1): `Daily` splits partitions by day.
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
        .catalog_partition_keys()
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
/// global `SegmentStore`."
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
    assert_eq!(ledger.catalog_partition_keys().len(), 2);

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

/// RFC 0111 §5: the catalog **and** the entity index are persisted, so a brand-new
/// `PartitionedLedger` at the same root sees every partition and resolves any entity with no
/// partition scan at all.
#[test]
fn catalog_and_entities_survive_a_reopen() {
    let dir = tempdir().unwrap();
    let legacy_id = KirId::new();
    {
        let ledger = ledger_with_root(dir.path());
        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        ledger
            .append_object(&KirObject::new("main.rs", ObjectKind::File))
            .unwrap();
        // one entity with history across two older time-bucket partitions
        let mut v1 = KirObject::new("legacy", ObjectKind::Table);
        v1.id = legacy_id;
        v1.created_at = "2026-05-10T00:00:00Z".parse().unwrap();
        let mut v2 = KirObject::new("legacy_v2", ObjectKind::Table);
        v2.id = legacy_id;
        v2.created_at = "2026-06-10T00:00:00Z".parse().unwrap();
        ledger.append_object(&v1).unwrap();
        ledger.append_object(&v2).unwrap();
    }
    assert!(dir.path().join("catalog.json").exists());
    assert!(
        std::fs::read_dir(dir.path().join("index"))
            .unwrap()
            .any(|e| e.unwrap().file_name().to_string_lossy().starts_with("run-")),
        "index run file written"
    );

    // fresh handle, same root, zero writes
    let reopened = ledger_with_root(dir.path());
    assert!(
        reopened.partition_keys().is_empty(),
        "nothing open until a read touches it"
    );
    // Table/2026-05, Table/2026-06, Table/<now>, File/<now>
    assert_eq!(reopened.catalog_partition_keys().len(), 4);

    // Point read: resolved from the persisted entity index — opens ONLY the entity's newest
    // partition, never scans the other three.
    assert_eq!(
        reopened.get_object(&legacy_id).unwrap().unwrap().name,
        "legacy_v2"
    );
    assert_eq!(
        reopened.partition_keys().len(),
        1,
        "no catalog scan — only the entity's newest partition was opened"
    );

    // Full history: opens exactly the two partitions the entity spans, still no scan.
    let hist: Vec<String> = reopened
        .object_history(&legacy_id)
        .unwrap()
        .into_iter()
        .map(|o| o.name)
        .collect();
    assert_eq!(hist, vec!["legacy".to_string(), "legacy_v2".to_string()]);
    assert_eq!(reopened.partition_keys().len(), 2);

    // pruned broad read works off the persisted catalog, deduplicated to current state
    let mut table_names: Vec<String> = reopened
        .objects_in_kind("Table")
        .unwrap()
        .into_iter()
        .map(|o| o.name)
        .collect();
    table_names.sort();
    assert_eq!(
        table_names,
        vec!["legacy_v2".to_string(), "orders".to_string()]
    );
    assert_eq!(reopened.objects_in_kind("File").unwrap().len(), 1);
    assert!(reopened.objects_in_kind("Module").unwrap().is_empty());

    // an unknown id resolves cleanly (scans, finds nothing, caches)
    assert!(reopened.get_object(&KirId::new()).unwrap().is_none());

    // the reopened handle's own writes still route + register correctly
    reopened
        .append_object(&KirObject::new(
            "new_svc",
            ObjectKind::Custom("Service".into()),
        ))
        .unwrap();
    assert_eq!(reopened.catalog_partition_keys().len(), 5);
}

/// The entity index tolerates a lost partition-crossing pair line: after open, the affected
/// entity's history is short one partition until `rebuild_entity_index` re-derives it from the
/// partitions themselves.
#[test]
fn rebuild_entity_index_repairs_a_dropped_pair_line() {
    let dir = tempdir().unwrap();
    let id = KirId::new();
    {
        let ledger = ledger_with_root(dir.path());
        let mut v1 = KirObject::new("svc", ObjectKind::Table);
        v1.id = id;
        v1.created_at = "2026-03-10T00:00:00Z".parse().unwrap();
        let mut v2 = KirObject::new("svc_v2", ObjectKind::Table);
        v2.id = id;
        v2.created_at = "2026-04-10T00:00:00Z".parse().unwrap();
        ledger.append_object(&v1).unwrap();
        ledger.append_object(&v2).unwrap();
    }

    // Simulate a lost crossing line: drop the newer partition's pair from every run file.
    let idx = dir.path().join("index");
    for entry in std::fs::read_dir(&idx).unwrap() {
        let p = entry.unwrap().path();
        let kept: String = std::fs::read_to_string(&p)
            .unwrap()
            .lines()
            .filter(|l| !l.contains("2026-04"))
            .map(|l| format!("{l}\n"))
            .collect();
        std::fs::write(&p, kept).unwrap();
    }

    let reopened = ledger_with_root(dir.path());
    // history is short the newer partition…
    assert_eq!(reopened.object_history(&id).unwrap().len(), 1);

    // …until a rebuild re-derives the index from the partitions.
    reopened.rebuild_entity_index().unwrap();
    let hist: Vec<String> = reopened
        .object_history(&id)
        .unwrap()
        .into_iter()
        .map(|o| o.name)
        .collect();
    assert_eq!(hist, vec!["svc".to_string(), "svc_v2".to_string()]);
    assert_eq!(reopened.get_object(&id).unwrap().unwrap().name, "svc_v2");
}

/// Entity-index runs are merged once [`COMPACT_AT`] accumulate: many reopen-with-write cycles
/// leave a bounded number of run files, and every entity still resolves.
#[test]
fn entity_index_runs_compact_on_open() {
    let dir = tempdir().unwrap();
    let mut ids = Vec::new();
    // COMPACT_AT + a few write sessions, each creating its own run file
    for i in 0..(COMPACT_AT + 3) {
        let ledger = ledger_with_root(dir.path());
        let obj = KirObject::new(format!("e{i}"), ObjectKind::Table);
        ids.push(obj.id);
        ledger.append_object(&obj).unwrap();
    }

    let run_count = std::fs::read_dir(dir.path().join("index"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("run-")
        })
        .count();
    assert!(
        run_count <= 5,
        "runs compacted, got {run_count} (COMPACT_AT = {COMPACT_AT})"
    );

    let reopened = ledger_with_root(dir.path());
    for id in &ids {
        assert!(reopened.get_object(id).unwrap().is_some());
    }
}

/// `PartitionDimension::SourceScope` routes by the source resolver's answer, independent of
/// `ObjectKind`.
#[test]
fn source_scope_routes_by_resolver_not_kind() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let part_root = root.clone();
    let ledger = PartitionedLedger::new(
        root,
        PartitionDimension::SourceScope,
        TimeBucket::Monthly,
        move |key| part_root.join(&key.dimension_value).join(&key.time_bucket),
    )
    .unwrap()
    .with_source_resolver(|obj| {
        // pretend name prefix carries the source
        obj.name.split_once(':').map(|(src, _)| src.to_string())
    });

    // Same kind (Table), different sources → different partitions.
    ledger
        .append_object(&KirObject::new("sql:orders", ObjectKind::Table))
        .unwrap();
    ledger
        .append_object(&KirObject::new("pentaho:orders_stg", ObjectKind::Table))
        .unwrap();
    // Different kind, same source as the first → same partition.
    ledger
        .append_object(&KirObject::new(
            "sql:load_orders",
            ObjectKind::Custom("View".into()),
        ))
        .unwrap();

    let mut scopes: Vec<String> = ledger
        .catalog_partition_keys()
        .into_iter()
        .map(|k| k.dimension_value)
        .collect();
    scopes.sort();
    assert_eq!(scopes, vec!["pentaho".to_string(), "sql".to_string()]);
    assert_eq!(ledger.objects_in_kind("sql").unwrap().len(), 2);
    assert_eq!(ledger.objects_in_kind("pentaho").unwrap().len(), 1);
}

/// A source-based dimension with no resolver answer is a hard error, never a silent misroute.
#[test]
fn source_scope_without_a_resolved_source_errors() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let ledger = PartitionedLedger::new(
        root.clone(),
        PartitionDimension::SourceScope,
        TimeBucket::Monthly,
        move |key| root.join(&key.dimension_value),
    )
    .unwrap(); // no with_source_resolver → default returns None

    let err = ledger
        .append_object(&KirObject::new("orders", ObjectKind::Table))
        .unwrap_err();
    assert!(matches!(err, PartitionError::UnresolvedSource { .. }));
}

/// `Composite` partitions by `source` + `kind` together.
#[test]
fn composite_partitions_by_source_and_kind() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let part_root = root.clone();
    let ledger = PartitionedLedger::new(
        root,
        PartitionDimension::Composite,
        TimeBucket::Monthly,
        move |key| part_root.join(key.dimension_value.replace('\u{1f}', "__")),
    )
    .unwrap()
    .with_source_resolver(|obj| obj.name.split_once(':').map(|(s, _)| s.to_string()));

    ledger
        .append_object(&KirObject::new("sql:orders", ObjectKind::Table))
        .unwrap();
    ledger
        .append_object(&KirObject::new(
            "sql:load",
            ObjectKind::Custom("View".into()),
        ))
        .unwrap();
    ledger
        .append_object(&KirObject::new("git:main.rs", ObjectKind::File))
        .unwrap();

    // sql+Table, sql+View, git+File → three distinct composite partitions.
    assert_eq!(ledger.catalog_partition_keys().len(), 3);
    assert_eq!(
        ledger
            .objects_in_kind(&format!("sql\u{1f}{}", ObjectKind::Table))
            .unwrap()
            .len(),
        1
    );
}

/// The routing/tiering config is frozen once partitions exist: reopening with a different
/// dimension or time bucket is a `DimensionMismatch` error, not a silent re-route.
#[test]
fn reopening_with_a_changed_dimension_or_bucket_errors() {
    let dir = tempdir().unwrap();
    {
        let ledger = ledger_with_bucket(dir.path(), TimeBucket::Monthly);
        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
    }
    // same dimension + bucket → fine
    assert!(
        ledger_with_bucket(dir.path(), TimeBucket::Monthly)
            .get_object(&KirId::new())
            .is_ok()
    );

    // changed time bucket → error
    let root = dir.path().to_path_buf();
    let res = PartitionedLedger::new(
        root.clone(),
        PartitionDimension::EntityKind,
        TimeBucket::Daily,
        move |k| root.join(&k.dimension_value),
    );
    assert!(matches!(
        res,
        Err(PartitionError::DimensionMismatch {
            field: "time-bucket",
            ..
        })
    ));

    // changed dimension → error
    let root = dir.path().to_path_buf();
    let res = PartitionedLedger::new(
        root.clone(),
        PartitionDimension::SourceScope,
        TimeBucket::Monthly,
        move |k| root.join(&k.dimension_value),
    );
    assert!(matches!(
        res,
        Err(PartitionError::DimensionMismatch {
            field: "dimension",
            ..
        })
    ));
}

#[test]
fn dimension_and_bucket_parse_config_strings() {
    assert_eq!(
        PartitionDimension::parse("entity-kind"),
        Some(PartitionDimension::EntityKind)
    );
    assert_eq!(
        PartitionDimension::parse("SOURCE_SCOPE"),
        Some(PartitionDimension::SourceScope)
    );
    assert_eq!(
        PartitionDimension::parse("composite").unwrap().as_str(),
        "composite"
    );
    assert_eq!(PartitionDimension::parse("nope"), None);
    assert_eq!(TimeBucket::Weekly.as_str(), "weekly");
}

/// RFC 0111 §3: `mark_cold_before` demotes aged partitions (evicting their handles), the tier
/// survives a reopen, reads still return byte-identical data, and any read promotes a cold
/// partition back to hot.
#[test]
fn aged_partitions_go_cold_evict_handles_and_rehydrate() {
    let dir = tempdir().unwrap();
    let ledger = ledger_with_root(dir.path());

    let mut legacy = KirObject::new("legacy", ObjectKind::Table);
    legacy.created_at = "2026-01-15T00:00:00Z".parse().unwrap();
    ledger.append_object(&legacy).unwrap();
    ledger
        .append_object(&KirObject::new("orders", ObjectKind::Table))
        .unwrap(); // current month

    let old_key = ledger
        .catalog_partition_keys()
        .into_iter()
        .find(|k| k.time_bucket == "2026-01")
        .unwrap();
    assert_eq!(ledger.partition_tier(&old_key), Some(Tier::Hot));
    assert_eq!(ledger.partition_keys().len(), 2);

    // sweep: anything before 2026-06 goes cold
    let cutoff = "2026-06-01T00:00:00Z".parse().unwrap();
    assert_eq!(ledger.mark_cold_before(cutoff).unwrap(), 1);
    assert_eq!(ledger.mark_cold_before(cutoff).unwrap(), 0, "idempotent");
    assert_eq!(ledger.partition_tier(&old_key), Some(Tier::Cold));
    assert_eq!(ledger.cold_partition_keys(), vec![old_key.clone()]);
    assert_eq!(
        ledger.partition_keys().len(),
        1,
        "cold partition's open handle was evicted"
    );

    // read the cold partition → data intact, tier auto-promoted
    assert_eq!(
        ledger.get_object(&legacy.id).unwrap().unwrap().name,
        "legacy"
    );
    assert_eq!(ledger.partition_tier(&old_key), Some(Tier::Hot));

    // re-cold, drop, reopen — persisted tier survives
    ledger.mark_cold_before(cutoff).unwrap();
    assert!(dir.path().join("catalog.json").exists());
    drop(ledger);

    let reopened = ledger_with_root(dir.path());
    assert_eq!(reopened.partition_tier(&old_key), Some(Tier::Cold));
    // unscoped read is still complete and byte-identical across hot + cold
    let mut names: Vec<String> = reopened
        .objects_in_kind("Table")
        .unwrap()
        .into_iter()
        .map(|o| o.name)
        .collect();
    names.sort();
    assert_eq!(names, vec!["legacy".to_string(), "orders".to_string()]);
    // …and that read promoted the cold one
    assert_eq!(reopened.partition_tier(&old_key), Some(Tier::Hot));
}

/// RFC 0111 amendment §1: relationships route by `"rel:"+kind`, disjoint from object
/// partitions; §2: `relationships_for` prunes to the endpoint's relationship partitions.
#[test]
fn relationships_route_by_kind_and_relationships_for_is_pruned() {
    let dir = tempdir().unwrap();
    let ledger = ledger_with_root(dir.path());

    let a = KirObject::new("a", ObjectKind::Table);
    let b = KirObject::new("b", ObjectKind::Table);
    let c = KirObject::new("c", ObjectKind::File);
    ledger.append_object(&a).unwrap();
    ledger.append_object(&b).unwrap();
    ledger.append_object(&c).unwrap();

    let r_ab = rel(a.id, b.id, RelationshipKind::DependsOn);
    let r_bc = rel(b.id, c.id, RelationshipKind::Calls);
    ledger.append_relationship(&r_ab).unwrap();
    ledger.append_relationship(&r_bc).unwrap();

    // object + relationship partitions are disjoint; rel partitions are "rel:*"
    let rel_parts: Vec<String> = ledger
        .catalog_partition_keys()
        .into_iter()
        .map(|k| k.dimension_value)
        .filter(|d| d.starts_with("rel:"))
        .collect();
    let mut rel_parts = rel_parts;
    rel_parts.sort();
    assert_eq!(rel_parts, vec!["rel:Calls", "rel:DependsOn"]);
    assert!(ledger.objects_in_kind("rel:DependsOn").unwrap().is_empty());

    assert_eq!(ledger.relationship_count().unwrap(), 2);
    assert_eq!(
        ledger
            .get_relationship(&r_ab.id)
            .unwrap()
            .unwrap()
            .kind
            .to_string(),
        "DependsOn"
    );

    // relationships_for(b): both rels touch b → both, from exactly two partitions
    assert_eq!(ledger.relationships_for(&b.id).unwrap().len(), 2);
    // relationships_for(a): only r_ab → touches only the "rel:DependsOn" partition
    assert_eq!(ledger.relationships_for(&a.id).unwrap().len(), 1);
    assert_eq!(ledger.endpoint_sites(&a.id).unwrap().len(), 1);
    assert!(ledger.endpoint_sites(&a.id).unwrap().len() < ledger.catalog_partition_keys().len());

    // history + reopen: the rel index persists → resolves with zero scans
    assert_eq!(ledger.relationship_history(&r_bc.id).unwrap().len(), 1);
    drop(ledger);

    let reopened = ledger_with_root(dir.path());
    assert!(reopened.partition_keys().is_empty());
    assert_eq!(reopened.relationships_for(&c.id).unwrap().len(), 1);
    assert_eq!(
        reopened.partition_keys().len(),
        1,
        "pruned to c's one rel partition, no scan"
    );
    assert_eq!(
        reopened.get_relationship(&r_ab.id).unwrap().unwrap().from,
        a.id
    );
}

/// `rebuild_entity_index` re-derives the relationship + endpoint index, not just objects.
#[test]
fn rebuild_also_repairs_the_relationship_index() {
    let dir = tempdir().unwrap();
    let x = KirId::new();
    let y = KirId::new();
    let r = rel(x, y, RelationshipKind::DependsOn);
    {
        let ledger = ledger_with_root(dir.path());
        ledger
            .append_object(&{
                let mut o = KirObject::new("x", ObjectKind::Table);
                o.id = x;
                o
            })
            .unwrap();
        ledger.append_relationship(&r).unwrap();
    }
    // wipe the index dir entirely
    std::fs::remove_dir_all(dir.path().join("index")).unwrap();

    let reopened = ledger_with_root(dir.path());
    // index gone → relationships_for falls back to a scan, still correct
    assert_eq!(reopened.relationships_for(&x).unwrap().len(), 1);

    reopened.rebuild_entity_index().unwrap();
    // now served from the rebuilt index
    assert_eq!(reopened.get_relationship(&r.id).unwrap().unwrap().to, y);
    assert_eq!(reopened.relationships_for(&y).unwrap().len(), 1);
}

fn event(subject: KirId, at: &str) -> KirEvent {
    KirEvent {
        id: KirId::new(),
        kind: ekos_kir::EventKind::Modified,
        subject,
        payload: serde_json::json!({}),
        evidence: Vec::new(),
        occurred_at: at.parse().unwrap(),
    }
}

/// RFC 0111 amendment §3: events & evidence route to their own `"events"`/`"evidence"`
/// partitions and resolve by id, across a reopen, with no full scan.
#[test]
fn events_and_evidence_route_and_resolve() {
    use ekos_kir::SourceLocation;
    let dir = tempdir().unwrap();
    let e_id;
    let ev_id;
    {
        let ledger = ledger_with_root(dir.path());
        let obj = KirObject::new("svc", ObjectKind::Table);
        ledger.append_object(&obj).unwrap();

        let e = event(obj.id, "2026-08-15T00:00:00Z");
        e_id = e.id;
        ledger.append_event(&e).unwrap();

        let evi = KirEvidence::new(SourceLocation::file("src/svc.sql"), "CREATE TABLE svc(...)");
        ev_id = evi.id;
        ledger.append_evidence(&evi).unwrap();

        assert!(
            ledger
                .catalog_partition_keys()
                .iter()
                .any(|k| k.dimension_value == "events")
        );
        assert!(
            ledger
                .catalog_partition_keys()
                .iter()
                .any(|k| k.dimension_value == "evidence")
        );
    }

    let reopened = ledger_with_root(dir.path());
    assert!(reopened.partition_keys().is_empty());
    assert_eq!(reopened.get_event(&e_id).unwrap().unwrap().id, e_id);
    assert_eq!(
        reopened.partition_keys().len(),
        1,
        "resolved via index, no scan"
    );
    assert_eq!(
        reopened.get_evidence(&ev_id).unwrap().unwrap().fragment,
        "CREATE TABLE svc(...)"
    );
    assert!(reopened.get_event(&KirId::new()).unwrap().is_none());
}

/// The full `KnowledgeStore` surface works through a `dyn` trait object: point-in-time,
/// search, counts, diff, and a self-contained `vacuum_into` copy.
#[test]
fn partitioned_ledger_is_a_drop_in_knowledge_store() {
    use std::time::Duration;
    let dir = tempdir().unwrap();
    let store: Box<dyn KnowledgeStore> = Box::new(ledger_with_root(dir.path()));

    let a = KirObject::new("orders", ObjectKind::Table);
    KnowledgeStore::append_object(&*store, &a).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    let mid = Utc::now();
    std::thread::sleep(Duration::from_millis(5));

    let mut a2 = KirObject::new("orders_renamed", ObjectKind::Table);
    a2.id = a.id;
    KnowledgeStore::append_object(&*store, &a2).unwrap();
    let b = KirObject::new("customers", ObjectKind::Table);
    KnowledgeStore::append_object(&*store, &b).unwrap();
    KnowledgeStore::append_relationship(
        &*store,
        &KirRelationship::new(RelationshipKind::DependsOn, a.id, b.id),
    )
    .unwrap();

    // point-in-time (observation-time cut): at `mid` only "orders" existed, customers not yet
    assert_eq!(store.object_at(&a.id, mid).unwrap().unwrap().name, "orders");
    assert_eq!(
        store.get_object(&a.id).unwrap().unwrap().name,
        "orders_renamed"
    );
    assert_eq!(store.all_objects_at(mid).unwrap().len(), 1);
    assert_eq!(store.all_objects().unwrap().len(), 2);
    assert!(store.relationships_at(&a.id, mid).unwrap().is_empty());
    assert_eq!(store.relationships_at(&a.id, Utc::now()).unwrap().len(), 1);

    assert_eq!(store.object_count().unwrap(), 2);
    assert_eq!(store.relationship_count().unwrap(), 1);
    assert!(store.entry_count().unwrap() >= 4);
    assert_eq!(store.relationships_for(&b.id).unwrap().len(), 1);

    // search
    let hits = store.find_objects("orders").unwrap();
    assert!(hits.iter().any(|(id, _)| *id == a.id));

    // diff over a window covering the later writes
    let d = store.diff(mid, Utc::now()).unwrap();
    assert!(!d.touched.is_empty());

    // vacuum into a fresh dir → reopen it and see the same knowledge
    let dest = tempdir().unwrap();
    store.vacuum_into(dest.path()).unwrap();
    let copy = ledger_with_root(dest.path());
    assert_eq!(copy.object_count().unwrap(), 2);
    assert_eq!(copy.relationship_count().unwrap(), 1);
    assert_eq!(
        copy.get_object(&a.id).unwrap().unwrap().name,
        "orders_renamed"
    );
}

#[test]
fn with_segment_backend_routes_each_partition_through_its_backend() {
    use std::sync::{Arc, Mutex};

    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let part_root = root.clone();

    // One MemBackend per partition, plus a log of every (partition-id, root) the resolver saw.
    let backends: Arc<Mutex<std::collections::HashMap<String, Arc<crate::MemBackend>>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let calls: Arc<Mutex<Vec<(String, std::path::PathBuf)>>> = Arc::new(Mutex::new(Vec::new()));
    let backends_c = backends.clone();
    let calls_c = calls.clone();

    let ledger = PartitionedLedger::new(
        root.clone(),
        PartitionDimension::EntityKind,
        TimeBucket::Monthly,
        move |key| part_root.join(&key.dimension_value).join(&key.time_bucket),
    )
    .unwrap()
    .with_segment_backend(move |key, local_root| {
        let id = format!("{}/{}", key.dimension_value, key.time_bucket);
        calls_c
            .lock()
            .unwrap()
            .push((id.clone(), local_root.to_path_buf()));
        let mut map = backends_c.lock().unwrap();
        let b = map
            .entry(id)
            .or_insert_with(|| Arc::new(crate::MemBackend::new(local_root.join(".seg-cache"))))
            .clone();
        Some(b as Arc<dyn SegmentBackend>)
    });

    let orders = KirObject::new("orders", ObjectKind::Table);
    let main_rs = KirObject::new("main.rs", ObjectKind::File);
    ledger.append_object(&orders).unwrap();
    ledger.append_object(&main_rs).unwrap();
    ledger
        .append_relationship(&rel(main_rs.id, orders.id, RelationshipKind::DependsOn))
        .unwrap();

    // The resolver was consulted for the Table, File, and rel: partitions.
    let seen: std::collections::HashSet<String> = calls
        .lock()
        .unwrap()
        .iter()
        .map(|(id, _)| id.clone())
        .collect();
    let month = Utc::now().format("%Y-%m").to_string();
    assert!(seen.contains(&format!("Table/{month}")));
    assert!(seen.contains(&format!("File/{month}")));
    assert!(seen.iter().any(|id| id.starts_with("rel:DependsOn/")));

    // Reads still work end to end through the backend-wired partitions.
    assert_eq!(
        ledger.get_object(&orders.id).unwrap().unwrap().name,
        "orders"
    );
    assert_eq!(ledger.object_count().unwrap(), 2);
    assert_eq!(ledger.relationships_for(&orders.id).unwrap().len(), 1);
}
