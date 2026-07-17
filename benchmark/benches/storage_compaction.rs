//! RFC 0015 storage instrumentation: bytes per 1K realistic objects, plus
//! read-path latency (`get_object`, `find_objects`) over that corpus. The
//! byte counts print to stderr during setup; the latency gates are criterion
//! benchmarks so regressions show up like any other bench.

use criterion::{criterion_group, criterion_main, Criterion};
use ekos_kir::{KirId, KirObject, ObjectKind};
use ekos_ledger::Ledger;
use std::path::Path;

const CORPUS_SIZE: usize = 1_000;

/// A file-shaped object like the ones `ekos build` writes: path, size,
/// artifact id, and a 600-char excerpt (RFC 0014).
fn realistic_object(i: usize) -> KirObject {
    let path = format!("src/module_{}/handler_{i}.rs", i % 40);
    let excerpt = format!(
        "//! Handler {i}: reconciles inbound events against the projection \
         store. Retries with exponential backoff; poison messages park in \
         the dead-letter queue after five attempts. {}",
        "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(7)
    );
    KirObject::new(&path, ObjectKind::File)
        .with_property("path", serde_json::Value::String(path.clone()))
        .with_property("size_bytes", serde_json::json!(1024 + i * 37))
        .with_property("artifact_id", serde_json::Value::String(format!("{:064x}", i)))
        .with_property("excerpt", serde_json::Value::String(excerpt[..600.min(excerpt.len())].to_string()))
}

fn ledger_file_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

fn populated_ledger() -> (Ledger, Vec<KirId>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let ledger = Ledger::open(&dir.path().join("bench.db")).unwrap();
    let mut ids = Vec::with_capacity(CORPUS_SIZE);
    for i in 0..CORPUS_SIZE {
        let obj = realistic_object(i);
        ids.push(obj.id);
        ledger.append_object(&obj).unwrap();
    }
    (ledger, ids, dir)
}

fn bench_storage(c: &mut Criterion) {
    let (ledger, ids, dir) = populated_ledger();

    let bytes = ledger_file_bytes(dir.path());
    eprintln!(
        "storage_compaction: {CORPUS_SIZE} objects -> {bytes} bytes on disk \
         ({:.0} bytes/object)",
        bytes as f64 / CORPUS_SIZE as f64
    );
    for (table, table_bytes) in ledger.storage_stats().unwrap() {
        eprintln!("storage_compaction:   {table}: {table_bytes} bytes");
    }

    c.bench_function("storage_get_object", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let obj = ledger.get_object(&ids[i % ids.len()]).unwrap();
            i += 1;
            obj
        });
    });

    c.bench_function("storage_find_objects", |b| {
        b.iter(|| ledger.find_objects("backoff").unwrap());
    });
}

criterion_group!(benches, bench_storage);
criterion_main!(benches);
