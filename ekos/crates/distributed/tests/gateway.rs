//! RFC 0113 B4b — the `DistributedLedger` gateway must answer `KnowledgeStore` reads the same
//! way the in-process `PartitionedLedger` does, fanning across two query workers.

use std::sync::Arc;

use chrono::Utc;
use ekos_cluster::{CoordinatorClient, PartitionLocation, spawn_ephemeral};
use ekos_distributed::{DistributedLedger, partition_id, spawn_ephemeral_worker};
use ekos_kir::{
    KirEvidence, KirObject, KirRelationship, ObjectKind, RelationshipKind, SourceLocation,
};
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
    // RFC 0136 Phase 7 — real evidence, so `evidence_count` (a new distributed RPC, previously
    // an Err stub) has something real to fan out over and sum.
    ledger
        .append_evidence(&KirEvidence::new(
            SourceLocation::file("schema.sql"),
            "CREATE TABLE orders (...)",
        ))
        .unwrap();
    ledger
        .append_evidence(&KirEvidence::new(
            SourceLocation::file("schema.sql"),
            "CREATE TABLE customers (...)",
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
    let want_evidence: usize = ledger.evidence_count().unwrap();

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
        assert_eq!(
            g.evidence_count().unwrap(),
            want_evidence,
            "RFC 0136 Phase 7 — evidence_count now fans out over the distributed gateway"
        );

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

/// F4 (test-runs/run-20260901T160842Z): `arm_timings` (RFC 0126) was always empty on the
/// distributed gateway's `retrieve` — the query-worker RPC doesn't carry a worker-internal arm
/// breakdown over the wire, so this measures at the gateway boundary (the fan-out round trip for
/// Bm25, local compute for ExactName) instead of leaving it empty outright.
#[tokio::test(flavor = "multi_thread")]
async fn gateway_retrieve_populates_arm_timings() {
    let dir = tempdir().unwrap();
    let c1 = tempdir().unwrap();
    let c2 = tempdir().unwrap();
    let (ledger, _orders, _customers, _main_rs) = build_workspace(dir.path());

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

    let handle = std::thread::spawn(move || {
        let g = DistributedLedger::open(coord_s, workers).unwrap();
        let results = g
            .retrieve(&ekos_ledger::RetrievalRequest::lexical("orders"))
            .unwrap();
        assert!(
            !results.arm_timings.is_empty(),
            "arm_timings must be populated on the distributed gateway, not left empty"
        );
        assert!(
            results
                .arm_timings
                .iter()
                .any(|t| t.source == ekos_ledger::SignalSource::Bm25),
            "a Bm25 arm timing must be present"
        );
        assert!(
            results.arm_timings.iter().all(|t| t.elapsed_ms >= 0.0),
            "every timing must be a real non-negative measurement"
        );
    });
    handle.join().unwrap();
}

/// RFC 0113 v1.1 — when the coordinator's `entity_id → partitions` index has an entry for an id,
/// the gateway must actually use it to prune, not just fall back to a full class scan. Proven by
/// deliberately mis-registering `orders`' id against a partition that does *not* hold it: if the
/// gateway silently fell back to scanning every object partition (as v1 always did), `get_object`
/// would still find `orders` in its real partition and this test would wrongly pass. It must
/// instead trust the (wrong) index and return `None`. A correctly-indexed id is checked in the
/// same test to confirm pruning doesn't break the common case.
#[tokio::test(flavor = "multi_thread")]
async fn gateway_uses_the_entity_index_to_prune_when_present() {
    let dir = tempdir().unwrap();
    let c1 = tempdir().unwrap();
    let c2 = tempdir().unwrap();
    let (ledger, orders, _customers, main_rs) = build_workspace(dir.path());

    let (coord_addr, _c) = spawn_ephemeral("127.0.0.1:0", None).await.unwrap();
    let coord = CoordinatorClient::connect(coord_addr).await.unwrap();
    let mut table_pid = None;
    let mut file_pid = None;
    for key in ledger.catalog_partition_keys() {
        let root = ledger.partition_root(&key).unwrap();
        let pid = partition_id(&key);
        coord
            .register_partition(
                &pid,
                PartitionLocation::Local {
                    root: root.to_string_lossy().into_owned(),
                },
            )
            .await
            .unwrap();
        match key.dimension_value.as_str() {
            "Table" => table_pid = Some(pid),
            "File" => file_pid = Some(pid),
            _ => {}
        }
    }
    let table_pid = table_pid.expect("orders/customers partition");
    let file_pid = file_pid.expect("main.rs partition");

    // Wrong on purpose: orders actually lives in `table_pid`, not `file_pid`.
    coord
        .record_entity_partitions(&orders.id.to_string(), std::slice::from_ref(&file_pid))
        .await
        .unwrap();
    // Correct: main.rs really does live in `file_pid`.
    coord
        .record_entity_partitions(&main_rs.id.to_string(), std::slice::from_ref(&file_pid))
        .await
        .unwrap();

    let (w1, _h1) = spawn_ephemeral_worker("127.0.0.1:0", &coord_addr.to_string(), c1.path())
        .await
        .unwrap();
    let (w2, _h2) = spawn_ephemeral_worker("127.0.0.1:0", &coord_addr.to_string(), c2.path())
        .await
        .unwrap();

    let coord_s = coord_addr.to_string();
    let workers = vec![w1.to_string(), w2.to_string()];
    let orders_id = orders.id;
    let main_rs_id = main_rs.id;

    let handle = std::thread::spawn(move || {
        let g = DistributedLedger::open(coord_s, workers).unwrap();

        assert!(
            g.get_object(&orders_id).unwrap().is_none(),
            "a mis-registered index entry must be trusted, not silently bypassed by a full scan"
        );
        assert_eq!(
            g.get_object(&main_rs_id).unwrap().unwrap().name,
            "main.rs",
            "a correctly-registered index entry must still resolve"
        );
    });
    handle.join().unwrap();

    let _ = table_pid; // only needed to prove it exists; the gateway never sees it for `orders`
}

/// ISSUE-2 regression (2026-09-01): when one query worker is unreachable, the gateway must fail
/// over to another — every worker can materialise and serve any partition, so a dead worker is
/// not a dead partition. Before the fix, a single `kill -9`'d worker made every gateway query
/// return `io error: Connection refused`.
#[tokio::test(flavor = "multi_thread")]
async fn gateway_fails_over_when_a_query_worker_is_down() {
    let dir = tempdir().unwrap();
    let c1 = tempdir().unwrap();
    let c2 = tempdir().unwrap();
    let (ledger, orders, _customers, _main_rs) = build_workspace(dir.path());
    // Force each partition's tantivy index to commit so a read-only opener (the worker) sees it.
    ledger.find_objects("orders").unwrap();

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
    let want_all: usize = ledger.object_count().unwrap();

    let (w1, h1) = spawn_ephemeral_worker("127.0.0.1:0", &coord_addr.to_string(), c1.path())
        .await
        .unwrap();
    let (w2, _h2) = spawn_ephemeral_worker("127.0.0.1:0", &coord_addr.to_string(), c2.path())
        .await
        .unwrap();

    // Kill worker 1 — abort the serve task, freeing its port so connects are refused.
    h1.abort();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let coord_s = coord_addr.to_string();
    let workers = vec![w1.to_string(), w2.to_string()]; // dead worker still listed first
    let orders_id = orders.id;

    let handle = std::thread::spawn(move || {
        let g = DistributedLedger::open(coord_s, workers).unwrap();
        assert_eq!(
            g.object_count().unwrap(),
            want_all,
            "fan-out count survives a dead worker"
        );
        assert!(
            g.get_object(&orders_id).unwrap().is_some(),
            "id-scoped read survives a dead worker"
        );
        assert_eq!(g.all_objects().unwrap().len(), want_all);
        assert!(
            !g.find_objects("orders").unwrap().is_empty(),
            "distributed search survives a dead worker"
        );
    });
    handle.join().unwrap();
}

/// With *every* worker down the gateway must surface a clean error, not hang.
#[tokio::test(flavor = "multi_thread")]
async fn gateway_errors_cleanly_when_all_workers_are_down() {
    let dir = tempdir().unwrap();
    let c1 = tempdir().unwrap();
    let (ledger, orders, ..) = build_workspace(dir.path());
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
    let (w1, h1) = spawn_ephemeral_worker("127.0.0.1:0", &coord_addr.to_string(), c1.path())
        .await
        .unwrap();
    h1.abort();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let coord_s = coord_addr.to_string();
    let workers = vec![w1.to_string()];
    let orders_id = orders.id;
    let handle = std::thread::spawn(move || {
        let g = DistributedLedger::open(coord_s, workers).unwrap();
        assert!(g.get_object(&orders_id).is_err(), "no worker → clean error");
        assert!(g.object_count().is_err());
    });
    handle.join().unwrap();
}
