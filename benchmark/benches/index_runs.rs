//! RFC 0016 Phase 3: ranged-scan latency over index runs at estate-like
//! scale — the read path that replaces N-queries-per-hop graph traversal.

use criterion::{Criterion, criterion_group, criterion_main};
use ekos_kir::{KirObject, ObjectKind};
use ekos_ledger::fact::TxId;
use ekos_ledger::fact::{AttributeRegistry, FactOp, FactValue, decompose};
use ekos_ledger::index::{FactIndexes, IndexEntry, ScanPrefix, SortOrder};
use uuid::Uuid;

const OBJECTS: usize = 5_000;

/// `runs`: how many EAVT runs the same data is spread over. 1 is a fully merged index; 8 is
/// `MERGE_RUNS_AT`, the most an unmerged ledger holds (and what EKOS's own ledger had when
/// RFC 0168 was measured).
fn build_indexes(
    dir: &std::path::Path,
    runs: usize,
) -> (FactIndexes, AttributeRegistry, Vec<Uuid>) {
    let mut reg = AttributeRegistry::new();
    let mut entries = Vec::new();
    let mut ids = Vec::new();
    for i in 0..OBJECTS {
        let obj = KirObject::new(
            format!("src/module_{}/file_{i}.rs", i % 40),
            ObjectKind::File,
        )
        .with_property("size_bytes", serde_json::json!(i))
        .with_evidence(ekos_kir::KirId(uuid::Uuid::from_u128(i as u128)));
        ids.push(obj.id.0);
        let facts = decompose(obj.id.0, &serde_json::to_value(&obj).unwrap(), &mut reg).unwrap();
        entries.extend(
            facts
                .iter()
                .map(|f| IndexEntry::from_fact(f, TxId(i as u64), FactOp::Assert)),
        );
    }
    let mut idx = FactIndexes::open(dir).unwrap().0;
    if runs == 1 {
        idx.add_runs("bench", &entries).unwrap();
        for order in SortOrder::ALL {
            idx.merge_runs(order).unwrap();
        }
    } else {
        // Entities land in runs in write order, as successive builds would put them.
        for (n, chunk) in entries.chunks(entries.len().div_ceil(runs)).enumerate() {
            idx.add_runs(&format!("bench{n}"), chunk).unwrap();
        }
    }
    (idx, reg, ids)
}

fn bench_index_runs(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let (idx, mut reg, ids) = build_indexes(dir.path(), 1);
    let evidence_attr = reg.intern("evidence");

    c.bench_function("index_eavt_entity_scan", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let hits = idx
                .scan(&ScanPrefix::Entity {
                    entity: ids[i % ids.len()],
                    attr: None,
                })
                .unwrap();
            i += 1;
            hits
        });
    });

    // RFC 0168 gate: 8 unmerged runs within 1.5x of the merged scan above. Before the per-run
    // entity filter, every run cost one block decode whether or not it held the entity.
    let dir8 = tempfile::tempdir().unwrap();
    let (idx8, _, ids8) = build_indexes(dir8.path(), 8);
    assert_eq!(idx8.run_count(SortOrder::Eavt), 8);
    c.bench_function("index_eavt_entity_scan_8_runs", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let hits = idx8
                .scan(&ScanPrefix::Entity {
                    entity: ids8[i % ids8.len()],
                    attr: None,
                })
                .unwrap();
            i += 1;
            hits
        });
    });

    // AVET indexes ref values only (RFC 0016 §7) — bench the graph hop.
    c.bench_function("index_avet_ref_lookup", |b| {
        b.iter(|| {
            idx.scan(&ScanPrefix::AttrValue {
                attr: evidence_attr,
                value: FactValue::Ref(uuid::Uuid::from_u128(2_887)),
            })
            .unwrap()
        });
    });
}

criterion_group!(benches, bench_index_runs);
criterion_main!(benches);
