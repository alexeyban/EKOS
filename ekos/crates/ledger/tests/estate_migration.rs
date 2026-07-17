//! Estate-scale migration measurement (RFC 0016 §8 storage gate).
//!
//! Ignored by default: runs only when `target/estate-v2-path.txt` (gitignored,
//! machine-local) names a copy of a v2 `ledger.db`. Never point it at a live
//! workspace ledger — migration writes a sibling `facts-gate-test` store next
//! to the source file.
//!
//! ```bash
//! echo /path/to/copy/of/ledger.db > target/estate-v2-path.txt
//! cargo test --release -p ekos-ledger --test estate_migration -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

fn dir_bytes(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += dir_bytes(&path);
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

#[test]
#[ignore = "estate-scale; needs target/estate-v2-path.txt"]
fn migrate_estate_and_report_sizes() {
    let path_file =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/estate-v2-path.txt");
    let Ok(src_text) = std::fs::read_to_string(&path_file) else {
        eprintln!("skip: {} not present", path_file.display());
        return;
    };
    let src = PathBuf::from(src_text.trim());
    assert!(src.exists(), "source ledger {} missing", src.display());

    let dest = src.parent().unwrap().join("facts-gate-test");
    if dest.exists() {
        std::fs::remove_dir_all(&dest).unwrap();
    }

    let started = std::time::Instant::now();
    let report = ekos_ledger::migrate_to_v3(&src, &dest).expect("migration must succeed");
    let elapsed = started.elapsed();

    println!("== RFC 0016 storage gate ==");
    println!("versions        : {}", report.versions);
    println!("objects         : {}", report.objects);
    println!("relationships   : {}", report.relationships);
    println!("elapsed         : {elapsed:?}");
    println!("v2 ledger.db    : {:8.1} MB", mb(report.bytes_before));
    println!("fact store total: {:8.1} MB", mb(report.bytes_after));
    for part in ["segments", "indexes", "search"] {
        println!("  {part:<14}: {:8.1} MB", mb(dir_bytes(&dest.join(part))));
    }
    let ratio = report.bytes_before as f64 / report.bytes_after.max(1) as f64;
    println!("ratio           : {ratio:.2}x (gate: >= 2.0x)");
}
