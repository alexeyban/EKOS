//! RFC 0119 (Phase 0 of RFC 0118) — the retrieval seam.
//!
//! A single scored, multi-signal retrieval surface on [`KnowledgeStore`](crate::KnowledgeStore),
//! with a default implementation that wraps `find_objects` so every existing implementor compiles
//! unchanged and produces **byte-identical** id ordering. Fusion, the exact-name signal, and the
//! vector/graph arms arrive in RFC 0120+.

use ekos_kir::{KirId, ObjectKind};

/// Reciprocal Rank Fusion constant. Shared with the real fuser in RFC 0120 so a Phase 0
/// rank-only score is directly comparable to a Phase 1 fused score.
pub const RRF_K: f32 = 60.0;

/// Which **store-local** retrieval arms to run. The graph arm is not store-local (it needs the
/// `Runtime`), so it is not represented here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmSet {
    pub bm25: bool,
    pub vector: bool,
}

impl ArmSet {
    /// BM25 only — the Phase 0 default.
    pub const LEXICAL: ArmSet = ArmSet {
        bm25: true,
        vector: false,
    };
}

/// Built by the Runtime-level orchestrator (RFC 0121+), never by end consumers. Handed to the
/// sync [`KnowledgeStore::retrieve`](crate::KnowledgeStore::retrieve).
#[derive(Debug, Clone)]
pub struct RetrievalRequest {
    /// Raw user text, verbatim. Drives exact-name logic (RFC 0120) and is the BM25 query when
    /// `keywords` is unset.
    pub raw: String,
    /// Processed keyword / boolean form for BM25 (`"a AND b"` / `"a OR b"`). Wins over `raw`.
    pub keywords: Option<String>,
    /// Pre-computed, L2-normalizable query embedding. The vector arm is skipped unless this is
    /// `Some` **and** its length matches the on-disk vector index header dim (RFC 0125). Inert in
    /// Phase 0.
    pub query_embedding: Option<Vec<f32>>,
    pub arms: ArmSet,
    /// Candidates pulled from each arm before fusion.
    pub per_arm_limit: usize,
    /// Final cap after fusion.
    pub limit: usize,
}

impl RetrievalRequest {
    /// BM25-only, `limit` 50 — the drop-in replacement for a `find_objects` call.
    pub fn lexical(raw: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            keywords: None,
            query_embedding: None,
            arms: ArmSet::LEXICAL,
            per_arm_limit: 50,
            limit: 50,
        }
    }

    /// The string handed to BM25: the processed `keywords` form if present, else the raw text.
    pub fn bm25_query(&self) -> &str {
        self.keywords.as_deref().unwrap_or(&self.raw)
    }
}

/// Where a hit came from and how it ranked there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Signal {
    pub source: SignalSource,
    /// 0-based rank within that source's own result list.
    pub rank: u32,
    /// The source's raw score. `0.0` when unknown — `find_objects` exposes no score, so every
    /// Phase 0 signal carries `0.0` here.
    pub raw_score: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalSource {
    Bm25,
    Vector,
    Graph,
    ExactName,
}

/// One ranked knowledge object, with the per-arm evidence for *why* it surfaced.
#[derive(Debug, Clone)]
pub struct Hit {
    pub id: KirId,
    pub name: String,
    /// `None` until `f_kind` is `STORED` (RFC 0120) or the orchestrator hydrates the final top-N.
    pub kind: Option<ObjectKind>,
    /// Fused score, strictly decreasing across `RankedResults::hits`. Rank-only in Phase 0:
    /// `1.0 / (RRF_K + rank)`.
    pub score: f32,
    pub signals: Vec<Signal>,
}

/// The result of a [`retrieve`](crate::KnowledgeStore::retrieve) call.
#[derive(Debug, Clone)]
pub struct RankedResults {
    pub hits: Vec<Hit>,
    /// The arms that actually ran (not just the ones requested). `vector` drops to `false` when
    /// no index is on disk or no query embedding was supplied.
    pub arms_run: ArmSet,
}

impl RankedResults {
    /// Wrap a `find_objects`-style ranked `(id, name)` list as a single-signal, rank-only
    /// `RankedResults` — the Phase 0 default-impl body. `pairs` is assumed best-first.
    pub fn from_ranked_pairs(
        pairs: Vec<(KirId, String)>,
        source: SignalSource,
        limit: usize,
    ) -> Self {
        let hits = pairs
            .into_iter()
            .take(limit)
            .enumerate()
            .map(|(i, (id, name))| {
                let rank = i as u32;
                Hit {
                    id,
                    name,
                    kind: None,
                    score: 1.0 / (RRF_K + rank as f32),
                    signals: vec![Signal {
                        source,
                        rank,
                        raw_score: 0.0,
                    }],
                }
            })
            .collect();
        Self {
            hits,
            arms_run: ArmSet::LEXICAL,
        }
    }

    /// The hit ids, best-first — the shape `find_objects` consumers reduce to.
    pub fn ids(&self) -> Vec<KirId> {
        self.hits.iter().map(|h| h.id).collect()
    }

    /// `(id, name)` pairs, best-first — the exact legacy `find_objects` return shape.
    pub fn into_pairs(self) -> Vec<(KirId, String)> {
        self.hits.into_iter().map(|h| (h.id, h.name)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FactLedger, KnowledgeStore};
    use ekos_kir::{KirObject, ObjectKind};
    use tempfile::tempdir;

    fn ledger_with(names: &[&str]) -> (FactLedger, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let l = FactLedger::open(&dir.path().join("fl")).unwrap();
        for n in names {
            l.append_object(&KirObject::new(*n, ObjectKind::Table))
                .unwrap();
        }
        (l, dir)
    }

    #[test]
    fn default_retrieve_is_byte_identical_to_find_objects() {
        let (l, _d) = ledger_with(&["orders", "orders_archive", "customers", "order_items"]);
        for q in [
            "orders",
            "order*",
            "\"order items\"",
            "nonexistent",
            "customers",
        ] {
            let legacy: Vec<KirId> = l
                .find_objects(q)
                .unwrap()
                .into_iter()
                .map(|(id, _)| id)
                .collect();
            let seam = l.retrieve(&RetrievalRequest::lexical(q)).unwrap();
            assert_eq!(
                seam.ids(),
                legacy,
                "query {q:?}: seam id order must match find_objects"
            );
            assert_eq!(seam.arms_run, ArmSet::LEXICAL);
            // scores strictly decreasing, one Bm25 signal per hit
            for w in seam.hits.windows(2) {
                assert!(w[0].score > w[1].score, "scores must strictly decrease");
            }
            for (i, h) in seam.hits.iter().enumerate() {
                assert_eq!(
                    h.signals,
                    vec![Signal {
                        source: SignalSource::Bm25,
                        rank: i as u32,
                        raw_score: 0.0
                    }]
                );
                assert!(h.kind.is_none(), "Phase 0 leaves kind unpopulated");
            }
        }
    }

    #[test]
    fn limit_truncates() {
        let (l, _d) = ledger_with(&["order_a", "order_b", "order_c", "order_d"]);
        let mut req = RetrievalRequest::lexical("order");
        req.limit = 2;
        assert_eq!(l.retrieve(&req).unwrap().hits.len(), 2);
    }

    #[test]
    fn bm25_query_prefers_keywords_over_raw() {
        let req = RetrievalRequest {
            raw: "the whole question".into(),
            keywords: Some("whole question".into()),
            ..RetrievalRequest::lexical("the whole question")
        };
        assert_eq!(req.bm25_query(), "whole question");
        assert_eq!(RetrievalRequest::lexical("bare").bm25_query(), "bare");
    }
}
