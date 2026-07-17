//! RFC 0016 Phase 1: fact decomposition/reconstruction throughput, and the
//! size of a semantic delta versus a whole-payload copy.

use criterion::{Criterion, criterion_group, criterion_main};
use ekos_kir::{KirId, KirObject, ObjectKind};
use ekos_ledger::fact::{AttributeRegistry, decompose, diff, reconstruct};

fn realistic_object(i: usize) -> KirObject {
    KirObject::new(
        format!("src/module_{}/handler_{i}.rs", i % 40),
        ObjectKind::File,
    )
    .with_property(
        "path",
        serde_json::json!(format!("src/module_{}/handler_{i}.rs", i % 40)),
    )
    .with_property("size_bytes", serde_json::json!(1024 + i * 37))
    .with_property(
        "excerpt",
        serde_json::json!(
            "Reconciles inbound events against the projection store; retries with backoff."
        ),
    )
    .with_evidence(KirId::new())
}

fn bench_fact_model(c: &mut Criterion) {
    let obj = realistic_object(7);
    let payload = serde_json::to_value(&obj).unwrap();

    c.bench_function("fact_decompose", |b| {
        let mut reg = AttributeRegistry::new();
        b.iter(|| decompose(obj.id.0, &payload, &mut reg).unwrap());
    });

    c.bench_function("fact_reconstruct", |b| {
        let mut reg = AttributeRegistry::new();
        let facts = decompose(obj.id.0, &payload, &mut reg).unwrap();
        b.iter(|| reconstruct(&facts, &reg).unwrap());
    });

    c.bench_function("fact_diff_one_property", |b| {
        let mut reg = AttributeRegistry::new();
        let old = decompose(obj.id.0, &payload, &mut reg).unwrap();
        let mut changed = obj.clone();
        changed
            .properties
            .insert("size_bytes".into(), serde_json::json!(9999));
        let new = decompose(obj.id.0, &serde_json::to_value(&changed).unwrap(), &mut reg).unwrap();
        b.iter(|| diff(&old, &new));
    });
}

criterion_group!(benches, bench_fact_model);
criterion_main!(benches);
