//! RFC 0113 B4a — a coordinator + a Service B query worker in front of a real partitioned
//! workspace. Every read served over RPC must match a direct `PartitionedLedger` read.

use chrono::Utc;
use ekos_cluster::{CoordinatorClient, PartitionLocation, spawn_ephemeral};
use ekos_distributed::{QueryWorkerClient, partition_id, spawn_ephemeral_worker};
use ekos_kir::{KirObject, KirRelationship, ObjectKind, RelationshipKind};
use ekos_ledger::partitioned::{PartitionDimension, PartitionedLedger, TimeBucket};
use tempfile::tempdir;

fn build_workspace(root: &std::path::Path) -> PartitionedLedger {
    let part_root = root.to_path_buf();
    let ledger = PartitionedLedger::new(
        root.to_path_buf(),
        PartitionDimension::EntityKind,
        TimeBucket::Monthly,
        move |key| part_root.join(&key.dimension_value).join(&key.time_bucket),
    )
    .unwrap();

    let orders = KirObject::new("orders", ObjectKind::Table);
    let customers = KirObject::new("customers", ObjectKind::Table);
    let main_rs = KirObject::new("main.rs", ObjectKind::File);
    ledger.append_object(&orders).unwrap();
    ledger.append_object(&customers).unwrap();
    ledger.append_object(&main_rs).unwrap();

    let mut orders_v2 = orders.clone();
    orders_v2
        .properties
        .insert("owner".into(), "data-eng".into());
    ledger.append_object(&orders_v2).unwrap();

    ledger
        .append_relationship(&KirRelationship::new(
            RelationshipKind::DependsOn,
            customers.id,
            orders.id,
        ))
        .unwrap();
    ledger
}

async fn register_all(coord: &CoordinatorClient, ledger: &PartitionedLedger) {
    for key in ledger.catalog_partition_keys() {
        let root = ledger.partition_root(&key).unwrap();
        coord
            .register_partition(
                &partition_id(&key),
                PartitionLocation::Local {
                    root: root.to_string_lossy().into_owned(),
                },
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn query_worker_reads_match_direct_partitioned_reads() {
    let dir = tempdir().unwrap();
    let cache = tempdir().unwrap();
    let ledger = build_workspace(dir.path());
    let month = Utc::now().format("%Y-%m").to_string();
    let table_pid = format!("Table/{month}");
    let file_pid = format!("File/{month}");

    let (coord_addr, _c) = spawn_ephemeral("127.0.0.1:0", None).await.unwrap();
    let coord = CoordinatorClient::connect(coord_addr).await.unwrap();
    register_all(&coord, &ledger).await;

    let (worker_addr, _w) =
        spawn_ephemeral_worker("127.0.0.1:0", &coord_addr.to_string(), cache.path())
            .await
            .unwrap();
    let client = QueryWorkerClient::connect(worker_addr).await.unwrap();
    client.ping().await.unwrap();

    let tables = ledger.objects_in_kind("Table").unwrap();
    assert_eq!(tables.len(), 2, "orders + customers");
    for obj in &tables {
        let direct = ledger.get_object(&obj.id).unwrap();
        let viarpc = client.get_object(&table_pid, obj.id).await.unwrap();
        assert_eq!(
            serde_json::to_value(&direct).unwrap(),
            serde_json::to_value(&viarpc).unwrap(),
            "get_object mismatch for {}",
            obj.name
        );
    }

    let orders_id = tables.iter().find(|o| o.name == "orders").unwrap().id;
    let customers_id = tables.iter().find(|o| o.name == "customers").unwrap().id;

    assert_eq!(
        client
            .object_history(&table_pid, orders_id)
            .await
            .unwrap()
            .len(),
        2,
        "orders has two versions"
    );

    // Relationships route to their own `rel:<kind>` partitions, disjoint from object partitions.
    let rel_pid = format!("rel:DependsOn/{month}");
    let rels = client
        .relationships_for(&rel_pid, customers_id)
        .await
        .unwrap();
    assert_eq!(rels.len(), 1);
    assert_eq!(rels[0].kind, RelationshipKind::DependsOn);
    assert_eq!(client.relationship_count(&rel_pid).await.unwrap(), 1);

    // find_objects over RPC must match a direct read-only open of the same partition dir.
    let table_root = ledger
        .partition_root(
            &ledger
                .catalog_partition_keys()
                .into_iter()
                .find(|k| k.dimension_value == "Table")
                .unwrap(),
        )
        .unwrap();
    let direct_hits = ekos_ledger::FactLedger::open_read_only(&table_root)
        .unwrap()
        .find_objects("orders")
        .unwrap();
    let rpc_hits = client.find_objects(&table_pid, "orders").await.unwrap();
    assert_eq!(
        serde_json::to_value(&direct_hits).unwrap(),
        serde_json::to_value(&rpc_hits).unwrap(),
        "find_objects over RPC diverges from a direct partition read"
    );

    assert_eq!(client.object_count(&table_pid).await.unwrap(), 2);
    assert_eq!(client.object_count(&file_pid).await.unwrap(), 1);

    assert!(
        client
            .object_at(&table_pid, orders_id, Utc::now())
            .await
            .unwrap()
            .is_some()
    );

    // an id in the wrong partition simply isn't found there (no error)
    assert!(
        client
            .get_object(&file_pid, orders_id)
            .await
            .unwrap()
            .is_none()
    );

    // an unknown partition is a clean error, not a panic
    let err = client
        .get_object("Nope/2000-01", orders_id)
        .await
        .unwrap_err();
    assert!(format!("{err}").contains("Nope/2000-01"));
}

#[cfg(feature = "object-store")]
#[test]
fn a_partition_materialises_from_object_storage() {
    // Sync test: `ObjectStoreBackend` drives its own runtime, so this must not run under one.
    use ekos_distributed::PartitionCache;

    let dir = tempdir().unwrap();
    let cache = tempdir().unwrap();
    let ledger = build_workspace(dir.path());
    let month = Utc::now().format("%Y-%m").to_string();
    let key = ledger
        .catalog_partition_keys()
        .into_iter()
        .find(|k| k.dimension_value == "Table")
        .unwrap();
    let root = ledger.partition_root(&key).unwrap();

    // Point an object_store `file://` backend at the real partition dir, then materialise it
    // through the cache exactly as a remote partition would be pulled.
    let loc = PartitionLocation::ObjectStore {
        url: format!("file://{}", root.display()),
        prefix: String::new(),
    };
    let pc = PartitionCache::new(cache.path());
    let materialised = pc.materialize(&format!("Table/{month}"), &loc).unwrap();
    assert_ne!(
        materialised, root,
        "a remote partition is copied, not used in place"
    );

    let reopened = ekos_ledger::FactLedger::open_read_only(&materialised).unwrap();
    let direct_tables = ledger.objects_in_kind("Table").unwrap();
    for obj in &direct_tables {
        assert!(
            reopened.get_object(&obj.id).unwrap().is_some(),
            "{} missing from the materialised copy",
            obj.name
        );
    }
}
