//! RFC 0126 (Phase 7 of RFC 0118) — the retrieval eval harness.
//!
//! Every prior phase (0119–0125) was gated on "the eval harness showing Recall@10 / MRR
//! non-regression" (RFC 0118 §8.1), but that harness was scaffolded ad-hoc and never checked in.
//! This is it, permanently:
//!
//! - [`seed_reference_estate`] builds a fixed ~30-object estate (Northwind tables + FK edges, a
//!   few code modules/symbols carrying `ai_overview` prose, some doc sections) directly into a
//!   [`KnowledgeStore`] — no pipeline, no LLM, deterministic. [`seed_reference_vectors`] then
//!   builds the RFC 0125 [`VectorIndex`](ekos_ledger::vector::VectorIndex) over it with the
//!   deterministic [`MockEmbeddingProvider`](ekos_recovery::MockEmbeddingProvider), so the
//!   `Conceptual` queries exercise a real (if mock-embedded) semantic arm.
//! - [`reference_queries`] is the graded query set: `{query, expected QueryType, relevant object
//!   names}`, keyed on **names** because ids are unstable.
//! - [`evaluate`] runs [`crate::Runtime::retrieve`] + [`crate::retrieval::understand`] over the
//!   set and computes Recall@10 / MRR / nDCG@10, overall and sliced by `QueryType`, plus the
//!   intent-classifier accuracy. Pass a query embedder to light up the vector arm.
//! - [`BASELINE`] + [`check_regression`] are the CI gate: a metric more than `tol` below its
//!   baseline fails the build (see `crates/runtime/tests/retrieval_eval.rs`).
//!
//! The metric math ([`recall_at_k`], [`reciprocal_rank`], [`ndcg_at_k`]) is pure and unit-tested
//! against textbook cases. Regenerate [`BASELINE`] with
//! `cargo test -p ekos-runtime retrieval_eval::print_current -- --ignored --nocapture`.

use crate::retrieval::{QueryType, understand};
use crate::{RetrievalRequest, Runtime};
use ekos_kir::{KirObject, KirRelationship, ObjectKind, RelationshipKind};
use ekos_ledger::KnowledgeStore;
use std::collections::HashMap;

/// One graded query.
#[derive(Debug, Clone, Copy)]
pub struct EvalQuery {
    pub query: &'static str,
    /// The shape [`understand`] should classify this as.
    pub expect_type: QueryType,
    /// Object names a good retrieval must surface in the top-k (binary relevance).
    pub relevant: &'static [&'static str],
}

/// Recall@10 / MRR / nDCG@10 over a set of queries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvalMetrics {
    pub recall_at_10: f64,
    pub mrr: f64,
    pub ndcg_at_10: f64,
    pub n: usize,
}

/// Per-query detail — for `--verbose` output and regenerating the baseline.
#[derive(Debug, Clone)]
pub struct QueryOutcome {
    pub query: &'static str,
    pub expect_type: QueryType,
    pub got_type: QueryType,
    pub recall_at_10: f64,
    pub reciprocal_rank: f64,
    pub ndcg_at_10: f64,
    /// Relevant names that never appeared in the ranked list at all.
    pub missed: Vec<String>,
}

/// Whether a query's answer is retrieval (`retrieve` returns it) rather than a graph traversal or
/// an aggregation. The overall rank metrics are computed over these only — `Structural` /
/// `Aggregate` questions route through REASON / EKL, not SEARCH, so scoring their `retrieve()`
/// output would measure the wrong thing. They still count toward `intent_accuracy`.
fn is_retrieval_type(t: QueryType) -> bool {
    matches!(
        t,
        QueryType::Lookup | QueryType::Lexical | QueryType::Conceptual
    )
}

/// The full result of an [`evaluate`] run.
#[derive(Debug, Clone)]
pub struct EvalReport {
    pub overall: EvalMetrics,
    pub by_type: Vec<(QueryType, EvalMetrics)>,
    pub per_query: Vec<QueryOutcome>,
    /// Fraction of queries where `understand().query_type == expect_type`.
    pub intent_accuracy: f64,
}

impl std::fmt::Display for EvalReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "overall  R@10={:.3}  MRR={:.3}  nDCG@10={:.3}  (n={}, intent_acc={:.3})",
            self.overall.recall_at_10,
            self.overall.mrr,
            self.overall.ndcg_at_10,
            self.overall.n,
            self.intent_accuracy
        )?;
        for (ty, m) in &self.by_type {
            writeln!(
                f,
                "  {:<11} R@10={:.3}  MRR={:.3}  nDCG@10={:.3}  (n={})",
                format!("{ty:?}"),
                m.recall_at_10,
                m.mrr,
                m.ndcg_at_10,
                m.n
            )?;
        }
        for q in &self.per_query {
            if !q.missed.is_empty() {
                writeln!(f, "  MISS {:?} -> {}", q.query, q.missed.join(", "))?;
            }
        }
        Ok(())
    }
}

// ── metric primitives (pure, unit-tested) ─────────────────────────────────────

/// Fraction of `relevant` ids that appear in the first `k` of `ranked`. `1.0` when `relevant` is
/// empty (nothing to find → nothing missed).
pub fn recall_at_k(ranked: &[ekos_kir::KirId], relevant: &[ekos_kir::KirId], k: usize) -> f64 {
    if relevant.is_empty() {
        return 1.0;
    }
    let top: std::collections::HashSet<_> = ranked.iter().take(k).collect();
    let found = relevant.iter().filter(|r| top.contains(r)).count();
    found as f64 / relevant.len() as f64
}

/// `1 / (rank of the first relevant id)`, 1-based; `0.0` if none of `relevant` is in `ranked`.
/// `1.0` when `relevant` is empty (nothing to rank is not a ranking failure).
pub fn reciprocal_rank(ranked: &[ekos_kir::KirId], relevant: &[ekos_kir::KirId]) -> f64 {
    if relevant.is_empty() {
        return 1.0;
    }
    let rel: std::collections::HashSet<_> = relevant.iter().collect();
    for (i, id) in ranked.iter().enumerate() {
        if rel.contains(id) {
            return 1.0 / (i as f64 + 1.0);
        }
    }
    0.0
}

/// Binary-relevance nDCG@k: DCG (gain 1 per relevant hit, `1/log2(rank+1)` discount) over the
/// ideal DCG for `min(k, |relevant|)` hits. `1.0` when `relevant` is empty.
pub fn ndcg_at_k(ranked: &[ekos_kir::KirId], relevant: &[ekos_kir::KirId], k: usize) -> f64 {
    if relevant.is_empty() {
        return 1.0;
    }
    let rel: std::collections::HashSet<_> = relevant.iter().collect();
    let dcg: f64 = ranked
        .iter()
        .take(k)
        .enumerate()
        .filter(|(_, id)| rel.contains(id))
        .map(|(i, _)| 1.0 / ((i as f64 + 2.0).log2()))
        .sum();
    let ideal: f64 = (0..relevant.len().min(k))
        .map(|i| 1.0 / ((i as f64 + 2.0).log2()))
        .sum();
    if ideal == 0.0 { 1.0 } else { dcg / ideal }
}

fn mean(xs: impl Iterator<Item = f64>) -> f64 {
    let mut sum = 0.0;
    let mut n = 0usize;
    for x in xs {
        sum += x;
        n += 1;
    }
    if n == 0 { 0.0 } else { sum / n as f64 }
}

// ── the run ──────────────────────────────────────────────────────────────────

/// A query-text → embedding function — the RFC 0125 vector arm's query side, provided by the
/// caller so query and document embeddings share a provider.
pub type QueryEmbedder<'a> = &'a dyn Fn(&str) -> Vec<f32>;

/// Run every query in `queries` against `runtime` and score it. `embed_query`, when given, is
/// called once per query to produce the pre-computed embedding for the RFC 0125 vector arm — pass
/// `None` to measure the lexical-only stack.
pub fn evaluate(
    runtime: &Runtime,
    queries: &[EvalQuery],
    embed_query: Option<QueryEmbedder<'_>>,
) -> EvalReport {
    // name (lowercased) -> id, from the seeded estate.
    let name_to_id: HashMap<String, ekos_kir::KirId> = runtime
        .list_objects()
        .unwrap_or_default()
        .into_iter()
        .map(|o| (o.name.to_lowercase(), o.id))
        .collect();

    let mut per_query = Vec::with_capacity(queries.len());
    let mut intent_hits = 0usize;

    for q in queries {
        let relevant: Vec<ekos_kir::KirId> = q
            .relevant
            .iter()
            .filter_map(|n| name_to_id.get(&n.to_lowercase()).copied())
            .collect();

        let got_type = understand(q.query, runtime)
            .map(|u| u.query_type)
            .unwrap_or(QueryType::Lexical);
        if got_type == q.expect_type {
            intent_hits += 1;
        }

        let mut req = RetrievalRequest::lexical(q.query);
        if let Some(embed) = embed_query {
            req.query_embedding = Some(embed(q.query));
        }
        let ranked: Vec<ekos_kir::KirId> = runtime
            .retrieve(&req)
            .map(|r| r.hits.iter().map(|h| h.id).collect())
            .unwrap_or_default();

        let ranked_set: std::collections::HashSet<_> = ranked.iter().collect();
        let missed = q
            .relevant
            .iter()
            .zip(&relevant)
            .filter(|(_, id)| !ranked_set.contains(id))
            .map(|(n, _)| n.to_string())
            .collect();

        per_query.push(QueryOutcome {
            query: q.query,
            expect_type: q.expect_type,
            got_type,
            recall_at_10: recall_at_k(&ranked, &relevant, 10),
            reciprocal_rank: reciprocal_rank(&ranked, &relevant),
            ndcg_at_10: ndcg_at_k(&ranked, &relevant, 10),
            missed,
        });
    }

    let rank_scored: Vec<&QueryOutcome> = per_query
        .iter()
        .filter(|q| is_retrieval_type(q.expect_type))
        .collect();
    let overall = EvalMetrics {
        recall_at_10: mean(rank_scored.iter().map(|q| q.recall_at_10)),
        mrr: mean(rank_scored.iter().map(|q| q.reciprocal_rank)),
        ndcg_at_10: mean(rank_scored.iter().map(|q| q.ndcg_at_10)),
        n: rank_scored.len(),
    };

    let mut by_type: Vec<(QueryType, EvalMetrics)> = Vec::new();
    for ty in [
        QueryType::Lookup,
        QueryType::Lexical,
        QueryType::Conceptual,
        QueryType::Structural,
        QueryType::Aggregate,
    ] {
        let slice: Vec<&QueryOutcome> = per_query.iter().filter(|q| q.expect_type == ty).collect();
        if slice.is_empty() {
            continue;
        }
        by_type.push((
            ty,
            EvalMetrics {
                recall_at_10: mean(slice.iter().map(|q| q.recall_at_10)),
                mrr: mean(slice.iter().map(|q| q.reciprocal_rank)),
                ndcg_at_10: mean(slice.iter().map(|q| q.ndcg_at_10)),
                n: slice.len(),
            },
        ));
    }

    EvalReport {
        overall,
        by_type,
        intent_accuracy: intent_hits as f64 / queries.len().max(1) as f64,
        per_query,
    }
}

// ── the baseline + regression gate ───────────────────────────────────────────

/// A checked-in snapshot of a real [`evaluate`] run — the CI gate compares against this.
#[derive(Debug, Clone, Copy)]
pub struct EvalBaseline {
    pub recall_at_10: f64,
    pub mrr: f64,
    pub ndcg_at_10: f64,
    pub intent_accuracy: f64,
}

/// Captured 2026-09-01 from the reference estate via
/// `cargo test -p ekos-runtime retrieval_eval::tests::print_current -- --ignored --nocapture`
/// (observed R@10 0.841 / MRR 0.739 / nDCG 0.745 / intent 0.862, set a hair below with a 0.02
/// tolerance on top). Overall metrics are over the retrieval-shaped types (Lookup / Lexical /
/// Conceptual) only. Update only with a real regeneration + a PR note on what moved it.
pub const BASELINE: EvalBaseline = EvalBaseline {
    recall_at_10: 0.84,
    mrr: 0.73,
    ndcg_at_10: 0.74,
    intent_accuracy: 0.85,
};

/// `Err` (with a human-readable list) when any metric is more than `tol` below its baseline.
pub fn check_regression(
    report: &EvalReport,
    baseline: &EvalBaseline,
    tol: f64,
) -> Result<(), String> {
    let mut drops = Vec::new();
    let mut check = |name: &str, got: f64, want: f64| {
        if got < want - tol {
            drops.push(format!("  {name}: {got:.3} < {want:.3} - {tol:.2}"));
        }
    };
    check(
        "recall_at_10",
        report.overall.recall_at_10,
        baseline.recall_at_10,
    );
    check("mrr", report.overall.mrr, baseline.mrr);
    check("ndcg_at_10", report.overall.ndcg_at_10, baseline.ndcg_at_10);
    check(
        "intent_accuracy",
        report.intent_accuracy,
        baseline.intent_accuracy,
    );
    if drops.is_empty() {
        Ok(())
    } else {
        Err(drops.join("\n"))
    }
}

// ── the reference estate ─────────────────────────────────────────────────────

fn table(name: &str, columns: &[&str]) -> KirObject {
    KirObject::new(name, ObjectKind::Table)
        .with_property("columns", serde_json::json!(columns))
        // `symbols` is in `indexed_content()` — this is how a real analyzer makes a table's
        // column names lexically findable ("territory description" → `Territories`).
        .with_property("symbols", serde_json::json!(columns))
}

fn with_overview(mut o: KirObject, overview: &str) -> KirObject {
    o = o.with_property("ai_overview", serde_json::json!(overview));
    o
}

/// Seed a fixed, realistic-shaped estate: the Northwind schema (tables + FK edges), three code
/// modules with symbols carrying `ai_overview` prose, and a handful of doc sections. Deterministic
/// and offline. Panics on a store error — this is test/bench scaffolding only.
pub fn seed_reference_estate(store: &dyn KnowledgeStore) {
    let mut ids: HashMap<&str, ekos_kir::KirId> = HashMap::new();
    let mut put = |o: KirObject, key: &'static str| {
        ids.insert(key, o.id);
        store.append_object(&o).expect("seed append_object");
    };

    // Northwind-ish tables.
    put(
        table(
            "Customers",
            &["CustomerID", "CompanyName", "ContactName", "Country"],
        ),
        "Customers",
    );
    put(
        table(
            "Orders",
            &[
                "OrderID",
                "CustomerID",
                "EmployeeID",
                "OrderDate",
                "ShipVia",
            ],
        ),
        "Orders",
    );
    put(
        table(
            "Order Details",
            &["OrderID", "ProductID", "UnitPrice", "Quantity", "Discount"],
        ),
        "Order Details",
    );
    put(
        table(
            "Products",
            &[
                "ProductID",
                "ProductName",
                "SupplierID",
                "CategoryID",
                "UnitPrice",
            ],
        ),
        "Products",
    );
    put(
        table("Categories", &["CategoryID", "CategoryName", "Description"]),
        "Categories",
    );
    put(
        table("Suppliers", &["SupplierID", "CompanyName", "Country"]),
        "Suppliers",
    );
    put(
        table(
            "Employees",
            &["EmployeeID", "LastName", "FirstName", "ReportsTo"],
        ),
        "Employees",
    );
    put(
        table("Shippers", &["ShipperID", "CompanyName", "Phone"]),
        "Shippers",
    );
    put(
        table("Region", &["RegionID", "RegionDescription"]),
        "Region",
    );
    put(
        table(
            "Territories",
            &["TerritoryID", "TerritoryDescription", "RegionID"],
        ),
        "Territories",
    );

    // Code modules + symbols. The `ai_overview` prose is what a `Conceptual` query has to match
    // when the object *name* shares no token with the question.
    put(
        with_overview(
            KirObject::new("notifications", ObjectKind::Custom("Module".into())),
            "Outbound transactional messaging: email and SMS delivery for account and order events.",
        ),
        "notifications",
    );
    put(
        with_overview(
            KirObject::new(
                "dispatch_signup_notification",
                ObjectKind::Custom("Symbol".into()),
            ),
            "Sends the welcome email to a customer who has just registered for a new account.",
        ),
        "dispatch_signup_notification",
    );
    put(
        with_overview(
            KirObject::new(
                "send_order_confirmation",
                ObjectKind::Custom("Symbol".into()),
            ),
            "Emails the customer a receipt once their order is paid and accepted.",
        ),
        "send_order_confirmation",
    );
    put(
        with_overview(
            KirObject::new("billing", ObjectKind::Custom("Module".into())),
            "Charges customer payment methods and reconciles settled transactions against orders.",
        ),
        "billing",
    );
    put(
        with_overview(
            KirObject::new("charge_payment_method", ObjectKind::Custom("Symbol".into())),
            "Captures funds from a stored credit card for the total amount of an order.",
        ),
        "charge_payment_method",
    );
    put(
        with_overview(
            KirObject::new("reconcile_settlements", ObjectKind::Custom("Symbol".into())),
            "Nightly job that matches bank settlement files to recorded payments and flags gaps.",
        ),
        "reconcile_settlements",
    );
    put(
        with_overview(
            KirObject::new("inventory", ObjectKind::Custom("Module".into())),
            "Tracks stock levels per product and decrements them as orders are fulfilled.",
        ),
        "inventory",
    );
    put(
        with_overview(
            KirObject::new("reserve_stock", ObjectKind::Custom("Symbol".into())),
            "Holds units of a product for a pending order so they cannot be double-sold.",
        ),
        "reserve_stock",
    );

    // Doc sections.
    put(
        with_overview(
            KirObject::new(
                "Onboarding a new customer",
                ObjectKind::Custom("Section".into()),
            ),
            "How the sales team creates a customer record and the automated welcome sequence that follows.",
        ),
        "Onboarding a new customer",
    );
    put(
        with_overview(
            KirObject::new("Payment retry policy", ObjectKind::Custom("Section".into())),
            "When a card charge fails, the schedule on which billing retries before the order is cancelled.",
        ),
        "Payment retry policy",
    );
    put(
        with_overview(
            KirObject::new("Data retention", ObjectKind::Custom("Section".into())),
            "How long order, customer, and payment records are kept before archival and deletion.",
        ),
        "Data retention",
    );

    let rel = |from: &str, to: &str| {
        KirRelationship::new(RelationshipKind::ForeignKey, ids[from], ids[to])
    };
    let dep = |from: &str, to: &str| {
        KirRelationship::new(RelationshipKind::DependsOn, ids[from], ids[to])
    };
    for r in [
        rel("Orders", "Customers"),
        rel("Orders", "Employees"),
        rel("Orders", "Shippers"),
        rel("Order Details", "Orders"),
        rel("Order Details", "Products"),
        rel("Products", "Categories"),
        rel("Products", "Suppliers"),
        rel("Employees", "Employees"),
        rel("Territories", "Region"),
        dep("dispatch_signup_notification", "notifications"),
        dep("send_order_confirmation", "notifications"),
        dep("charge_payment_method", "billing"),
        dep("reconcile_settlements", "billing"),
        dep("reserve_stock", "inventory"),
        dep("send_order_confirmation", "billing"),
    ] {
        store
            .append_relationship(&r)
            .expect("seed append_relationship");
    }
}

/// Build the RFC 0125 vector index over an already-seeded estate, at `<ledger-root>/vectors/`,
/// using the deterministic mock embedder. Returns a `|query| Vec<f32>` closure to hand to
/// [`evaluate`] so query and document embeddings come from the same provider.
pub async fn seed_reference_vectors(
    store: &dyn KnowledgeStore,
    index_dir: &std::path::Path,
) -> impl Fn(&str) -> Vec<f32> + use<> {
    let provider = ekos_recovery::MockEmbeddingProvider::default();
    ekos_recovery::embed_objects(
        store,
        &provider,
        index_dir,
        &ekos_common::redaction::RedactionConfig::default(),
    )
    .await
    .expect("seed vector index");
    move |q: &str| provider.embed_sync(q)
}

/// The graded query set — ~40 queries across all five `QueryType`s.
pub fn reference_queries() -> &'static [EvalQuery] {
    use QueryType::*;
    &[
        // Lookup — a bare exact entity name.
        EvalQuery {
            query: "Customers",
            expect_type: Lookup,
            relevant: &["Customers"],
        },
        EvalQuery {
            query: "Orders",
            expect_type: Lookup,
            relevant: &["Orders"],
        },
        EvalQuery {
            query: "Products",
            expect_type: Lookup,
            relevant: &["Products"],
        },
        EvalQuery {
            query: "Suppliers",
            expect_type: Lookup,
            relevant: &["Suppliers"],
        },
        EvalQuery {
            query: "Shippers",
            expect_type: Lookup,
            relevant: &["Shippers"],
        },
        EvalQuery {
            query: "reserve_stock",
            expect_type: Lookup,
            relevant: &["reserve_stock"],
        },
        // Lexical — keywords that appear in a name.
        EvalQuery {
            query: "order details",
            expect_type: Lexical,
            relevant: &["Order Details"],
        },
        EvalQuery {
            query: "category name",
            expect_type: Lexical,
            relevant: &["Categories"],
        },
        EvalQuery {
            query: "employee reports to",
            expect_type: Lexical,
            relevant: &["Employees"],
        },
        EvalQuery {
            query: "territory description",
            expect_type: Lexical,
            relevant: &["Territories"],
        },
        EvalQuery {
            query: "signup notification",
            expect_type: Lexical,
            relevant: &["dispatch_signup_notification"],
        },
        EvalQuery {
            query: "payment method charge",
            expect_type: Lexical,
            relevant: &["charge_payment_method"],
        },
        EvalQuery {
            query: "reconcile settlements",
            expect_type: Lexical,
            relevant: &["reconcile_settlements"],
        },
        EvalQuery {
            query: "order confirmation",
            expect_type: Lexical,
            relevant: &["send_order_confirmation"],
        },
        // Conceptual — token-disjoint from the target's name; matches `ai_overview` prose.
        EvalQuery {
            query: "the thing that emails a new customer when they sign up",
            expect_type: Conceptual,
            relevant: &["dispatch_signup_notification", "notifications"],
        },
        EvalQuery {
            query: "where do we take money from a saved card",
            expect_type: Conceptual,
            relevant: &["charge_payment_method", "billing"],
        },
        EvalQuery {
            query: "how we avoid selling the same item twice",
            expect_type: Conceptual,
            relevant: &["reserve_stock", "inventory"],
        },
        EvalQuery {
            query: "matching bank files against what we recorded",
            expect_type: Conceptual,
            relevant: &["reconcile_settlements"],
        },
        EvalQuery {
            query: "the receipt sent after checkout",
            expect_type: Conceptual,
            relevant: &["send_order_confirmation"],
        },
        EvalQuery {
            query: "rules for keeping records before we delete them",
            expect_type: Conceptual,
            relevant: &["Data retention"],
        },
        EvalQuery {
            query: "what happens when a card is declined",
            expect_type: Conceptual,
            relevant: &["Payment retry policy"],
        },
        EvalQuery {
            query: "how a customer first gets set up",
            expect_type: Conceptual,
            relevant: &["Onboarding a new customer"],
        },
        // Structural — intent recognition is the check (retrieve alone won't traverse).
        EvalQuery {
            query: "what depends on the Orders table",
            expect_type: Structural,
            relevant: &["Orders"],
        },
        EvalQuery {
            query: "what depends on Customers",
            expect_type: Structural,
            relevant: &["Customers"],
        },
        EvalQuery {
            query: "callers of charge_payment_method",
            expect_type: Structural,
            relevant: &["charge_payment_method"],
        },
        EvalQuery {
            query: "what breaks if I change Products",
            expect_type: Structural,
            relevant: &["Products"],
        },
        // Aggregate — hand back to EKL COUNT/GROUP BY.
        EvalQuery {
            query: "how many tables are there",
            expect_type: Aggregate,
            relevant: &[],
        },
        EvalQuery {
            query: "count the modules",
            expect_type: Aggregate,
            relevant: &[],
        },
        EvalQuery {
            query: "list all tables by number of columns",
            expect_type: Aggregate,
            relevant: &[],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::KirId;

    fn ids(n: usize) -> Vec<KirId> {
        (0..n).map(|_| KirId::new()).collect()
    }

    #[test]
    fn recall_at_k_perfect_and_partial() {
        let all = ids(5);
        assert_eq!(recall_at_k(&all, &all[..2], 10), 1.0);
        // only 1 of the 2 relevant is in the top-2
        let ranked = vec![all[0], all[3], all[1], all[2]];
        assert_eq!(recall_at_k(&ranked, &[all[0], all[1]], 2), 0.5);
        assert_eq!(recall_at_k(&[], &all[..1], 10), 0.0);
        assert_eq!(recall_at_k(&all, &[], 10), 1.0, "empty relevant → 1.0");
    }

    #[test]
    fn reciprocal_rank_positions() {
        let a = ids(4);
        assert_eq!(reciprocal_rank(&a, &[a[0]]), 1.0);
        assert_eq!(reciprocal_rank(&a, &[a[2]]), 1.0 / 3.0);
        assert_eq!(reciprocal_rank(&a, &[KirId::new()]), 0.0);
    }

    /// Not a gate — run with `--ignored --nocapture` to print a paste-ready `BASELINE` block
    /// after a deliberate retrieval change.
    #[tokio::test]
    #[ignore]
    async fn print_current() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("fl");
        let fl = ekos_ledger::FactLedger::open(&root).unwrap();
        seed_reference_estate(&fl);
        let embed = seed_reference_vectors(&fl, &root.join("vectors")).await;
        let rt = Runtime::over(&fl);
        let report = evaluate(&rt, reference_queries(), Some(&embed));
        eprintln!("{report}");
        eprintln!(
            "pub const BASELINE: EvalBaseline = EvalBaseline {{\n    \
             recall_at_10: {:.2},\n    mrr: {:.2},\n    ndcg_at_10: {:.2},\n    \
             intent_accuracy: {:.2},\n}};",
            report.overall.recall_at_10,
            report.overall.mrr,
            report.overall.ndcg_at_10,
            report.intent_accuracy,
        );
    }

    #[test]
    fn ndcg_at_k_ideal_and_reordered() {
        let a = ids(4);
        // perfect: both relevant at the front
        assert!((ndcg_at_k(&a, &[a[0], a[1]], 10) - 1.0).abs() < 1e-9);
        // one relevant demoted to rank 3 → below 1.0 but > 0
        let n = ndcg_at_k(&a, &[a[0], a[2]], 10);
        assert!(n > 0.0 && n < 1.0, "got {n}");
        assert_eq!(ndcg_at_k(&a, &[], 10), 1.0);
    }
}
