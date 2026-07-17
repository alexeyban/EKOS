//! RFC 0016 Phase 2: fact-segment commit latency (fsync-bound, like the
//! SQLite append it will replace) and full replay throughput.

use criterion::{Criterion, criterion_group, criterion_main};
use ekos_kir::{KirObject, ObjectKind};
use ekos_ledger::fact::{AttributeRegistry, FactOp, decompose};
use ekos_ledger::segment::SegmentStore;

fn ops(reg: &mut AttributeRegistry, i: usize) -> Vec<(FactOp, ekos_ledger::fact::Fact)> {
    let obj = KirObject::new(format!("table_{i}"), ObjectKind::Table)
        .with_property("size_bytes", serde_json::json!(i));
    decompose(obj.id.0, &serde_json::to_value(&obj).unwrap(), reg)
        .unwrap()
        .into_iter()
        .map(|f| (FactOp::Assert, f))
        .collect()
}

fn bench_segment_store(c: &mut Criterion) {
    c.bench_function("segment_append_batch", |b| {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SegmentStore::open(dir.path()).unwrap();
        let mut reg = AttributeRegistry::new();
        let mut i = 0usize;
        b.iter(|| {
            store.append(ops(&mut reg, i), i as i64).unwrap();
            i += 1;
        });
    });

    c.bench_function("segment_replay_1k_batches", |b| {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SegmentStore::open(dir.path()).unwrap();
        let mut reg = AttributeRegistry::new();
        for i in 0..1_000 {
            store.append(ops(&mut reg, i), i as i64).unwrap();
        }
        b.iter(|| store.batches().unwrap());
    });
}

criterion_group!(benches, bench_segment_store);
criterion_main!(benches);
