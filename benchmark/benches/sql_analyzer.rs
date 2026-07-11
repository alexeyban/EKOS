use criterion::{criterion_group, criterion_main, Criterion};
use ekos_recovery::parse_ddl_structural;

const ECOMMERCE_SQL: &str = include_str!("../../tests/fixtures/ecommerce.sql");

fn bench_sql_analyzer(c: &mut Criterion) {
    c.bench_function("sql_analyzer_structural_parse_ecommerce", |b| {
        b.iter(|| parse_ddl_structural(ECOMMERCE_SQL, "ecommerce.sql"));
    });
}

criterion_group!(benches, bench_sql_analyzer);
criterion_main!(benches);
