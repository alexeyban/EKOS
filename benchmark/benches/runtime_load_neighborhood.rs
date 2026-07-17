use criterion::{Criterion, criterion_group, criterion_main};
use ekos_kir::{KirObject, KirRelationship, ObjectKind, RelationshipKind};
use ekos_ledger::Ledger;
use ekos_runtime::Runtime;

fn seed_ledger(ledger: &Ledger, n: usize) -> ekos_kir::KirId {
    let objects: Vec<KirObject> = (0..n)
        .map(|i| KirObject::new(format!("table_{i}"), ObjectKind::Table))
        .collect();
    for obj in &objects {
        ledger.append_object(obj).unwrap();
    }
    // Chain relationships: table_0 -> table_1 -> table_2 -> ...
    for pair in objects.windows(2) {
        ledger
            .append_relationship(&KirRelationship::new(
                RelationshipKind::ForeignKey,
                pair[0].id,
                pair[1].id,
            ))
            .unwrap();
    }
    objects[0].id
}

fn bench_load_neighborhood(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let ledger = Ledger::open(&dir.path().join("bench.db")).unwrap();
    let root_id = seed_ledger(&ledger, 50);
    let runtime = Runtime::new(&ledger);

    c.bench_function("runtime_load_neighborhood_depth_2", |b| {
        b.iter(|| runtime.load_neighborhood(&root_id, 2).unwrap());
    });
}

criterion_group!(benches, bench_load_neighborhood);
criterion_main!(benches);
