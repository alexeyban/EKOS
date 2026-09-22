//! RFC 0112 — the cost claim the RFC has to make true: a cheap watermark-based refresh versus
//! the two things it replaces in `StoreCache` (a `walkdir` fingerprint of the whole store on
//! every call, and a full cold reopen on any change).
use criterion::{Criterion, criterion_group, criterion_main};
use ekos_kir::{KirObject, ObjectKind};
use ekos_ledger::FactLedger;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

/// The pre-RFC-0112 freshness check, copied verbatim from `crates/cli/src/commands/mcp.rs`.
fn store_fingerprint(root: &Path) -> Option<SystemTime> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
}

fn seeded(n: usize) -> (tempfile::TempDir, std::path::PathBuf, FactLedger) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("facts");
    // A small seal threshold gives the store a realistic multi-file layout (many sealed segments
    // plus index runs), which is exactly what makes the directory walk expensive.
    let writer = FactLedger::open_with_seal_threshold(&path, 64 * 1024).unwrap();
    for i in 0..n {
        writer
            .append_object(&KirObject::new(format!("table_{i}"), ObjectKind::Table))
            .unwrap();
    }
    (dir, path, writer)
}

fn bench_refresh(c: &mut Criterion) {
    let (_dir, path, writer) = seeded(5_000);
    let mut g = c.benchmark_group("ledger_freshness_5k_objects");
    g.sample_size(30);

    g.bench_function("old_walkdir_fingerprint", |b| b.iter(|| store_fingerprint(&path)));

    let reader = FactLedger::open_read_only(&path).unwrap();
    g.bench_function("new_refresh_snapshot_unchanged", |b| {
        b.iter(|| reader.refresh_snapshot().unwrap())
    });

    g.bench_function("old_full_cold_reopen", |b| {
        b.iter(|| FactLedger::open_read_only(&path).unwrap())
    });

    let mut i = 0u64;
    g.bench_function("new_refresh_snapshot_after_one_batch", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                writer
                    .append_object(&KirObject::new(format!("late_{i}"), ObjectKind::Table))
                    .unwrap();
                i += 1;
                let t = Instant::now();
                reader.refresh_snapshot().unwrap();
                total += t.elapsed();
            }
            total
        })
    });
    g.finish();
}

criterion_group!(benches, bench_refresh);
criterion_main!(benches);
