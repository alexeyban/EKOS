//! RFC 0113 B5 — distributed search. Matching objects split across two partitions on two
//! different workers must come back as one score-ordered, de-duplicated, `k`-bounded list.

use chrono::Utc;
use ekos_cluster::{CoordinatorClient, PartitionLocation, spawn_ephemeral};
use ekos_distributed::{DistributedLedger, partition_id, spawn_ephemeral_worker};
use ekos_kir::{KirObject, ObjectKind};
use ekos_ledger::partitioned::{PartitionDimension, PartitionedLedger, TimeBucket};
use tempfile::tempdir;

fn build(root: &std::path::Path) -> PartitionedLedger {
    let part_root = root.to_path_buf();
    let ledger = PartitionedLedger::new(
        root.to_path_buf(),
        PartitionDimension::EntityKind,
        TimeBucket::Monthly,
        move |key| part_root.join(&key.dimension_value).join(&key.time_bucket),
    )
    .unwrap();
    // Table partition: two "orders" matches, exact name should outrank the compound one.
    ledger
        .append_object(&KirObject::new("orders", ObjectKind::Table))
        .unwrap();
    ledger
        .append_object(&KirObject::new("orders_archive", ObjectKind::Table))
        .unwrap();
    ledger
        .append_object(&KirObject::new("shipping", ObjectKind::Table))
        .unwrap();
    // File partition: one "orders" match — a cross-partition hit.
    ledger
        .append_object(&KirObject::new("orders.md", ObjectKind::File))
        .unwrap();
    ledger
        .append_object(&KirObject::new("readme.md", ObjectKind::File))
        .unwrap();
    // Commit each partition's tantivy index so a separate read-only opener (the worker) sees it.
    ledger.find_objects("orders").unwrap();
    ledger
}

#[tokio::test(flavor = "multi_thread")]
async fn distributed_search_merges_top_k_across_partitions() {
    let dir = tempdir().unwrap();
    let c1 = tempdir().unwrap();
    let c2 = tempdir().unwrap();
    let ledger = build(dir.path());

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

    // in-process expectation (set of ids, order-independent — PartitionedLedger doesn't rank-merge)
    let want_ids: std::collections::HashSet<_> = ledger
        .find_objects("orders")
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(want_ids.len(), 3, "orders / orders_archive / orders.md");

    let handle = std::thread::spawn(move || {
        let g = DistributedLedger::open(coord_s, workers).unwrap();

        let hits = g.search("orders", 10).unwrap();
        assert_eq!(hits.len(), 3, "all three matches, merged and de-duplicated");

        // The merge sort produces a globally score-ordered list...
        for w in hits.windows(2) {
            assert!(w[0].2 >= w[1].2, "scores must be non-increasing: {hits:?}");
        }
        // ...over BM25 scores that are *shard-local* (per-partition term statistics — RFC 0111
        // §7's accepted query-then-fetch approximation). So the top hit is whichever partition
        // scored its match highest locally, NOT necessarily the exact-name `orders`.
        let names: Vec<&str> = hits.iter().map(|(_, n, _)| n.as_str()).collect();
        assert!(names.contains(&"orders"));
        assert!(names.contains(&"orders_archive"));
        assert!(
            names.contains(&"orders.md"),
            "cross-partition (File) hit is merged in"
        );

        // ids unique, and exactly the set the in-process ledger returns
        let ids: std::collections::HashSet<_> = hits.iter().map(|(id, _, _)| *id).collect();
        assert_eq!(ids.len(), hits.len());
        assert_eq!(ids, want_ids);

        // k actually bounds the merged result
        let one = g.search("orders", 1).unwrap();
        assert_eq!(one.len(), 1);
        assert!(names.contains(&one[0].1.as_str()));

        // the trait method rides on the same merge
        let via_trait: std::collections::HashSet<_> = {
            use ekos_ledger::KnowledgeStore;
            g.find_objects("orders")
                .unwrap()
                .into_iter()
                .map(|(id, _)| id)
                .collect()
        };
        assert_eq!(via_trait, want_ids);

        // a miss is empty, not an error
        assert!(g.search("zzzznomatch", 5).unwrap().is_empty());
    });
    handle.join().unwrap();

    let _ = Utc::now();
}
