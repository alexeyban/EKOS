//! RFC 0126 (Phase 7 of RFC 0118) — the retrieval scoreboard.
//!
//! `cargo bench --bench retrieval_eval` prints the full quality table (Recall@10 / MRR /
//! nDCG@10, overall and per `QueryType`, plus intent-classifier accuracy) once, then times the
//! two hot paths — query understanding and the fused `retrieve` — over the reference query set.
//! The quality gate itself lives in `ekos-runtime`'s `tests/retrieval_eval.rs` (the normal
//! `cargo test` CI job); this is the human "show me the numbers + how fast is it" command.

use criterion::{Criterion, criterion_group, criterion_main};
use ekos_ledger::FactLedger;
use ekos_runtime::retrieval::understand;
use ekos_runtime::{RetrievalRequest, Runtime, retrieval_eval};

/// Seed the reference estate + mock vector index once; return the temp dir (keep it alive), the
/// ledger, and a query embedder.
fn setup() -> (tempfile::TempDir, FactLedger, Box<dyn Fn(&str) -> Vec<f32>>) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("fl");
    let fl = FactLedger::open(&root).unwrap();
    retrieval_eval::seed_reference_estate(&fl);
    let embed = rt.block_on(retrieval_eval::seed_reference_vectors(
        &fl,
        &root.join("vectors"),
    ));
    (dir, fl, Box::new(embed))
}

fn bench_retrieval_eval(c: &mut Criterion) {
    let (_dir, fl, embed) = setup();
    let runtime = Runtime::over(&fl);
    let queries = retrieval_eval::reference_queries();

    // Print the scoreboard once, before timing.
    let report = retrieval_eval::evaluate(&runtime, queries, Some(embed.as_ref()));
    println!("\n─── retrieval eval (RFC 0126) ───\n{report}");
    match retrieval_eval::check_regression(&report, &retrieval_eval::BASELINE, 0.02) {
        Ok(()) => println!("baseline: OK (tol 0.02)\n"),
        Err(drops) => println!("baseline: REGRESSED\n{drops}\n"),
    }

    c.bench_function("retrieval_eval_understand_all", |b| {
        b.iter(|| {
            for q in queries {
                let _ = understand(q.query, &runtime);
            }
        });
    });

    c.bench_function("retrieval_eval_retrieve_all_hybrid", |b| {
        b.iter(|| {
            for q in queries {
                let mut req = RetrievalRequest::lexical(q.query);
                req.query_embedding = Some(embed(q.query));
                let _ = runtime.retrieve(&req);
            }
        });
    });

    c.bench_function("retrieval_eval_retrieve_all_lexical", |b| {
        b.iter(|| {
            for q in queries {
                let _ = runtime.retrieve(&RetrievalRequest::lexical(q.query));
            }
        });
    });
}

criterion_group!(benches, bench_retrieval_eval);
criterion_main!(benches);
