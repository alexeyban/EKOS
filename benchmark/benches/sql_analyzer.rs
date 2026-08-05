use criterion::{Criterion, criterion_group, criterion_main};
use ekos_recovery::parse_ddl_structural;
use sqlparser::dialect::GenericDialect;

const ECOMMERCE_SQL: &str = include_str!("../../tests/fixtures/ecommerce.sql");

fn bench_sql_analyzer(c: &mut Criterion) {
    c.bench_function("sql_analyzer_structural_parse_ecommerce", |b| {
        b.iter(|| parse_ddl_structural(ECOMMERCE_SQL, "ecommerce.sql", &GenericDialect {}));
    });
}

criterion_group!(benches, bench_sql_analyzer);
criterion_main!(benches);
