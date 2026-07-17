//! RFC 0016 Phase 3: ranged-scan latency over index runs at estate-like
//! scale — the read path that replaces N-queries-per-hop graph traversal.

use criterion::{Criterion, criterion_group, criterion_main};
use ekos_kir::{KirObject, ObjectKind};
use ekos_ledger::fact::TxId;
use ekos_ledger::fact::{AttributeRegistry, FactOp, FactValue, decompose};
use ekos_ledger::index::{FactIndexes, IndexEntry, ScanPrefix, SortOrder};
use uuid::Uuid;

const OBJECTS: usize = 5_000;

fn build_indexes(dir: &std::path::Path) -> (FactIndexes, AttributeRegistry, Vec<Uuid>) {
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
    idx.add_runs("bench", &entries).unwrap();
    for order in SortOrder::ALL {
        idx.merge_runs(order).unwrap();
    }
    (idx, reg, ids)
}

fn bench_index_runs(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let (idx, mut reg, ids) = build_indexes(dir.path());
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
