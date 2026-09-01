//! RFC 0126 (Phase 7 of RFC 0118) — the CI gate.
//!
//! Runs in the normal `cargo test --workspace` job, so a change that drops Recall@10 / MRR /
//! nDCG@10 / intent accuracy more than the tolerance below [`retrieval_eval::BASELINE`] fails the
//! build. Regenerate the baseline with
//! `cargo test -p ekos-runtime retrieval_eval::tests::print_current -- --ignored --nocapture`.

use ekos_ledger::FactLedger;
use ekos_runtime::{Runtime, retrieval_eval};

#[tokio::test]
async fn retrieval_quality_meets_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("fl");
    let fl = FactLedger::open(&root).unwrap();

    retrieval_eval::seed_reference_estate(&fl);
    let embed_query = retrieval_eval::seed_reference_vectors(&fl, &root.join("vectors")).await;

    let rt = Runtime::over(&fl);
    let report =
        retrieval_eval::evaluate(&rt, retrieval_eval::reference_queries(), Some(&embed_query));

    retrieval_eval::check_regression(&report, &retrieval_eval::BASELINE, 0.02)
        .unwrap_or_else(|drops| panic!("retrieval quality regressed:\n{drops}\n\n{report}"));

    // Sanity floors independent of the tunable baseline.
    assert!(
        report.overall.recall_at_10 >= 0.70,
        "overall Recall@10 unexpectedly low: {report}"
    );
    assert!(
        report.intent_accuracy >= 0.80,
        "intent classifier accuracy dropped: {report}"
    );
}

/// The lexical stack alone (no query embedder) must still clear a lower bar — proves the harness
/// isn't wholly dependent on the mock vector arm.
#[tokio::test]
async fn lexical_only_stack_is_still_usable() {
    let dir = tempfile::tempdir().unwrap();
    let fl = FactLedger::open(&dir.path().join("fl")).unwrap();
    retrieval_eval::seed_reference_estate(&fl);
    let rt = Runtime::over(&fl);

    let report = retrieval_eval::evaluate(&rt, retrieval_eval::reference_queries(), None);
    // Lookup + Lexical carry the lexical-only run; Conceptual largely collapses without vectors.
    assert!(
        report.overall.recall_at_10 >= 0.45,
        "lexical-only Recall@10 collapsed: {report}"
    );
}
