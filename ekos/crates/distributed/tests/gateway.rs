//! RFC 0113 B4b — the `DistributedLedger` gateway must answer `KnowledgeStore` reads the same
//! way the in-process `PartitionedLedger` does, fanning across two query workers.

use std::sync::Arc;

use chrono::Utc;
use ekos_cluster::{CoordinatorClient, PartitionLocation, spawn_ephemeral};
use ekos_distributed::{DistributedLedger, partition_id, spawn_ephemeral_worker};
use ekos_kir::{KirObject, KirRelationship, ObjectKind, RelationshipKind};
use ekos_ledger::KnowledgeStore;
use ekos_ledger::partitioned::{PartitionDimension, PartitionedLedger, TimeBucket};
use tempfile::tempdir;

fn build_workspace(root: &std::path::Path) -> (PartitionedLedger, KirObject, KirObject, KirObject) {
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
    (ledger, orders, customers, main_rs)
}

#[tokio::test(flavor = "multi_thread")]
async fn gateway_matches_partitioned_ledger_over_two_workers() {
    let dir = tempdir().unwrap();
    let c1 = tempdir().unwrap();
    let c2 = tempdir().unwrap();
    let (ledger, orders, customers, main_rs) = build_workspace(dir.path());

    let (coord_addr, _c) = spawn_ephemeral("127.0.0.1:0", None).await.unwrap();
    let coord = CoordinatorClient::connect(coord_addr).await.unwrap();
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

    let (w1, _h1) = spawn_ephemeral_worker("127.0.0.1:0", &coord_addr.to_string(), c1.path())
        .await
        .unwrap();
    let (w2, _h2) = spawn_ephemeral_worker("127.0.0.1:0", &coord_addr.to_string(), c2.path())
        .await
        .unwrap();

    let coord_s = coord_addr.to_string();
    let workers = vec![w1.to_string(), w2.to_string()];
    let orders_id = orders.id;
    let customers_id = customers.id;
    let main_rs_id = main_rs.id;

    // Direct expectations from the in-process ledger.
    let want_orders = ledger.get_object(&orders_id).unwrap().unwrap();
    let want_all: usize = ledger.object_count().unwrap();
    let want_hist = ledger.object_history(&orders_id).unwrap().len();
    let want_rels = ledger.relationships_for(&customers_id).unwrap().len();

    // The gateway's `KnowledgeStore` calls are sync; run them off the test runtime so the gateway
    // uses its own runtime (no ambient handle on a plain thread).
    let handle = std::thread::spawn(move || {
        let g = DistributedLedger::open(coord_s, workers).unwrap();

        let got_orders = g
            .get_object(&orders_id)
            .unwrap()
            .expect("orders via gateway");
        assert_eq!(
            serde_json::to_value(&want_orders).unwrap(),
            serde_json::to_value(&got_orders).unwrap(),
        );
        assert_eq!(got_orders.name, "orders");
        assert_eq!(
            got_orders.properties.get("owner").and_then(|v| v.as_str()),
            Some("data-eng"),
            "gateway returns the newest version",
        );

        assert!(
            g.get_object(&main_rs_id).unwrap().is_some(),
            "cross-partition object"
        );
        assert_eq!(g.object_count().unwrap(), want_all);
        assert_eq!(g.all_objects().unwrap().len(), want_all);
        assert_eq!(g.object_history(&orders_id).unwrap().len(), want_hist);
        assert_eq!(g.relationships_for(&customers_id).unwrap().len(), want_rels);
        assert_eq!(g.relationship_count().unwrap(), 1);

        let now = Utc::now();
        assert!(g.object_at(&orders_id, now).unwrap().is_some());

        // writes are rejected
        assert!(
            g.append_object(&KirObject::new("nope", ObjectKind::Table))
                .is_err()
        );

        // unknown id → clean None, not an error
        assert!(g.get_object(&ekos_kir::KirId::new()).unwrap().is_none());
    });
    handle.join().unwrap();

    let _ = Arc::new(()); // keep imports tidy
}
