use criterion::{criterion_group, criterion_main, Criterion};
use ekos_kir::{KirObject, ObjectKind};
use ekos_ledger::Ledger;

fn bench_ledger_write(c: &mut Criterion) {
    c.bench_function("ledger_append_object", |b| {
        let dir = tempfile::tempdir().unwrap();
        let ledger = Ledger::open(&dir.path().join("bench.db")).unwrap();
        let mut i = 0u64;
        b.iter(|| {
            let obj = KirObject::new(format!("table_{i}"), ObjectKind::Table);
            ledger.append_object(&obj).unwrap();
            i += 1;
        });
    });
}

criterion_group!(benches, bench_ledger_write);
criterion_main!(benches);
