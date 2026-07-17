use criterion::{Criterion, criterion_group, criterion_main};
use ekos_kir::{KirGraph, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use ekos_semantic::build_ckm;

fn fixture_graph(n: usize) -> KirGraph {
    let mut graph = KirGraph::new();
    let mut ids = Vec::with_capacity(n);
    for i in 0..n {
        let id = graph.add_object(KirObject::new(format!("table_{i}"), ObjectKind::Table));
        ids.push(id);
    }
    for pair in ids.windows(2) {
        graph.relationships.push(KirRelationship::new(
            RelationshipKind::ForeignKey,
            pair[0],
            pair[1],
        ));
    }
    graph
}

fn bench_semantic_compiler(c: &mut Criterion) {
    let graph = fixture_graph(50);

    c.bench_function("semantic_compiler_build_ckm_50_objects", |b| {
        b.iter(|| build_ckm(&graph));
    });
}

criterion_group!(benches, bench_semantic_compiler);
criterion_main!(benches);
