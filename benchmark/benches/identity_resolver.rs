use criterion::{criterion_group, criterion_main, Criterion};
use ekos_identity::{DefaultResolver, IdentityResolver};
use ekos_kir::{KirGraph, KirObject, ObjectKind};

fn fixture_graph(n: usize) -> KirGraph {
    let mut graph = KirGraph::new();
    for i in 0..n {
        graph.add_object(KirObject::new(format!("customer_{i}"), ObjectKind::Table));
        // A near-duplicate name so the resolver has real merge candidates to score.
        graph.add_object(KirObject::new(format!("customers_{i}"), ObjectKind::Table));
    }
    graph
}

fn bench_identity_resolver(c: &mut Criterion) {
    let graph = fixture_graph(25);
    let resolver = DefaultResolver::new();

    c.bench_function("identity_resolver_resolve_50_objects", |b| {
        b.iter(|| resolver.resolve(&graph));
    });
}

criterion_group!(benches, bench_identity_resolver);
criterion_main!(benches);
