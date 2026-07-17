//! RFC 0016 Phase 4: FactLedger vs the SQLite backend on the same
//! operations — the parity scoreboard for the backend swap decision.

use criterion::{Criterion, criterion_group, criterion_main};
use ekos_kir::{KirId, KirObject, ObjectKind};
use ekos_ledger::{FactLedger, Ledger};

fn object(i: usize) -> KirObject {
    KirObject::new(
        format!("src/module_{}/file_{i}.rs", i % 40),
        ObjectKind::File,
    )
    .with_property("size_bytes", serde_json::json!(1024 + i))
    .with_property(
        "excerpt",
        serde_json::json!("Reconciles inbound events against the projection store."),
    )
}

fn bench_fact_ledger(c: &mut Criterion) {
    c.bench_function("fact_ledger_append_object", |b| {
        let dir = tempfile::tempdir().unwrap();
        let ledger = FactLedger::open(&dir.path().join("fl")).unwrap();
        let mut i = 0usize;
        b.iter(|| {
            ledger.append_object(&object(i)).unwrap();
            i += 1;
        });
    });

    let populated: (FactLedger, Ledger, Vec<KirId>, tempfile::TempDir) = {
        let dir = tempfile::tempdir().unwrap();
        let fl = FactLedger::open(&dir.path().join("fl")).unwrap();
        let sq = Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let mut ids = Vec::new();
        for i in 0..1_000 {
            let o = object(i);
            ids.push(o.id);
            fl.append_object(&o).unwrap();
            sq.append_object(&o).unwrap();
        }
        (fl, sq, ids, dir)
    };
    let (fl, sq, ids, _dir) = populated;

    c.bench_function("fact_ledger_get_object", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let o = fl.get_object(&ids[i % ids.len()]).unwrap();
            i += 1;
            o
        });
    });

    c.bench_function("sqlite_get_object_baseline", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let o = sq.get_object(&ids[i % ids.len()]).unwrap();
            i += 1;
            o
        });
    });

    c.bench_function("fact_ledger_find_objects", |b| {
        b.iter(|| fl.find_objects("reconciles").unwrap());
    });
}

criterion_group!(benches, bench_fact_ledger);
criterion_main!(benches);
